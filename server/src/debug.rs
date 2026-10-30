//! Board debugging (Boards → Debug): per channel each board's envelope, its
//! thresholds and the passes it would detect on its own, next to the boards
//! of each gate combined (as the laps are counted). Live for the last minute
//! (`Analysis` in `Core`, fed with every board's levels, also those of boards
//! set to "off"), and for a level file: a recording made here or a session's.
//!
//! The detectors are the ones of `detect.rs` with the current settings (a
//! session: its own), one per gate and one per board. They run besides the
//! ones that count the laps and change nothing there.
//!
//! Recordings (`Recording`): `<data>/recordings/rec_<date>_<time>.csv`, the
//! format of the session level files, plus `# board` lines (id and name) and
//! `# detection:` (the settings, so that opening it later shows the same).
//! `laptimer replay` reads them too.
//!
//! - `GET /api/debug/live?after_us&window_s&channels=0,2`: the slots after
//!   `after_us` (at most `window_s` back, 10 ms each, level codes), the
//!   passes of that time, the boards, the recording in progress.
//! - `POST /api/debug/record` starts a recording, `POST /api/debug/record/stop` ends it.
//! - `GET /api/debug/recordings` lists them, `GET`/`DELETE /api/debug/recordings/{name}`.
//! - `GET /api/debug/graph?source=rec:<name>|session:<id>&channel&from_us&to_us&points`:
//!   a level file worked through, in at most `points` points per series.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::core::BoardRole;
use crate::detect::{DetectConfig, DetectionSettings, Detector, Event, TracePoint};
use crate::esp::board::Level;
use crate::esp::frame::rssi_dbfs;
use crate::server::App;

/// Series: 0 the finish gate's boards combined, 1 the arming gate's, then
/// one per board (2 + board index).
const FINISH: usize = 0;
const ARMING: usize = 1;
const FIRST_BOARD: usize = 2;

/// Live: the slots of the last minute.
pub const LIVE_KEEP_US: i64 = 60_000_000;

/// How the detectors are set up: the current settings, or a session's.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectSetup {
    pub n_channels: usize,
    pub min_lap_us: i64,
    pub detection: DetectionSettings,
}

impl DetectSetup {
    fn detector(&self, arming: bool) -> Detector {
        let config = DetectConfig {
            min_lap_us: self.min_lap_us,
            min_rise_db: self.detection.min_rise_db,
        };
        let mut d = Detector::with_trace(self.n_channels, config);
        d.set_detection(&self.detection, arming);
        d
    }
}

/// A board as the debug page shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugBoard {
    pub index: usize,
    pub id: String,
    pub name: Option<String>,
    pub role: BoardRole,
}

/// A pass one of the detectors reported.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct DebugPass {
    pub series: usize,
    pub channel: u8,
    /// Wall clock µs.
    pub time_us: i64,
    pub peak_db: f32,
    /// Taken back later (start-up of the automatic thresholds).
    pub retracted: bool,
}

/// The detectors and what they recorded.
pub struct Analysis {
    setup: DetectSetup,
    /// host µs + this = wall clock µs.
    offset_us: i64,
    roles: Vec<BoardRole>,
    finish: Detector,
    arming: Detector,
    /// One per board; made when its first levels come.
    singles: Vec<Detector>,
    /// Per series, per channel: the slots (wall clock µs), oldest first.
    slots: Vec<Vec<VecDeque<TracePoint>>>,
    passes: Vec<DebugPass>,
    /// Live: drop what is older than this. None: keep everything (a file).
    keep_us: Option<i64>,
}

impl Analysis {
    pub fn new(setup: DetectSetup, roles: Vec<BoardRole>, offset_us: i64, keep_us: Option<i64>) -> Analysis {
        Analysis {
            finish: setup.detector(false),
            arming: setup.detector(true),
            setup,
            offset_us,
            roles,
            singles: Vec::new(),
            slots: Vec::new(),
            passes: Vec::new(),
            keep_us,
        }
    }

    /// New detection settings, keeping what the detectors saw and learned
    /// (as `Detector::set_detection`).
    pub fn set_detection(&mut self, detection: &DetectionSettings) {
        self.finish.set_detection(detection, false);
        self.arming.set_detection(detection, true);
        for (i, d) in self.singles.iter_mut().enumerate() {
            d.set_detection(detection, self.roles.get(i) == Some(&BoardRole::Arming));
        }
        self.setup.detection = detection.clone();
    }

    /// One board's levels: into its gate's detector and its own.
    pub fn push(&mut self, board: usize, levels: &[Level]) {
        match self.roles.get(board).copied().unwrap_or_default() {
            BoardRole::Finish => self.finish.push(board, levels),
            BoardRole::Arming => self.arming.push(board, levels),
            BoardRole::Off => {}
        }
        while self.singles.len() <= board {
            self.singles.push(self.setup.detector(self.roles.get(self.singles.len()) == Some(&BoardRole::Arming)));
        }
        // To its own detector the board is the only one: board 0.
        self.singles[board].push(0, levels);
    }

    /// Finishes the slots all detectors can, and collects them.
    pub fn advance(&mut self) {
        let events = self.finish.advance();
        collect(FINISH, &mut self.finish, events, self.offset_us, &mut self.slots, &mut self.passes);
        let events = self.arming.advance();
        collect(ARMING, &mut self.arming, events, self.offset_us, &mut self.slots, &mut self.passes);
        for (i, d) in self.singles.iter_mut().enumerate() {
            let events = d.advance();
            collect(FIRST_BOARD + i, d, events, self.offset_us, &mut self.slots, &mut self.passes);
        }
        self.trim();
    }

    /// At the end of a file: everything, also the last passes' hold time.
    fn flush(&mut self) {
        let events = self.finish.flush();
        collect(FINISH, &mut self.finish, events, self.offset_us, &mut self.slots, &mut self.passes);
        let events = self.arming.flush();
        collect(ARMING, &mut self.arming, events, self.offset_us, &mut self.slots, &mut self.passes);
        for (i, d) in self.singles.iter_mut().enumerate() {
            let events = d.flush();
            collect(FIRST_BOARD + i, d, events, self.offset_us, &mut self.slots, &mut self.passes);
        }
    }

    /// Live: drops what is older than `keep_us`.
    fn trim(&mut self) {
        let Some(keep) = self.keep_us else { return };
        let Some(newest) = self.newest_us() else { return };
        let oldest = newest - keep;
        for series in &mut self.slots {
            for ch in series {
                while ch.front().is_some_and(|p| p.time_us < oldest) {
                    ch.pop_front();
                }
            }
        }
        self.passes.retain(|p| p.time_us >= oldest);
    }

    fn newest_us(&self) -> Option<i64> {
        self.slots.iter().flatten().filter_map(|c| c.back()).map(|p| p.time_us).max()
    }

    /// First and last slot of any series, wall clock µs.
    pub fn bounds(&self) -> Option<(i64, i64)> {
        let first = self.slots.iter().flatten().filter_map(|c| c.front()).map(|p| p.time_us).min()?;
        Some((first, self.newest_us()?))
    }

    /// Live: per series and asked-for channel the slots after `after_us`
    /// (as level codes, 0 = none; one every 10 ms from `t0`), and the passes
    /// since `from_us`.
    pub fn live_json(&self, after_us: i64, from_us: i64, channels: &[usize]) -> Value {
        let since = after_us.max(from_us);
        let mut series = Vec::new();
        for (s, chans) in self.slots.iter().enumerate() {
            let mut out = Vec::new();
            for &c in channels {
                let Some(pts) = chans.get(c) else { continue };
                // The slots are in time order: skip the old ones from the front.
                let start = pts.partition_point(|p| p.time_us <= since);
                let new: Vec<&TracePoint> = pts.range(start..).collect();
                let Some(first) = new.first() else { continue };
                out.push(json!({
                    "channel": c,
                    "t0": first.time_us,
                    "level": new.iter().map(|p| p.level).collect::<Vec<_>>(),
                    "enter": new.iter().map(|p| p.enter).collect::<Vec<_>>(),
                    "exit": new.iter().map(|p| p.exit).collect::<Vec<_>>(),
                }));
            }
            if !out.is_empty() {
                series.push(json!({ "series": s, "channels": out }));
            }
        }
        let passes: Vec<&DebugPass> = self
            .passes
            .iter()
            .filter(|p| p.time_us >= from_us && channels.contains(&(p.channel as usize)))
            .collect();
        json!({ "series": series, "passes": passes })
    }

    /// One channel from `from_us` to `to_us` in `n` equal parts; per part
    /// and series the highest level (no peak gets lost), with the thresholds
    /// at that point. dBFS, null where there is nothing.
    pub fn graph_json(&self, channel: usize, from_us: i64, to_us: i64, n: usize) -> Value {
        let n = n.clamp(10, 5000);
        let per = ((to_us - from_us) as f64 / n as f64).max(1.0);
        let db = |code: u8| (code > 0).then(|| rssi_dbfs(code));
        let mut series = Vec::new();
        for (s, chans) in self.slots.iter().enumerate() {
            let Some(pts) = chans.get(channel) else { continue };
            if pts.is_empty() {
                continue;
            }
            let mut top: Vec<Option<TracePoint>> = vec![None; n];
            let start = pts.partition_point(|p| p.time_us < from_us);
            for p in pts.range(start..) {
                if p.time_us > to_us {
                    break;
                }
                let i = (((p.time_us - from_us) as f64 / per) as usize).min(n - 1);
                if top[i].is_none_or(|t| p.level > t.level) {
                    top[i] = Some(*p);
                }
            }
            if top.iter().all(|t| t.is_none_or(|p| p.level == 0)) {
                continue;
            }
            series.push(json!({
                "series": s,
                "level": top.iter().map(|t| t.and_then(|p| db(p.level))).collect::<Vec<_>>(),
                "enter": top.iter().map(|t| t.and_then(|p| db(p.enter))).collect::<Vec<_>>(),
                "exit": top.iter().map(|t| t.and_then(|p| db(p.exit))).collect::<Vec<_>>(),
            }));
        }
        let time_us: Vec<i64> = (0..n).map(|i| from_us + ((i as f64 + 0.5) * per) as i64).collect();
        let passes: Vec<&DebugPass> = self
            .passes
            .iter()
            .filter(|p| p.channel as usize == channel && p.time_us >= from_us && p.time_us <= to_us)
            .collect();
        json!({ "time_us": time_us, "series": series, "passes": passes })
    }

    /// Every pass of a channel (the page compares the boards' passes).
    pub fn passes(&self, channel: usize) -> Vec<DebugPass> {
        self.passes.iter().filter(|p| p.channel as usize == channel).copied().collect()
    }
}

/// Takes a detector's new slots and passes into the series `series`.
fn collect(
    series: usize,
    det: &mut Detector,
    events: Vec<Event>,
    offset_us: i64,
    slots: &mut Vec<Vec<VecDeque<TracePoint>>>,
    passes: &mut Vec<DebugPass>,
) {
    for e in events {
        match e {
            Event::Pass(p) => passes.push(DebugPass {
                series,
                channel: p.channel,
                time_us: p.time_us + offset_us,
                peak_db: p.peak_db,
                retracted: false,
            }),
            Event::Retract(p) => {
                let t = p.time_us + offset_us;
                if let Some(x) = passes
                    .iter_mut()
                    .find(|x| x.series == series && x.channel == p.channel && x.time_us == t)
                {
                    x.retracted = true;
                }
            }
        }
    }
    let trace = det.drain_trace();
    if trace.iter().all(|c| c.is_empty()) {
        return;
    }
    while slots.len() <= series {
        slots.push(Vec::new());
    }
    let chans = &mut slots[series];
    while chans.len() < trace.len() {
        chans.push(VecDeque::new());
    }
    for (c, pts) in trace.into_iter().enumerate() {
        chans[c].extend(pts.into_iter().map(|p| TracePoint {
            time_us: p.time_us + offset_us,
            ..p
        }));
    }
}

// --- level files ---

/// A level file worked through, with what the page needs to label it.
pub struct FileAnalysis {
    pub analysis: Analysis,
    pub boards: Vec<DebugBoard>,
    /// "R1@5658" per channel.
    pub channels: Vec<String>,
}

/// The boards' roles from a level file's "# roles: 0=finish 1=arming" line
/// (after the colon), by board index.
pub fn parse_roles(rest: &str) -> Vec<BoardRole> {
    let mut roles = Vec::new();
    for (b, role) in rest.split_whitespace().filter_map(|r| r.split_once('=')) {
        let (Ok(b), Ok(role)) = (b.parse::<usize>(), serde_json::from_value(json!(role))) else {
            continue;
        };
        if roles.len() <= b {
            roles.resize(b + 1, BoardRole::Finish);
        }
        roles[b] = role;
    }
    roles
}

/// The channels in a level file's header ("R1@5658" …), without reading
/// the levels.
pub fn file_channels(path: &Path) -> Result<Vec<String>> {
    let file = File::open(path).with_context(|| format!("{}", path.display()))?;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if let Some(rest) = line.strip_prefix("# channels:") {
            return Ok(parse_channels(rest));
        }
        if !line.starts_with('#') {
            break;
        }
    }
    Ok(Vec::new())
}

/// What a level file holds, from its header and its first and last line
/// (for the lists to pick one; nothing is worked through).
#[derive(Debug, Clone, Serialize)]
pub struct FileInfo {
    /// "R1@5658", …
    pub channels: Vec<String>,
    /// The boards with their role and name (`# board` lines; older session
    /// files only have `# roles:`, then without names; neither: none).
    pub boards: Vec<DebugBoard>,
    /// From the first to the last level, seconds.
    pub duration_s: Option<f64>,
}

pub fn file_info(path: &Path) -> Result<FileInfo> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = File::open(path).with_context(|| format!("{}", path.display()))?;
    let mut channels = Vec::new();
    let mut roles = Vec::new();
    let mut boards: Vec<DebugBoard> = Vec::new();
    let mut first_us = None;
    for line in BufReader::new(&mut file).lines() {
        let line = line?;
        if let Some(rest) = line.strip_prefix("# channels:") {
            channels = parse_channels(rest);
        } else if let Some(rest) = line.strip_prefix("# roles:") {
            roles = parse_roles(rest);
        } else if let Some((_, j)) = line.strip_prefix("# board ").and_then(|r| r.split_once(' ')) {
            if let Ok(b) = serde_json::from_str::<DebugBoard>(j) {
                boards.push(b);
            }
        } else if !line.starts_with('#') && !line.starts_with("board") {
            first_us = host_us(&line);
            break;
        }
    }
    // Boards without a "# board" line (session files): their number and role.
    for (i, role) in roles.iter().enumerate() {
        if !boards.iter().any(|b| b.index == i) {
            boards.push(DebugBoard { index: i, id: format!("{}", i + 1), name: None, role: *role });
        }
    }
    boards.sort_by_key(|b| b.index);
    // The last whole line: in the last 4 KB.
    let len = file.seek(SeekFrom::End(0))?;
    file.seek(SeekFrom::Start(len.saturating_sub(4096)))?;
    let mut tail = String::new();
    file.read_to_string(&mut tail).ok();
    let last_us = tail.lines().rev().find_map(host_us);
    let duration_s = match (first_us, last_us) {
        (Some(a), Some(b)) if b > a => Some((b - a) as f64 / 1e6),
        _ => None,
    };
    Ok(FileInfo { channels, boards, duration_s })
}

/// `host_us` of a level line ("board,channel,host_us,…"); None for anything else.
fn host_us(line: &str) -> Option<i64> {
    let f: Vec<&str> = line.split(',').collect();
    if f.len() < 6 {
        return None;
    }
    f[2].parse().ok()
}

/// "0=R1@5658 1=R3@5732" (after the colon) → "R1@5658", "R3@5732" (what
/// follows the "="; the first recordings of `laptimer record` wrote
/// "R1=5658", that gives just the frequency).
fn parse_channels(rest: &str) -> Vec<String> {
    rest.split_whitespace()
        .map(|c| c.split_once('=').map_or(c, |(_, v)| v).to_string())
        .collect()
}

/// Reads a level file (a recording, a session's, `laptimer record`'s) and
/// runs it through the detectors. `setup`: a session's settings; None: the
/// file's own (`# detection:`, recordings), else the defaults.
pub fn load(path: &Path, setup: Option<DetectSetup>) -> Result<FileAnalysis> {
    let file = File::open(path).with_context(|| format!("{}", path.display()))?;
    let mut offset_us = 0;
    let mut channels: Vec<String> = Vec::new();
    let mut roles: Vec<BoardRole> = Vec::new();
    let mut boards: Vec<DebugBoard> = Vec::new();
    let mut file_setup: Option<DetectSetup> = None;
    let mut analysis: Option<Analysis> = None;
    for (n, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if let Some(rest) = line.strip_prefix("# channels:") {
            channels = parse_channels(rest);
            continue;
        }
        if let Some(rest) = line.strip_prefix("# roles:") {
            roles = parse_roles(rest);
            continue;
        }
        if let Some(rest) = line.strip_prefix("# board ") {
            // "0 {json}" (recordings); other programs write other things here.
            if let Some((_, j)) = rest.split_once(' ')
                && let Ok(b) = serde_json::from_str::<DebugBoard>(j)
            {
                boards.push(b);
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("# detection: ") {
            file_setup = serde_json::from_str(rest).ok();
            continue;
        }
        if line.starts_with('#') {
            // "# gain: 30; host_us + 1792677381421871 = wall clock µs"
            if let Some((_, rest)) = line.split_once("host_us + ") {
                offset_us = rest.split_whitespace().next().and_then(|x| x.parse().ok()).unwrap_or(0);
            }
            continue;
        }
        let f: Vec<i64> = line.split(',').filter_map(|x| x.parse().ok()).collect();
        if f.len() < 6 {
            continue; // the column names, or a line cut off at the end
        }
        let a = analysis.get_or_insert_with(|| {
            let mut s = setup.clone().or(file_setup.clone()).unwrap_or_else(|| DetectSetup {
                n_channels: 0,
                min_lap_us: 3_000_000,
                detection: DetectionSettings::default(),
            });
            s.n_channels = channels.len().max(1);
            Analysis::new(s, roles.clone(), offset_us, None)
        });
        let level = Level {
            host_us: f[2],
            board_us: f[3] as u64,
            channel: f[1] as u8,
            rssi: f[4] as u8,
            flags: f[5] as u8,
        };
        a.push(f[0] as usize, &[level]);
        if n % 16 == 0 {
            a.advance();
        }
    }
    let mut analysis = analysis.context("In der Datei sind keine Pegel.")?;
    analysis.flush();
    // Boards without a "# board" line (session files): just their number.
    for i in 0..analysis.singles.len() {
        if !boards.iter().any(|b| b.index == i) {
            boards.push(DebugBoard {
                index: i,
                id: format!("{}", i + 1),
                name: None,
                role: roles.get(i).copied().unwrap_or_default(),
            });
        }
    }
    boards.sort_by_key(|b| b.index);
    Ok(FileAnalysis {
        analysis,
        boards,
        channels,
    })
}

// --- recordings ---

/// A recording in progress: every board's levels, as they come.
pub struct Recording {
    pub name: String,
    out: BufWriter<File>,
    pub started_us: i64,
    pub levels: u64,
}

impl Recording {
    /// Starts `<dir>/rec_<date>_<time>.csv` with its header.
    pub fn start(
        dir: &Path,
        started_us: i64,
        offset_us: i64,
        channels: &[String],
        gain: u8,
        boards: &[DebugBoard],
        setup: &DetectSetup,
    ) -> Result<Recording> {
        fs::create_dir_all(dir).with_context(|| format!("{}", dir.display()))?;
        let name = chrono::Local::now().format("rec_%Y-%m-%d_%H-%M-%S.csv").to_string();
        let mut out = BufWriter::new(File::create(dir.join(&name))?);
        writeln!(out, "# laptimer recording")?;
        let chans: Vec<String> = channels.iter().enumerate().map(|(i, c)| format!("{i}={c}")).collect();
        writeln!(out, "# channels: {}", chans.join(" "))?;
        writeln!(out, "# gain: {gain}; host_us + {offset_us} = wall clock µs")?;
        let roles: Vec<String> = boards
            .iter()
            .map(|b| format!("{}={}", b.index, serde_json::to_value(b.role).unwrap().as_str().unwrap_or("finish")))
            .collect();
        writeln!(out, "# roles: {}", roles.join(" "))?;
        for b in boards {
            writeln!(out, "# board {} {}", b.index, serde_json::to_string(b)?)?;
        }
        writeln!(out, "# detection: {}", serde_json::to_string(setup)?)?;
        writeln!(out, "board,channel,host_us,board_us,rssi,flags")?;
        Ok(Recording {
            name,
            out,
            started_us,
            levels: 0,
        })
    }

    pub fn write(&mut self, board: usize, levels: &[Level]) -> Result<()> {
        for l in levels {
            writeln!(
                self.out,
                "{board},{},{},{},{},{}",
                l.channel, l.host_us, l.board_us, l.rssi, l.flags
            )?;
        }
        self.levels += levels.len() as u64;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.out.flush()?;
        Ok(())
    }

    pub fn finish(mut self) -> Result<()> {
        self.flush()
    }

    pub fn json(&self) -> Value {
        json!({ "name": self.name, "started_us": self.started_us, "levels": self.levels })
    }
}

// --- routes ---

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/debug/live", get(live))
        .route("/debug/record", post(start_recording))
        .route("/debug/record/stop", post(stop_recording))
        .route("/debug/recordings", get(list_recordings))
        .route("/debug/session-files", get(session_files))
        .route("/debug/recordings/{name}", get(download_recording).delete(delete_recording))
        .route("/debug/graph", get(graph))
}

type St = State<Arc<App>>;

fn bad(e: anyhow::Error) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("{e:#}") }))).into_response()
}

/// "0,2" → [0, 2]; missing: all `n`.
fn channel_list(s: Option<&str>, n: usize) -> Vec<usize> {
    match s {
        Some(s) if !s.is_empty() => s.split(',').filter_map(|x| x.trim().parse().ok()).collect(),
        _ => (0..n).collect(),
    }
}

#[derive(Deserialize)]
struct LiveQuery {
    after_us: Option<i64>,
    window_s: Option<f64>,
    channels: Option<String>,
}

async fn live(State(app): St, Query(q): Query<LiveQuery>) -> Response {
    let core = app.core.lock().unwrap();
    let now = core.now_us();
    let window = (q.window_s.unwrap_or(30.0).clamp(1.0, 60.0) * 1e6) as i64;
    let channels = channel_list(q.channels.as_deref(), core.settings.channels.len());
    let mut v = core.debug.live_json(q.after_us.unwrap_or(i64::MIN), now - window, &channels);
    v["now_us"] = json!(now);
    v["boards"] = json!(core.debug_boards());
    v["channels"] = json!(core.settings.channels.iter().map(|c| format!("{}@{}", c.name, c.mhz)).collect::<Vec<_>>());
    v["recording"] = json!(core.recording.as_ref().map(|r| r.json()));
    Json(v).into_response()
}

async fn start_recording(State(app): St) -> Response {
    match app.core.lock().unwrap().start_recording() {
        Ok(name) => Json(json!({ "name": name })).into_response(),
        Err(e) => bad(e),
    }
}

async fn stop_recording(State(app): St) -> Response {
    match app.core.lock().unwrap().stop_recording() {
        Ok(name) => Json(json!({ "name": name })).into_response(),
        Err(e) => bad(e),
    }
}

fn recordings_dir(app: &App) -> PathBuf {
    app.core.lock().unwrap().data_dir().join("recordings")
}

/// Ok if `name` is a recording's file name (nothing outside the folder).
fn check_name(name: &str) -> Result<()> {
    if !name.starts_with("rec_") || !name.ends_with(".csv") || name.contains('/') || name.contains('\\') {
        bail!("„{name}“ ist keine Aufnahme.");
    }
    Ok(())
}

/// The recordings, newest first: name, size, the recording in progress marked.
async fn list_recordings(State(app): St) -> Response {
    let dir = recordings_dir(&app);
    let current = app.core.lock().unwrap().recording.as_ref().map(|r| r.name.clone());
    let mut list: Vec<Value> = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(meta) = entry.metadata() else { continue };
            if check_name(&name).is_err() || !meta.is_file() {
                continue;
            }
            let info = file_info(&entry.path()).ok();
            list.push(json!({ "name": name, "size": meta.len(), "recording": current.as_deref() == Some(&name), "info": info }));
        }
    }
    // The names hold date and time: sorting by name sorts by time.
    list.sort_by(|a, b| b["name"].as_str().cmp(&a["name"].as_str()));
    Json(json!({ "recordings": list })).into_response()
}

/// For each session with a level file: what it holds (`FileInfo`), by id.
/// Sessions without one are missing.
async fn session_files(State(app): St) -> Response {
    let files: Vec<(i64, String)> = {
        let core = app.core.lock().unwrap();
        match core.db.sessions(u32::MAX) {
            Ok(list) => list.into_iter().filter_map(|s| Some((s.id, s.levels_file?))).collect(),
            Err(e) => return bad(e),
        }
    };
    // Reading the files: not under the core lock, not on the async threads.
    let result = tokio::task::spawn_blocking(move || {
        let mut out = serde_json::Map::new();
        for (id, path) in files {
            if let Ok(info) = file_info(Path::new(&path)) {
                out.insert(id.to_string(), json!(info));
            }
        }
        out
    })
    .await;
    match result {
        Ok(map) => Json(Value::Object(map)).into_response(),
        Err(e) => bad(e.into()),
    }
}

async fn download_recording(State(app): St, UrlPath(name): UrlPath<String>) -> Response {
    if let Err(e) = check_name(&name) {
        return bad(e);
    }
    app.core.lock().unwrap().flush_recording();
    match fs::read(recordings_dir(&app).join(&name)) {
        Ok(body) => (
            [
                (header::CONTENT_TYPE, "text/csv".to_string()),
                (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
            ],
            body,
        )
            .into_response(),
        Err(_) => bad(anyhow::anyhow!("Die Aufnahme „{name}“ gibt es nicht.")),
    }
}

async fn delete_recording(State(app): St, UrlPath(name): UrlPath<String>) -> Response {
    if let Err(e) = check_name(&name) {
        return bad(e);
    }
    if app.core.lock().unwrap().recording.as_ref().is_some_and(|r| r.name == name) {
        return bad(anyhow::anyhow!("Die Aufnahme läuft noch – erst stoppen."));
    }
    match fs::remove_file(recordings_dir(&app).join(&name)) {
        Ok(()) => {
            app.debug_files.lock().unwrap().retain(|(k, _, _)| *k != format!("rec:{name}"));
            Json(json!({})).into_response()
        }
        Err(_) => bad(anyhow::anyhow!("Die Aufnahme „{name}“ gibt es nicht.")),
    }
}

#[derive(Deserialize)]
struct GraphQuery {
    /// "rec:<name>" or "session:<id>".
    source: String,
    channel: usize,
    from_us: Option<i64>,
    to_us: Option<i64>,
    points: Option<usize>,
}

async fn graph(State(app): St, Query(q): Query<GraphQuery>) -> Response {
    let result = tokio::task::spawn_blocking(move || -> Result<Value> {
        let f = file_analysis(&app, &q.source)?;
        let Some((start, end)) = f.analysis.bounds() else {
            bail!("In der Datei sind keine Pegel.");
        };
        let (from, to) = (q.from_us.unwrap_or(start), q.to_us.unwrap_or(end));
        let mut v = f.analysis.graph_json(q.channel, from, to.max(from + 1), q.points.unwrap_or(800));
        v["start_us"] = json!(start);
        v["end_us"] = json!(end);
        v["boards"] = json!(f.boards);
        v["channels"] = json!(f.channels);
        // All passes of the channel, for the comparison table and the overview.
        v["all_passes"] = json!(f.analysis.passes(q.channel));
        Ok(v)
    })
    .await;
    match result {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => bad(e),
        Err(e) => bad(e.into()),
    }
}

/// A level file worked through: from the cache (same file size), else read.
/// Blocks for a while (seconds for a long session on the Pi).
fn file_analysis(app: &App, source: &str) -> Result<Arc<FileAnalysis>> {
    let (path, setup) = source_file(app, source)?;
    let size = fs::metadata(&path).with_context(|| format!("{}", path.display()))?.len();
    if let Some((_, _, f)) = app
        .debug_files
        .lock()
        .unwrap()
        .iter()
        .find(|(k, s, _)| k == source && *s == size)
    {
        return Ok(Arc::clone(f));
    }
    let f = Arc::new(load(&path, setup)?);
    let mut cache = app.debug_files.lock().unwrap();
    cache.retain(|(k, _, _)| k != source);
    if cache.len() >= 2 {
        cache.remove(0); // keep the memory small
    }
    cache.push((source.to_string(), size, Arc::clone(&f)));
    Ok(f)
}

/// The level file behind "rec:<name>" or "session:<id>", and for a session
/// its detection settings (also the source of a replay, `server.rs`).
pub fn source_file(app: &App, source: &str) -> Result<(PathBuf, Option<DetectSetup>)> {
    Ok(if let Some(name) = source.strip_prefix("rec:") {
        check_name(name)?;
        let mut core = app.core.lock().unwrap();
        core.flush_recording();
        (core.data_dir().join("recordings").join(name), None)
    } else if let Some(id) = source.strip_prefix("session:") {
        let id: i64 = id.parse().context("Ungültige Session")?;
        let mut core = app.core.lock().unwrap();
        let s = core.db.session(id)?.context("Diese Session gibt es nicht.")?;
        core.flush_levels(id)?;
        let path = s.levels_file.context("Für diese Session sind keine Pegel aufgezeichnet.")?;
        let setup = DetectSetup {
            n_channels: 0, // from the file
            min_lap_us: (s.config.min_lap_s * 1e6) as i64,
            detection: s.config.detection.clone().unwrap_or_default(),
        };
        (PathBuf::from(path), Some(setup))
    } else {
        bail!("Unbekannte Quelle „{source}“.");
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_from_the_header() {
        let path = std::env::temp_dir().join(format!("laptimer_info_{}.csv", std::process::id()));
        fs::write(
            &path,
            "# laptimer session 3\n# channels: 0=R1@5658 1=R3@5732\n# gain: 30; host_us + 5 = wall clock µs\n\
             # roles: 0=finish 1=arming\n# board 0 {\"index\":0,\"id\":\"AA\",\"name\":\"S1\",\"role\":\"finish\"}\n\
             board,channel,host_us,board_us,rssi,flags\n0,0,1000000,1,40,0\n1,1,3500000,2,41,0\n0,1,3",
        )
        .unwrap();
        let info = file_info(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(info.channels, vec!["R1@5658", "R3@5732"]);
        let boards: Vec<(Option<&str>, BoardRole)> = info.boards.iter().map(|b| (b.name.as_deref(), b.role)).collect();
        assert_eq!(boards, vec![(Some("S1"), BoardRole::Finish), (None, BoardRole::Arming)]);
        // The cut-off last line doesn't count.
        assert_eq!(info.duration_s, Some(2.5));
    }

    /// Two boards: board 0 sees a pass every 5 s, board 1 only noise.
    fn levels(board: usize, t0: i64, seconds: i64) -> Vec<Level> {
        let mut out = Vec::new();
        for i in 0..seconds * 500 {
            let t = t0 + i * 2_000;
            let s = (t as f64 / 1e6) % 5.0;
            let db = if board == 0 && (2.4..2.6).contains(&s) { -20.0 } else { -40.0 };
            out.push(Level {
                host_us: t,
                board_us: t as u64,
                channel: 0,
                rssi: ((db + 63.75) * 4.0) as u8,
                flags: 0,
            });
        }
        out
    }

    #[test]
    fn each_board_and_the_gate() {
        let setup = DetectSetup {
            n_channels: 1,
            min_lap_us: 3_000_000,
            detection: DetectionSettings::default(),
        };
        let mut a = Analysis::new(setup, vec![BoardRole::Finish, BoardRole::Finish], 1_000, Some(LIVE_KEEP_US));
        let (l0, l1) = (levels(0, 0, 40), levels(1, 0, 40));
        // In batches of 10 ms, as the boards send them.
        for (c0, c1) in l0.chunks(5).zip(l1.chunks(5)) {
            a.push(0, c0);
            a.push(1, c1);
            a.advance();
        }
        let count = |s: usize| a.passes.iter().filter(|p| p.series == s && !p.retracted).count();
        assert!(count(FINISH) >= 5, "finish: {}", count(FINISH));
        assert_eq!(count(FINISH), count(FIRST_BOARD));
        assert_eq!(count(FIRST_BOARD + 1), 0);
        // Wall clock times, the slots of the last minute.
        let (first, last) = a.bounds().unwrap();
        assert!(first > -100_000 && last <= 40_001_000); // less the envelope delay
        let v = a.live_json(last - 100_000, 0, &[0]);
        let s = v["series"].as_array().unwrap();
        assert_eq!(s.len(), 3); // finish, board 0, board 1 (no arming boards)
        assert!(s[0]["channels"][0]["level"].as_array().unwrap().len() <= 10);
    }
}
