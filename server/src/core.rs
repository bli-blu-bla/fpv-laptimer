//! The server's state and what happens to it: levels come in from the
//! boards, passes go into the database, laps are computed, announcements
//! played, and every change goes out to the browsers as JSON.
//!
//! One `Core` lives behind a mutex (`server.rs`). Everything here runs while
//! that lock is held, so it must be quick: no waiting for boards or audio.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast;

use crate::db::{Db, Seat, SessionConfig, SessionRow};
use crate::detect::{ChannelStatus, DetectConfig, DetectionSettings, Detector, Event as DetectEvent};
use crate::esp::board::{Event as BoardEvent, Level, LinkStats};
use crate::laps::{self, EndRule, PassRecord, PilotResult, StartMode};
use crate::sound::Sounds;

/// Live levels go out this often, µs.
const LIVE_EVERY_US: i64 = 100_000;

/// The settings, kept in the database (key "settings") and edited in the
/// browser.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub channels: Vec<ChannelSetting>,
    /// Who flies on each channel (same order as `channels`).
    pub seats: Vec<SeatSetting>,
    /// RX gain of the boards.
    pub gain: u8,
    pub race: RaceSettings,
    pub min_lap_s: f64,
    pub consecutive: u32,
    /// Announcements on.
    #[serde(default = "yes")]
    pub sound: bool,
    /// Auto or classic (fixed) thresholds.
    #[serde(default)]
    pub detection: DetectionSettings,
    /// Which gate each board watches, by its id (USB serial number). Boards
    /// not listed watch the finish gate.
    #[serde(default)]
    pub board_roles: BTreeMap<String, BoardRole>,
}

/// Which gate a board watches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardRole {
    /// The gate that counts laps.
    #[default]
    Finish,
    /// A pass at the finish only counts after one here (since the last
    /// counted pass).
    Arming,
    /// Not used.
    Off,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelSetting {
    pub name: String,
    pub mhz: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeatSetting {
    pub pilot_id: Option<i64>,
    /// Free text; empty: the pilot's callsign, or else the channel name.
    pub label: String,
    /// Off: nobody flies on this channel; left out of sessions.
    #[serde(default = "yes")]
    pub enabled: bool,
}

impl Default for SeatSetting {
    fn default() -> Self {
        SeatSetting {
            pilot_id: None,
            label: String::new(),
            enabled: true,
        }
    }
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RaceSettings {
    pub start_mode: StartMode,
    pub end_rule: EndRule,
    pub countdown_s: u32,
}

impl Default for Settings {
    fn default() -> Self {
        let channels = [("R1", 5658), ("R3", 5732), ("R6", 5843), ("R8", 5917)]
            .into_iter()
            .map(|(name, mhz)| ChannelSetting { name: name.into(), mhz })
            .collect::<Vec<_>>();
        Settings {
            seats: vec![SeatSetting::default(); channels.len()],
            channels,
            gain: 30,
            race: RaceSettings {
                start_mode: StartMode::Holeshot,
                end_rule: EndRule::Laps(3),
                countdown_s: 5,
            },
            min_lap_s: 3.0,
            consecutive: 3,
            sound: true,
            detection: DetectionSettings::default(),
            board_roles: BTreeMap::new(),
        }
    }
}

/// What the browser shows per board.
#[derive(Debug, Clone, Default, Serialize)]
pub struct BoardView {
    pub port: String,
    /// USB serial number (MAC), stable across plugging.
    pub id: String,
    pub info: String,
    pub levels_per_s: f64,
    pub stats: Option<LinkStats>,
    pub failed: Option<String>,
    pub role: BoardRole,
    /// The board's current level (highest envelope of its channels), dBFS:
    /// to tell the boards apart when assigning their roles.
    pub level_db: Option<f32>,
    #[serde(skip)]
    count: u64,
}

impl BoardView {
    pub fn new(port: String, id: String, info: String) -> BoardView {
        BoardView {
            port,
            id,
            info,
            ..Default::default()
        }
    }
}

/// The session in progress.
struct Active {
    row: SessionRow,
    passes: Vec<PassRecord>,
    results: Vec<PilotResult>,
    /// Channels already announced as finished.
    announced: Vec<bool>,
    best_lap_us: Option<i64>,
    levels_out: Option<BufWriter<File>>,
}

pub struct Core {
    pub db: Db,
    pub settings: Settings,
    /// The detection of the finish gate (laps) and of the arming gate.
    finish: Detector,
    arming: Detector,
    /// Per channel: the last arming-gate pass, and the last pass that counted
    /// at the finish (wall µs), for the arming rule.
    armed_at: Vec<Option<i64>>,
    last_counted: Vec<Option<i64>>,
    /// Host time (µs since `origin`) → wall clock: origin_unix_us + host_us.
    origin: Instant,
    origin_unix_us: i64,
    active: Option<Active>,
    pub boards: Vec<BoardView>,
    sounds: Sounds,
    /// Announcements waiting for their time: (wall µs, sound).
    scheduled: Vec<(i64, String)>,
    tx: broadcast::Sender<String>,
    data_dir: PathBuf,
    next_live_us: i64,
    next_rate_us: i64,
}

impl Core {
    pub fn new(db: Db, data_dir: PathBuf, origin: Instant, sounds: Sounds, tx: broadcast::Sender<String>) -> Core {
        let settings: Settings = db.get_setting("settings").ok().flatten().unwrap_or_default();
        if let Ok(n) = db.close_open_sessions()
            && n > 0
        {
            eprintln!("closed {n} session(s) left open by an earlier run");
        }
        let (finish, arming) = detectors(&settings);
        sounds.set_enabled(settings.sound);
        let now = unix_now_us();
        Core {
            db,
            finish,
            arming,
            armed_at: Vec::new(),
            last_counted: Vec::new(),
            settings,
            origin,
            origin_unix_us: now - origin.elapsed().as_micros() as i64,
            active: None,
            boards: Vec::new(),
            sounds,
            scheduled: Vec::new(),
            tx,
            data_dir,
            next_live_us: now,
            next_rate_us: now + 1_000_000,
        }
    }

    fn wall_us(&self, host_us: i64) -> i64 {
        self.origin_unix_us + host_us
    }

    /// The boards are (re)started: new board list, fresh detectors.
    pub fn boards_started(&mut self, boards: Vec<BoardView>) {
        self.boards = boards;
        self.apply_roles();
        (self.finish, self.arming) = detectors(&self.settings);
        self.broadcast_state();
    }

    /// Each board's role from the settings.
    fn apply_roles(&mut self) {
        for b in &mut self.boards {
            b.role = self.settings.board_roles.get(&b.id).copied().unwrap_or_default();
        }
    }

    /// True when a working board watches the arming gate: then the arming
    /// rule applies.
    fn has_arming(&self) -> bool {
        self.boards
            .iter()
            .any(|b| b.role == BoardRole::Arming && b.failed.is_none())
    }

    pub fn origin(&self) -> Instant {
        self.origin
    }

    // --- input from the boards ---

    pub fn on_board_event(&mut self, ev: BoardEvent) -> Result<()> {
        match ev {
            BoardEvent::Levels { board, levels } => {
                if let Some(b) = self.boards.get_mut(board) {
                    b.count += levels.len() as u64;
                }
                self.write_levels(board, &levels)?;
                match self.boards.get(board).map(|b| b.role).unwrap_or_default() {
                    BoardRole::Finish => {
                        self.finish.push(board, &levels);
                        for e in self.finish.advance() {
                            self.on_detect(e)?;
                        }
                    }
                    BoardRole::Arming => {
                        self.arming.push(board, &levels);
                        for e in self.arming.advance() {
                            self.on_arming(e)?;
                        }
                    }
                    BoardRole::Off => {}
                }
            }
            BoardEvent::Stats { board, stats } => {
                if let Some(b) = self.boards.get_mut(board) {
                    b.stats = Some(stats);
                }
            }
            BoardEvent::Failed { board, error } => {
                if let Some(b) = self.boards.get_mut(board) {
                    b.failed = Some(error);
                }
                self.broadcast_state();
            }
        }
        Ok(())
    }

    fn write_levels(&mut self, board: usize, levels: &[Level]) -> Result<()> {
        let Some(out) = self.active.as_mut().and_then(|a| a.levels_out.as_mut()) else {
            return Ok(());
        };
        for l in levels {
            writeln!(
                out,
                "{board},{},{},{},{},{}",
                l.channel, l.host_us, l.board_us, l.rssi, l.flags
            )?;
        }
        Ok(())
    }

    fn on_detect(&mut self, e: DetectEvent) -> Result<()> {
        let Some(session_id) = self.active.as_ref().map(|a| a.row.id) else {
            return Ok(());
        };
        match e {
            DetectEvent::Pass(p) => {
                let time_us = self.wall_us(p.time_us);
                let row = &self.active.as_ref().unwrap().row;
                if time_us < row.created_us {
                    return Ok(()); // from before Start was pressed
                }
                let t0 = if row.config.race { row.start_us } else { row.created_us };
                // The arming rule: after the first counted pass, a pass only
                // counts if the arming gate saw the pilot since the last one.
                let c = p.channel as usize;
                let last = self.last_counted.get(c).copied().flatten();
                let armed = self.armed_at.get(c).copied().flatten();
                let counts = !self.has_arming() || last.is_none() || armed.is_some_and(|a| Some(a) > last);
                let id = self.db.add_pass(session_id, p.channel, time_us, p.peak_db, false)?;
                if !counts {
                    // Kept as deleted: the pass editor can restore it.
                    self.db.set_pass_deleted(id, true)?;
                } else if time_us >= t0
                    && let Some(l) = self.last_counted.get_mut(c)
                {
                    *l = Some(time_us);
                }
                self.refresh(counts)?;
            }
            DetectEvent::Retract(p) => {
                let time_us = self.wall_us(p.time_us);
                self.db.remove_auto_pass(session_id, p.channel, time_us)?;
                self.refresh(false)?;
            }
        }
        Ok(())
    }

    /// A pass at the arming gate: remembered for the arming rule, and stored
    /// (the pass editor shows it).
    fn on_arming(&mut self, e: DetectEvent) -> Result<()> {
        let DetectEvent::Pass(p) = e else {
            if let (DetectEvent::Retract(p), Some(a)) = (e, &self.active) {
                let id = a.row.id;
                self.db.remove_auto_pass(id, p.channel, self.wall_us(p.time_us))?;
            }
            return Ok(());
        };
        let time_us = self.wall_us(p.time_us);
        if let Some(a) = self.armed_at.get_mut(p.channel as usize) {
            *a = Some(time_us);
        }
        if let Some(a) = &self.active
            && time_us >= a.row.created_us
        {
            let id = a.row.id;
            self.db.add_arming_pass(id, p.channel, time_us, p.peak_db)?;
            self.refresh(false)?;
        }
        Ok(())
    }

    /// Reloads the active session's passes, recomputes the laps and tells
    /// the browsers. `announce`: play the sounds for what changed.
    fn refresh(&mut self, announce: bool) -> Result<()> {
        let Some(a) = self.active.as_mut() else { return Ok(()) };
        a.passes = self.db.passes(a.row.id)?;
        let new = results(&a.row, &a.passes);
        if announce {
            let mut sounds = Vec::new();
            let mut lap_done = false;
            for (i, r) in new.iter().enumerate() {
                let old_laps = a.results.get(i).map_or(0, |o| o.laps.len());
                lap_done |= r.laps.len() > old_laps;
                if r.finished_us.is_some() && !a.announced[i] {
                    a.announced[i] = true;
                    // "Pilot N" is the slot (channel position), as on the page.
                    sounds.push(format!("pilot{}_fertig", a.row.seats[i].channel + 1));
                }
            }
            let best = new.iter().filter_map(|r| r.best_lap_us).min();
            let new_best = !a.row.config.race && best.is_some() && a.best_lap_us.is_some_and(|b| best.unwrap() < b);
            if best.is_some() {
                a.best_lap_us = best;
            }
            if a.row.config.race && !a.announced.is_empty() && a.announced.iter().all(|&x| x) && !sounds.is_empty() {
                sounds.push("rennen_beendet".into());
            }
            if sounds.is_empty() && lap_done {
                sounds.push(if new_best { "bestzeit" } else { "runde" }.into());
            }
            for s in sounds {
                self.sounds.play(&s);
            }
        }
        a.results = new;
        // A race is over when everybody has finished.
        let all_done = a.row.config.race && !a.results.is_empty() && a.results.iter().all(|r| r.finished_us.is_some());
        if all_done {
            self.stop_session()?;
        } else {
            self.broadcast_state();
        }
        Ok(())
    }

    // --- sessions ---

    pub fn start_session(&mut self, race: bool) -> Result<i64> {
        if self.active.is_some() {
            bail!("Es läuft schon eine Session – erst stoppen.");
        }
        let s = &self.settings;
        let config = if race {
            SessionConfig {
                race: true,
                start_mode: s.race.start_mode,
                end_rule: s.race.end_rule,
                countdown_s: s.race.countdown_s,
                min_lap_s: s.min_lap_s,
                consecutive: s.consecutive,
                detection: Some(s.detection.clone()),
            }
        } else {
            SessionConfig {
                race: false,
                start_mode: StartMode::Flying,
                end_rule: EndRule::Open,
                countdown_s: 0,
                min_lap_s: s.min_lap_s,
                consecutive: s.consecutive,
                detection: Some(s.detection.clone()),
            }
        };
        let pilots = self.db.pilots()?;
        let seats: Vec<Seat> = s
            .channels
            .iter()
            .enumerate()
            .filter(|(i, _)| s.seats.get(*i).is_none_or(|seat| seat.enabled))
            .map(|(i, ch)| {
                let seat = s.seats.get(i).cloned().unwrap_or_default();
                let pilot_name = seat
                    .pilot_id
                    .and_then(|id| pilots.iter().find(|p| p.id == id))
                    .map(|p| p.name.clone());
                let label = if !seat.label.trim().is_empty() {
                    seat.label.trim().to_string()
                } else {
                    pilot_name.unwrap_or_else(|| ch.name.clone())
                };
                Seat {
                    channel: i as u8,
                    channel_name: ch.name.clone(),
                    mhz: ch.mhz,
                    pilot_id: seat.pilot_id,
                    label,
                }
            })
            .collect();

        let created_us = unix_now_us();
        let start_us = created_us + config.countdown_s as i64 * 1_000_000;
        let levels_dir = self.data_dir.join("levels");
        fs::create_dir_all(&levels_dir)?;
        let id = self.db.create_session(&config, created_us, start_us, &seats, None)?;
        let levels_path = levels_dir.join(format!("session_{id}.csv"));
        let mut out = BufWriter::new(File::create(&levels_path)?);
        writeln!(out, "# laptimer session {id}")?;
        let chans: Vec<String> = s
            .channels
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{i}={}@{}", c.name, c.mhz))
            .collect();
        writeln!(out, "# channels: {}", chans.join(" "))?;
        writeln!(
            out,
            "# gain: {}; host_us + {} = wall clock µs",
            s.gain, self.origin_unix_us
        )?;
        let roles: Vec<String> = self
            .boards
            .iter()
            .enumerate()
            .map(|(i, b)| {
                format!(
                    "{i}={}",
                    serde_json::to_value(b.role).unwrap().as_str().unwrap_or("finish")
                )
            })
            .collect();
        writeln!(out, "# roles: {}", roles.join(" "))?;
        writeln!(out, "board,channel,host_us,board_us,rssi,flags")?;
        // Remember the file (the row was made before we knew the id).
        self.db.set_levels_file(id, &levels_path.to_string_lossy())?;

        if race && config.countdown_s > 0 {
            let at = |s_before: i64| start_us - s_before * 1_000_000;
            if config.countdown_s >= 5 {
                self.scheduled.push((at(5), "rennen_beginnt".into()));
            }
            for n in (1..=3).filter(|&n| n <= config.countdown_s as i64) {
                self.scheduled.push((at(n), n.to_string()));
            }
            self.scheduled.push((start_us, "start".into()));
        } else if race {
            self.sounds.play("start");
        }

        let row = self.db.session(id)?.expect("just created");
        let n = row.seats.len();
        self.armed_at = vec![None; s.channels.len()];
        self.last_counted = vec![None; s.channels.len()];
        self.active = Some(Active {
            row,
            passes: Vec::new(),
            results: Vec::new(),
            announced: vec![false; n],
            best_lap_us: None,
            levels_out: Some(out),
        });
        self.refresh(false)?;
        Ok(id)
    }

    pub fn stop_session(&mut self) -> Result<()> {
        let Some(mut a) = self.active.take() else { return Ok(()) };
        if let Some(mut out) = a.levels_out.take() {
            out.flush()?;
        }
        self.db.end_session(a.row.id, unix_now_us())?;
        self.scheduled.clear();
        self.broadcast_state();
        Ok(())
    }

    /// Writes out the active session's buffered levels (before its file is read).
    pub fn flush_levels(&mut self, session_id: i64) -> Result<()> {
        if let Some(a) = self.active.as_mut()
            && a.row.id == session_id
            && let Some(out) = a.levels_out.as_mut()
        {
            out.flush()?;
        }
        Ok(())
    }

    /// Something changed a stored session (a correction): recompute.
    pub fn session_changed(&mut self, session_id: i64) -> Result<()> {
        if self.active.as_ref().is_some_and(|a| a.row.id == session_id) {
            self.refresh(false)?;
        } else {
            let _ = self
                .tx
                .send(json!({ "type": "session_changed", "id": session_id }).to_string());
        }
        Ok(())
    }

    /// Saves the settings. Channels and gain only take effect when the
    /// boards are restarted (`server.rs` does that).
    pub fn set_settings(&mut self, s: Settings) -> Result<()> {
        if self.active.is_some() && s.channels != self.settings.channels {
            bail!("Kanäle lassen sich erst nach dem Stoppen der Session ändern.");
        }
        self.db.set_setting("settings", &s)?;
        self.sounds.set_enabled(s.sound);
        let redetect = s.min_lap_s != self.settings.min_lap_s
            || s.detection != self.settings.detection
            || s.board_roles != self.settings.board_roles;
        self.settings = s;
        if redetect {
            self.apply_roles();
            (self.finish, self.arming) = detectors(&self.settings);
        }
        self.broadcast_state();
        Ok(())
    }

    // --- output ---

    /// Called often (every ~10 ms): due announcements, live levels.
    pub fn tick(&mut self) {
        let now = unix_now_us();
        // `retain` keeps what is not due yet; the due ones are played.
        let mut due = Vec::new();
        self.scheduled.retain(|(t, s)| {
            if *t <= now {
                due.push(s.clone());
                false
            } else {
                true
            }
        });
        for s in due {
            self.sounds.play(&s);
        }
        if now >= self.next_rate_us {
            for b in &mut self.boards {
                b.levels_per_s = b.count as f64;
                b.count = 0;
            }
            self.next_rate_us = now + 1_000_000;
        }
        if now >= self.next_live_us {
            self.next_live_us = now + LIVE_EVERY_US;
            for (i, b) in self.boards.iter_mut().enumerate() {
                b.level_db = match b.role {
                    BoardRole::Finish => self.finish.board_level(i),
                    BoardRole::Arming => self.arming.board_level(i),
                    BoardRole::Off => None,
                };
            }
            let n = self.settings.channels.len();
            let arming: Option<Vec<ChannelStatus>> = self
                .has_arming()
                .then(|| (0..n).map(|c| self.arming.status(c)).collect());
            let channels: Vec<ChannelStatus> = (0..self.settings.channels.len())
                .map(|c| self.finish.status(c))
                .collect();
            let msg =
                json!({ "type": "live", "now_us": now, "channels": channels, "arming": arming, "boards": self.boards });
            let _ = self.tx.send(msg.to_string());
        }
    }

    /// Everything the browser needs to draw the page.
    pub fn state_json(&self) -> serde_json::Value {
        let pilots = self.db.pilots().unwrap_or_default();
        let active = self.active.as_ref().map(|a| {
            json!({
                "session": a.row,
                "passes": a.passes,
                "results": a.results,
            })
        });
        json!({
            "type": "state",
            "now_us": unix_now_us(),
            "settings": self.settings,
            "pilots": pilots,
            "boards": self.boards,
            "active": active,
        })
    }

    pub fn broadcast_state(&self) {
        let _ = self.tx.send(self.state_json().to_string());
    }
}

/// Laps of every seat of a session.
pub fn results(row: &SessionRow, passes: &[PassRecord]) -> Vec<PilotResult> {
    let c = &row.config;
    // Training counts from Start; a race from its start signal.
    let start = if c.race { row.start_us } else { row.created_us };
    row.seats
        .iter()
        .map(|s| laps::compute(s.channel, passes, c.start_mode, c.end_rule, start, c.consecutive))
        .collect()
}

/// Fresh detectors for the finish and the arming gate.
fn detectors(s: &Settings) -> (Detector, Detector) {
    let n = s.channels.len();
    let mut finish = Detector::new(n, detect_config(s));
    finish.set_fixed(&s.detection.fixed(false, n));
    let mut arming = Detector::new(n, detect_config(s));
    arming.set_fixed(&s.detection.fixed(true, n));
    (finish, arming)
}

fn detect_config(s: &Settings) -> DetectConfig {
    DetectConfig {
        min_lap_us: (s.min_lap_s * 1e6) as i64,
    }
}

pub fn unix_now_us() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}
