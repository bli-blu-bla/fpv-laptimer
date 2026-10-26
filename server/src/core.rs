//! The server's state and what happens to it: levels come in from the
//! boards, passes go into the database, laps are computed, announcements
//! played, and every change goes out to the browsers as JSON.
//!
//! One `Core` lives behind a mutex (`server.rs`). Everything here runs while
//! that lock is held, so it must be quick: no waiting for boards or audio.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::broadcast;

use crate::db::{Db, Pilot, Preset, PresetValues, Seat, SessionConfig, SessionRow};
use crate::detect::{ChannelStatus, DetectConfig, DetectionSettings, Detector, Event as DetectEvent, Thresholds};
use crate::esp::board::{Event as BoardEvent, Level, LinkStats};
use crate::events::Events;
use crate::laps::{self, EndRule, PassRecord, PilotResult, StartMode};
use crate::sound::{Part, Sounds};
use crate::stats;

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
    /// Announcements on (all of them; the header's speaker button).
    #[serde(default = "yes")]
    pub sound: bool,
    /// Where they are played, and which ones.
    #[serde(default)]
    pub announce: AnnounceSettings,
    /// Auto or classic (fixed) thresholds.
    #[serde(default)]
    pub detection: DetectionSettings,
    /// Which gate each board watches, by its id (USB serial number). Boards
    /// not listed watch the finish gate.
    #[serde(default)]
    pub board_roles: BTreeMap<String, BoardRole>,
}

/// Where announcements are played, and which ones.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnnounceSettings {
    pub output: SoundOutput,
    /// Race: "Piloten bereit", "Rennen beginnt in", countdown, start tone;
    /// training: "Training läuft".
    pub start: bool,
    /// After every lap.
    pub lap: LapCall,
    /// Training: "Pilot <name> hat eine neue Bestzeit erzielt: 13,7"
    /// (instead of that pilot's lap announcement).
    pub best: bool,
    /// Race: "<name> ist im Ziel".
    pub finished: bool,
    /// Race: "Rennen beendet".
    pub race_over: bool,
}

impl Default for AnnounceSettings {
    fn default() -> Self {
        AnnounceSettings {
            output: SoundOutput::Server,
            start: true,
            lap: LapCall::Beep,
            best: true,
            finished: true,
            race_over: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundOutput {
    /// The server's speaker (the Pi).
    #[default]
    Server,
    /// The browsers switched on as speakers (e.g. a laptop with a
    /// Bluetooth box).
    Browser,
}

/// What is announced after a lap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LapCall {
    Off,
    /// A short tone (once, even if several pilots pass together).
    #[default]
    Beep,
    /// "<name> ist in Runde 3".
    Number,
    /// "<name> 13,7" (the lap time).
    Time,
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
    /// The pilot's preset last copied into this channel's detection
    /// (`Core::apply_presets`). The channel keeps its values without one.
    #[serde(default)]
    pub preset_id: Option<i64>,
}

impl Default for SeatSetting {
    fn default() -> Self {
        SeatSetting {
            pilot_id: None,
            label: String::new(),
            enabled: true,
            preset_id: None,
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
            announce: AnnounceSettings::default(),
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
    /// The name saved on the board (`NAME`), if any.
    pub name: Option<String>,
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
    pub fn new(port: String, id: String, info: String, name: Option<String>) -> BoardView {
        BoardView {
            port,
            id,
            info,
            name,
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
    /// Passes already sent as `pilot_pass` events.
    reported: HashSet<i64>,
    /// The next countdown second to send as an event (0: done), and whether
    /// `start_signal` went out.
    countdown_next: i64,
    started: bool,
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
    /// All session times come from this one clock (`now_us`), not from the
    /// system clock: a Pi without a battery clock boots with an old time and
    /// NTP later jumps it, sometimes by a day. With two clocks the passes
    /// would then land before the session's start and be dropped.
    origin: Instant,
    origin_unix_us: i64,
    active: Option<Active>,
    pub boards: Vec<BoardView>,
    sounds: Sounds,
    /// Announcements waiting for their time: (wall µs, clip).
    scheduled: Vec<(i64, Part)>,
    tx: broadcast::Sender<String>,
    /// To the API WebSocket and the hooks (`events.rs`).
    pub events: Events,
    data_dir: PathBuf,
    next_live_us: i64,
    next_rate_us: i64,
}

impl Core {
    pub fn new(
        db: Db,
        data_dir: PathBuf,
        origin: Instant,
        sounds: Sounds,
        tx: broadcast::Sender<String>,
        events: Events,
    ) -> Core {
        let settings: Settings = db.get_setting("settings").ok().flatten().unwrap_or_default();
        if let Ok(n) = db.close_open_sessions()
            && n > 0
        {
            eprintln!("closed {n} session(s) left open by an earlier run");
        }
        let (finish, arming) = detectors(&settings);
        sounds.set_enabled(settings.sound);
        sounds.set_browser(settings.announce.output == SoundOutput::Browser);
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
            events,
            data_dir,
            next_live_us: now,
            next_rate_us: now + 1_000_000,
        }
    }

    fn wall_us(&self, host_us: i64) -> i64 {
        self.origin_unix_us + host_us
    }

    /// Now on the session clock (see `origin`).
    pub fn now_us(&self) -> i64 {
        self.wall_us(self.origin.elapsed().as_micros() as i64)
    }

    /// Follows the system clock again if it jumped (NTP after booting).
    /// Only between sessions: during one, all its times stay on one clock.
    fn resync_clock(&mut self) {
        let host_us = self.origin.elapsed().as_micros() as i64;
        let fresh = unix_now_us() - host_us;
        let jump = fresh - self.origin_unix_us;
        if jump.abs() > 1_000_000 {
            eprintln!("system clock jumped by {:.1} s, following it", jump as f64 / 1e6);
            self.origin_unix_us = fresh;
        }
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

    pub fn session_running(&self) -> bool {
        self.active.is_some()
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
                self.events.emit("passes_changed", json!({ "session_id": session_id }));
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
        // A pass right after the start must not be reported before the
        // `start_signal` (that one otherwise waits for the next `tick`).
        self.countdown_events(self.now_us());
        let Some(a) = self.active.as_mut() else { return Ok(()) };
        a.passes = self.db.passes(a.row.id)?;
        let new = results(&a.row, &a.passes);
        report_passes(a, &new, &self.events);
        if announce {
            let ann = &self.settings.announce;
            let race = a.row.config.race;
            let best = new.iter().filter_map(|r| r.best_lap_us).min();
            // A new best lap of the session (training only, and only once
            // there was one before).
            let new_best = !race && best.is_some() && a.best_lap_us.is_some_and(|b| best.unwrap() < b);
            if best.is_some() {
                a.best_lap_us = best;
            }
            let mut beep = false;
            let mut calls: Vec<Vec<Part>> = Vec::new();
            let mut finished_now = false;
            for (i, r) in new.iter().enumerate() {
                let old_laps = a.results.get(i).map_or(0, |o| o.laps.len());
                let name = Part::text(&a.row.seats[i].label);
                let finished = r.finished_us.is_some();
                if finished && !a.announced[i] {
                    a.announced[i] = true;
                    finished_now = true;
                    if ann.finished {
                        calls.push(vec![name, Part::clip("ist_im_ziel", "ist im Ziel")]);
                        continue;
                    }
                }
                let Some(lap) = r.laps.last().filter(|_| r.laps.len() > old_laps) else {
                    continue;
                };
                let time = Part::text(&spoken_seconds(lap.duration_us));
                if ann.best && new_best && r.best_lap_us == best {
                    calls.push(vec![
                        Part::clip("pilot", "Pilot"),
                        name,
                        Part::clip("neue_bestzeit", "hat eine neue Bestzeit erzielt"),
                        time,
                    ]);
                    continue;
                }
                match ann.lap {
                    LapCall::Off => {}
                    // After n laps the pilot flies lap n + 1 (a finished
                    // pilot flies none: just the tone).
                    LapCall::Beep => beep = true,
                    LapCall::Number if finished => beep = true,
                    LapCall::Number => calls.push(vec![
                        name,
                        Part::clip("ist_in_runde", "ist in Runde"),
                        Part::number(r.laps.len() + 1),
                    ]),
                    LapCall::Time => calls.push(vec![name, time]),
                }
            }
            if beep {
                self.sounds.play("runde", "");
            }
            for parts in calls {
                self.sounds.say(parts);
            }
            if race && ann.race_over && finished_now && a.announced.iter().all(|&x| x) {
                self.sounds.play("rennen_beendet", "Rennen beendet");
            }
        }
        // Finished pilots, as events (announced or not).
        let positions = positions(a.row.config.race, &new);
        for (i, r) in new.iter().enumerate() {
            let was = a.results.get(i).is_some_and(|o| o.finished_us.is_some());
            if r.finished_us.is_some() && !was {
                let mut data = seat_json(&a.row.seats[i]);
                data["session_id"] = json!(a.row.id);
                data["laps"] = json!(r.laps.len());
                data["total_us"] = json!(r.total_us);
                data["total_s"] = json!(r.total_us.map(seconds));
                data["best_lap_us"] = json!(r.best_lap_us);
                data["best_lap_s"] = json!(r.best_lap_us.map(seconds));
                data["position"] = json!(positions[i]);
                self.events.emit("pilot_finished", data);
            }
        }
        a.results = new;
        // A race is over when everybody has finished.
        let all_done = a.row.config.race && !a.results.is_empty() && a.results.iter().all(|r| r.finished_us.is_some());
        if all_done {
            self.end_session("finished")?;
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
        if self.db.unconfirmed_race()?.is_some() {
            bail!("Erst das Ergebnis des letzten Rennens bestätigen (oder verwerfen).");
        }
        self.resync_clock();
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

        let created_us = self.now_us();
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

        // The pilot names, ready for the announcements.
        self.sounds.prepare(seats.iter().map(|s| s.label.clone()).collect());
        if self.settings.announce.start {
            self.schedule_start(race, config.countdown_s as i64, start_us);
        }

        let row = self.db.session(id)?.expect("just created");
        let n = row.seats.len();
        let n_channels = self.settings.channels.len();
        self.armed_at = vec![None; n_channels];
        self.last_counted = vec![None; n_channels];
        self.active = Some(Active {
            row,
            passes: Vec::new(),
            results: Vec::new(),
            announced: vec![false; n],
            best_lap_us: None,
            levels_out: Some(out),
            reported: HashSet::new(),
            countdown_next: config.countdown_s as i64,
            started: false,
        });
        let row = &self.active.as_ref().unwrap().row;
        let (end, end_value) = match config.end_rule {
            EndRule::Open => ("open", None),
            EndRule::Laps(n) => ("laps", Some(n)),
            EndRule::Seconds(n) => ("seconds", Some(n)),
        };
        self.events.emit(
            "session_started",
            json!({
                "session_id": id,
                "race": race,
                "start_mode": config.start_mode,
                "end": end,
                "end_value": end_value,
                "countdown_s": config.countdown_s,
                "created_us": created_us,
                "start_us": start_us,
                "pilot_count": row.seats.len(),
                "seats": row.seats.iter().map(seat_json).collect::<Vec<_>>(),
            }),
        );
        self.refresh(false)?;
        Ok(id)
    }

    /// The start announcements. Race: "Piloten bereit" (countdown of 8 s
    /// or more), "Rennen beginnt in" 5 s before, the last seconds counted
    /// (up to 5), the start tone. Training: "Training läuft".
    fn schedule_start(&mut self, race: bool, countdown_s: i64, start_us: i64) {
        if !race {
            self.sounds.play("training_beginnt", "Training läuft");
            return;
        }
        let at = |s_before: i64| start_us - s_before * 1_000_000;
        if countdown_s >= 8 {
            self.sounds.play("piloten_bereit", "Piloten, macht euch bereit");
        }
        // "Rennen beginnt in" takes about 1.5 s: then the count from 3.
        let count_from = if countdown_s >= 5 {
            self.scheduled
                .push((at(5), Part::clip("rennen_beginnt", "Rennen beginnt in")));
            3
        } else {
            countdown_s
        };
        for n in (1..=count_from).rev() {
            self.scheduled.push((at(n), Part::number(n as usize)));
        }
        self.scheduled.push((start_us, Part::tone("start")));
    }

    /// A sample announcement (settings page): the best-lap one, with the
    /// first slot's pilot.
    pub fn announce_test(&self) {
        let pilots = self.db.pilots().unwrap_or_default();
        let name = self
            .settings
            .seats
            .iter()
            .find_map(|s| {
                let label = s.label.trim();
                if !label.is_empty() {
                    return Some(label.to_string());
                }
                s.pilot_id
                    .and_then(|id| pilots.iter().find(|p| p.id == id))
                    .map(|p| p.name.clone())
            })
            .unwrap_or_else(|| "Nitro".into());
        self.sounds.say(vec![
            Part::clip("pilot", "Pilot"),
            Part::text(&name),
            Part::clip("neue_bestzeit", "hat eine neue Bestzeit erzielt"),
            Part::text(&spoken_seconds(13_700_000)),
        ]);
    }

    pub fn stop_session(&mut self) -> Result<()> {
        self.end_session("stopped")
    }

    /// Ends the active session. `reason` (for the event): "stopped" (by
    /// hand), "finished" (everybody in the race finished), "cancelled"
    /// (stopped to be deleted).
    pub fn end_session(&mut self, reason: &str) -> Result<()> {
        let Some(mut a) = self.active.take() else { return Ok(()) };
        if let Some(mut out) = a.levels_out.take() {
            out.flush()?;
        }
        let ended_us = self.now_us();
        self.db.end_session(a.row.id, ended_us)?;
        self.scheduled.clear();
        let mut data = result_json(&a.row, &a.results);
        data["reason"] = json!(reason);
        data["ended_us"] = json!(ended_us);
        data["needs_confirmation"] = json!(a.row.config.race && reason != "cancelled");
        self.events.emit("session_stopped", data);
        self.broadcast_state();
        Ok(())
    }

    /// Deletes an ended session with its passes and levels.
    pub fn delete_session(&mut self, id: i64) -> Result<()> {
        let s = self.db.session(id)?.context("Diese Session gibt es nicht.")?;
        if s.ended_us.is_none() {
            bail!("Die Session läuft noch.");
        }
        if let Some(f) = &s.levels_file {
            let _ = fs::remove_file(f);
        }
        self.db.delete_session(id)?;
        let _ = self.tx.send(json!({ "type": "session_changed", "id": id }).to_string());
        self.events
            .emit("session_deleted", json!({ "session_id": id, "race": s.config.race }));
        self.broadcast_state(); // an unconfirmed race may be gone
        Ok(())
    }

    /// Cuts an ended session `TRIM_AFTER_US` after its last pass (finish
    /// gate, not deleted): the levels after it, passes there and the end.
    /// For sessions that ran on after the flying (pilot landed, Stop
    /// forgotten). Rewrites the level file under the core lock, so no one
    /// reads it half done. Returns (cut wall µs, levels removed).
    pub fn trim_session(&mut self, id: i64) -> Result<(i64, usize)> {
        let s = self.db.session(id)?.context("Diese Session gibt es nicht.")?;
        if s.ended_us.is_none() {
            bail!("Die Session läuft noch.");
        }
        let path = s
            .levels_file
            .context("Für diese Session sind keine Pegel aufgezeichnet.")?;
        let last = self
            .db
            .passes(id)?
            .iter()
            .filter(|p| p.gate == 0 && !p.deleted)
            .map(|p| p.time_us)
            .max()
            .context("Die Session hat keinen Durchflug – nichts zum Kürzen.")?;
        let cut_us = last + TRIM_AFTER_US;
        let removed = crate::trace::trim_file(Path::new(&path), cut_us)?;
        if removed == 0 {
            bail!("Die Pegel enden schon spätestens 10 s nach dem letzten Durchflug – nichts zum Kürzen.");
        }
        self.db.trim_session(id, cut_us)?;
        self.session_changed(id)?;
        Ok((cut_us, removed))
    }

    /// Deletes every session with its passes and level files (pilots,
    /// settings, keys and hooks stay). Refused while a session runs.
    /// Returns how many were deleted.
    pub fn reset_sessions(&mut self) -> Result<usize> {
        if self.active.is_some() {
            bail!("Es läuft eine Session – erst stoppen.");
        }
        let files: Vec<String> = self
            .db
            .sessions(u32::MAX)?
            .into_iter()
            .filter_map(|s| s.levels_file)
            .collect();
        let n = self.db.delete_all_sessions()?;
        for f in files {
            let _ = fs::remove_file(f);
        }
        let _ = self.tx.send(json!({ "type": "session_changed" }).to_string());
        self.events.emit("sessions_reset", json!({ "deleted": n }));
        self.broadcast_state();
        Ok(n)
    }

    /// The folder of the database and the level files.
    pub fn data_dir(&self) -> &std::path::Path {
        &self.data_dir
    }

    /// The id of the session in progress.
    pub fn active_id(&self) -> Option<i64> {
        self.active.as_ref().map(|a| a.row.id)
    }

    /// Confirms an ended race's result; only then can the next session start.
    pub fn confirm_session(&mut self, id: i64) -> Result<()> {
        let s = self.db.session(id)?.context("Diese Session gibt es nicht.")?;
        if s.ended_us.is_none() {
            bail!("Die Session läuft noch.");
        }
        if s.confirmed_us.is_none() {
            let now = self.now_us();
            self.db.confirm_session(id, now)?;
            let passes = self.db.passes(id)?;
            let mut data = result_json(&s, &results(&s, &passes));
            data["confirmed_us"] = json!(now);
            self.events.emit("race_confirmed", data);
        }
        let _ = self.tx.send(json!({ "type": "session_changed", "id": id }).to_string());
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
        self.events.emit("passes_changed", json!({ "session_id": session_id }));
        if self.active.as_ref().is_some_and(|a| a.row.id == session_id) {
            self.refresh(false)?;
        } else {
            let _ = self
                .tx
                .send(json!({ "type": "session_changed", "id": session_id }).to_string());
        }
        Ok(())
    }

    /// A pass added by hand that counts like a detected one (API: a button
    /// at the gate): in the running session announced and reported as
    /// `pilot_pass`; otherwise a correction.
    pub fn pass_added(&mut self, session_id: i64) -> Result<()> {
        if self.active_id() == Some(session_id) {
            self.refresh(true)
        } else {
            self.session_changed(session_id)
        }
    }

    /// Saves the settings. Channels and gain only take effect when the
    /// boards are restarted (`server.rs` does that).
    pub fn set_settings(&mut self, mut s: Settings) -> Result<()> {
        if self.active.is_some() && s.channels != self.settings.channels {
            bail!("Kanäle lassen sich erst nach dem Stoppen der Session ändern.");
        }
        self.apply_presets(&mut s)?;
        self.db.set_setting("settings", &s)?;
        let channels_changed = s.channels != self.settings.channels || s.seats != self.settings.seats;
        self.sounds.set_enabled(s.sound);
        self.sounds.set_browser(s.announce.output == SoundOutput::Browser);
        let redetect = s.min_lap_s != self.settings.min_lap_s
            || s.detection != self.settings.detection
            || s.board_roles != self.settings.board_roles;
        self.settings = s;
        if redetect {
            self.apply_roles();
            (self.finish, self.arming) = detectors(&self.settings);
        }
        if channels_changed {
            self.events
                .emit("channels_changed", json!({ "channels": self.channel_list() }));
        }
        self.broadcast_state();
        Ok(())
    }

    /// The channels with who flies on them (the API's view of the settings).
    pub fn channel_list(&self) -> Vec<Value> {
        let pilots = self.db.pilots().unwrap_or_default();
        self.settings
            .channels
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let seat = self.settings.seats.get(i).cloned().unwrap_or_default();
                let pilot = seat.pilot_id.and_then(|id| pilots.iter().find(|p| p.id == id));
                let shown = if !seat.label.trim().is_empty() {
                    seat.label.trim().to_string()
                } else {
                    pilot.map_or(c.name.clone(), |p| p.name.clone())
                };
                json!({
                    "index": i,
                    "name": c.name,
                    "mhz": c.mhz,
                    "enabled": seat.enabled,
                    "pilot_id": seat.pilot_id,
                    "pilot": pilot.map(|p| &p.name),
                    "label": seat.label,
                    "shown": shown,
                })
            })
            .collect()
    }

    /// Presets in new settings `s`, per seat: a seat whose pilot changed gets
    /// the pilot's preset if they have exactly one (none otherwise, also for
    /// free text); a seat whose preset changed gets its values copied into
    /// the finish gate's detection of that channel. Without a preset the
    /// channel keeps what it has (guests, free text).
    fn apply_presets(&self, s: &mut Settings) -> Result<()> {
        let presets = self.db.presets()?;
        for i in 0..s.seats.len() {
            let old = self.settings.seats.get(i).cloned().unwrap_or_default();
            let seat = &mut s.seats[i];
            // Who flies: free text means no pilot (and no preset).
            let flying = |x: &SeatSetting| if x.label.trim().is_empty() { x.pilot_id } else { None };
            let pilot = flying(seat);
            let old_pilot = flying(&old);
            let theirs: Vec<&Preset> = presets.iter().filter(|p| Some(p.pilot_id) == pilot).collect();
            if pilot != old_pilot {
                // A swap may bring the preset along (the page does that).
                if !seat.preset_id.is_some_and(|id| theirs.iter().any(|p| p.id == id)) {
                    seat.preset_id = match theirs.as_slice() {
                        [only] => Some(only.id),
                        _ => None,
                    };
                }
            } else if !seat.preset_id.is_some_and(|id| theirs.iter().any(|p| p.id == id)) {
                seat.preset_id = None; // deleted, or not this pilot's
            }
            // Unchanged: the channel's values may have been tuned since.
            if seat.preset_id == old.preset_id {
                continue;
            }
            let Some(p) = seat.preset_id.and_then(|id| presets.iter().find(|p| p.id == id)) else {
                continue;
            };
            let list = &mut s.detection.finish;
            while list.len() <= i {
                list.push(Thresholds::default());
            }
            list[i] = Thresholds {
                enter_db: p.values.enter_db,
                exit_db: p.values.exit_db,
                mode: Some(p.values.mode),
                min_rise_db: Some(p.values.min_rise_db),
            };
        }
        Ok(())
    }

    /// The finish gate's detection of channel `slot` as preset values (its
    /// mode and minimum swing resolved: what it uses right now).
    fn preset_values(&self, slot: usize) -> Result<PresetValues> {
        if slot >= self.settings.channels.len() {
            bail!("Diesen Slot gibt es nicht.");
        }
        let d = &self.settings.detection;
        let t = d.finish.get(slot).copied().unwrap_or_default();
        Ok(PresetValues {
            mode: t.mode.unwrap_or(d.finish_mode),
            enter_db: t.enter_db,
            exit_db: t.exit_db,
            min_rise_db: t.min_rise_db.unwrap_or(d.min_rise_db),
            gain: self.settings.gain,
        })
    }

    // --- presets ---

    /// A new preset for `pilot_id` from slot `slot`'s current detection; the
    /// slot then uses it.
    pub fn add_preset(&mut self, pilot_id: i64, name: &str, slot: usize) -> Result<i64> {
        let name = name.trim();
        if name.is_empty() {
            bail!("Das Preset braucht einen Namen.");
        }
        self.pilot(pilot_id)?;
        let values = self.preset_values(slot)?;
        let id = self.db.add_preset(pilot_id, name, &values)?;
        self.set_seat_preset(slot, pilot_id, id)?; // broadcasts
        Ok(id)
    }

    /// Renames a preset, and with `slot` overwrites its values with that
    /// slot's current detection.
    pub fn update_preset(&mut self, id: i64, name: Option<&str>, slot: Option<usize>) -> Result<()> {
        let Some(p) = self.db.presets()?.into_iter().find(|p| p.id == id) else {
            bail!("Dieses Preset gibt es nicht.");
        };
        let name = name.map_or(p.name.clone(), |n| n.trim().to_string());
        if name.is_empty() {
            bail!("Das Preset braucht einen Namen.");
        }
        let values = match slot {
            Some(slot) => self.preset_values(slot)?,
            None => p.values,
        };
        self.db.update_preset(id, &name, &values)?;
        match slot {
            Some(slot) => self.set_seat_preset(slot, p.pilot_id, id)?,
            None => self.broadcast_state(),
        }
        Ok(())
    }

    /// Seats using it keep their values.
    pub fn delete_preset(&mut self, id: i64) -> Result<()> {
        self.db.delete_preset(id)?;
        if self.settings.seats.iter().any(|s| s.preset_id == Some(id)) {
            let s = self.settings.clone();
            self.set_settings(s)?; // apply_presets clears it; broadcasts
        } else {
            self.broadcast_state();
        }
        Ok(())
    }

    /// Slot `slot` uses preset `id` (just saved from it, so its values are
    /// already the channel's), if `pilot_id` flies there.
    fn set_seat_preset(&mut self, slot: usize, pilot_id: i64, id: i64) -> Result<()> {
        let mut s = self.settings.clone();
        match s.seats.get_mut(slot) {
            Some(seat) if seat.pilot_id == Some(pilot_id) && seat.label.trim().is_empty() => {
                seat.preset_id = Some(id);
                // Saved directly: the values are the channel's already.
                self.db.set_setting("settings", &s)?;
                self.settings = s;
            }
            _ => {}
        }
        self.broadcast_state();
        Ok(())
    }

    // --- pilots (here, so every change is an event) ---

    pub fn add_pilot(&mut self, name: &str, full_name: &str) -> Result<i64> {
        let id = self.db.add_pilot(name, full_name)?;
        self.events.emit(
            "pilot_added",
            json!({ "pilot_id": id, "name": name, "full_name": full_name }),
        );
        self.broadcast_state();
        Ok(id)
    }

    pub fn update_pilot(&mut self, id: i64, name: &str, full_name: &str) -> Result<()> {
        if !self.db.pilots()?.iter().any(|p| p.id == id) {
            bail!("Diesen Piloten gibt es nicht.");
        }
        self.db.update_pilot(id, name, full_name)?;
        self.events.emit(
            "pilot_updated",
            json!({ "pilot_id": id, "name": name, "full_name": full_name }),
        );
        self.broadcast_state();
        Ok(())
    }

    /// Also takes the pilot out of their slot.
    pub fn delete_pilot(&mut self, id: i64) -> Result<()> {
        let Some(p) = self.db.pilots()?.into_iter().find(|p| p.id == id) else {
            bail!("Diesen Piloten gibt es nicht.");
        };
        self.db.delete_pilot(id)?;
        self.events
            .emit("pilot_deleted", json!({ "pilot_id": id, "name": p.name }));
        if self.settings.seats.iter().any(|s| s.pilot_id == Some(id)) {
            let mut s = self.settings.clone();
            for seat in &mut s.seats {
                if seat.pilot_id == Some(id) {
                    seat.pilot_id = None;
                }
            }
            self.set_settings(s)?; // broadcasts
        } else {
            self.broadcast_state();
        }
        Ok(())
    }

    pub fn pilot(&self, id: i64) -> Result<Pilot> {
        self.db
            .pilots()?
            .into_iter()
            .find(|p| p.id == id)
            .context("Diesen Piloten gibt es nicht.")
    }

    // --- output ---

    /// Called often (every ~10 ms): due announcements, live levels.
    pub fn tick(&mut self) {
        let now = self.now_us();
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
        for part in due {
            self.sounds.say(vec![part]);
        }
        self.countdown_events(now);
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

    /// `countdown` (every second before the start signal) and
    /// `start_signal` events of the active session.
    fn countdown_events(&mut self, now: i64) {
        let Some(a) = self.active.as_mut() else { return };
        if a.started {
            return;
        }
        let start_us = a.row.start_us;
        while a.countdown_next > 0 && now >= start_us - a.countdown_next * 1_000_000 {
            self.events.emit(
                "countdown",
                json!({ "session_id": a.row.id, "race": a.row.config.race, "seconds_left": a.countdown_next }),
            );
            a.countdown_next -= 1;
        }
        if now >= start_us {
            a.started = true;
            self.events.emit(
                "start_signal",
                json!({ "session_id": a.row.id, "race": a.row.config.race, "start_us": start_us }),
            );
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
            "now_us": self.now_us(),
            "version": crate::VERSION,
            "settings": self.settings,
            "pilots": pilots,
            "presets": self.db.presets().unwrap_or_default(),
            "boards": self.boards,
            "active": active,
            // The ended race still waiting for its result to be confirmed.
            "unconfirmed": self.db.unconfirmed_race().ok().flatten(),
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

/// Places of the seats: a race by laps, then total time; a training by the
/// best lap. None: no lap yet.
pub fn positions(race: bool, results: &[PilotResult]) -> Vec<Option<usize>> {
    let order: Vec<usize> = if race {
        stats::race_order(results)
    } else {
        let mut o: Vec<usize> = (0..results.len())
            .filter(|&i| results[i].best_lap_us.is_some())
            .collect();
        o.sort_by_key(|&i| results[i].best_lap_us);
        o
    };
    let mut pos = vec![None; results.len()];
    for (place, &i) in order.iter().enumerate() {
        pos[i] = Some(place + 1);
    }
    pos
}

/// µs → seconds, rounded to ms (13.712).
pub fn seconds(us: i64) -> f64 {
    (us as f64 / 1000.0).round() / 1000.0
}

/// Who flies on a seat, for events and the API.
pub fn seat_json(s: &Seat) -> Value {
    json!({
        "channel": s.channel,
        "channel_name": s.channel_name,
        "mhz": s.mhz,
        "pilot_id": s.pilot_id,
        "pilot": s.label,
    })
}

/// Every seat's result in a simple form (API and events), in seat order.
pub fn standings(row: &SessionRow, results: &[PilotResult]) -> Vec<Value> {
    let pos = positions(row.config.race, results);
    row.seats
        .iter()
        .zip(results)
        .enumerate()
        .map(|(i, (seat, r))| {
            let mut v = seat_json(seat);
            let laps: Vec<i64> = r.laps.iter().map(|l| l.duration_us).collect();
            v["position"] = json!(pos[i]);
            v["laps"] = json!(laps.len());
            v["lap_times_us"] = json!(laps);
            v["last_lap_us"] = json!(laps.last());
            v["best_lap_us"] = json!(r.best_lap_us);
            v["best_consecutive_us"] = json!(r.best_consecutive.map(|(sum, _)| sum));
            v["total_us"] = json!(r.total_us);
            v["holeshot_us"] = json!(r.holeshot_us);
            v["finished"] = json!(r.finished_us.is_some());
            v
        })
        .collect()
}

/// A session's result for the `session_stopped` and `race_confirmed`
/// events: the standings and, for a race, the winner.
pub fn result_json(row: &SessionRow, results: &[PilotResult]) -> Value {
    let standings = standings(row, results);
    let winner = standings
        .iter()
        .find(|s| s["position"] == json!(1))
        .map(|s| s["pilot"].clone());
    json!({
        "session_id": row.id,
        "race": row.config.race,
        "winner": if row.config.race { winner } else { None },
        "results": standings,
    })
}

/// New passes of the active session (not deleted, not reported yet) as
/// `pilot_pass` events: which lap they ended, or that they were the
/// holeshot, the first pass of a flying start or didn't count.
fn report_passes(a: &mut Active, new: &[PilotResult], events: &Events) {
    let best = new.iter().filter_map(|r| r.best_lap_us).min();
    let pos = positions(a.row.config.race, new);
    for p in &a.passes {
        if p.gate != 0 || p.deleted || !a.reported.insert(p.id) {
            continue; // `insert` is false: reported before
        }
        let Some(i) = a.row.seats.iter().position(|s| s.channel == p.channel) else {
            continue;
        };
        let r = &new[i];
        let lap = r.laps.iter().find(|l| l.pass_id == p.id);
        let kind = if lap.is_some() {
            "lap"
        } else if r.ignored.contains(&p.id) {
            "ignored"
        } else if a.row.config.start_mode == StartMode::Holeshot {
            "holeshot"
        } else {
            "start"
        };
        let mut data = seat_json(&a.row.seats[i]);
        let obj = data.as_object_mut().unwrap();
        let lap_us = lap.map(|l| l.duration_us);
        for (k, v) in [
            ("session_id", json!(a.row.id)),
            ("race", json!(a.row.config.race)),
            ("pass_id", json!(p.id)),
            ("time_us", json!(p.time_us)),
            ("manual", json!(p.manual)),
            ("kind", json!(kind)),
            ("lap", json!(lap.map(|l| l.number))),
            ("lap_time_us", json!(lap_us)),
            ("lap_time_s", json!(lap_us.map(seconds))),
            ("laps", json!(r.laps.len())),
            ("best_lap_us", json!(r.best_lap_us)),
            ("best_lap_s", json!(r.best_lap_us.map(seconds))),
            ("personal_best", json!(lap_us.is_some() && lap_us == r.best_lap_us)),
            ("session_best", json!(lap_us.is_some() && lap_us == best)),
            ("holeshot_us", json!(r.holeshot_us)),
            ("finished", json!(r.finished_us == Some(p.time_us))),
            ("position", json!(pos[i])),
        ] {
            obj.insert(k.to_string(), v);
        }
        events.emit("pilot_pass", data);
    }
}

/// Fresh detectors for the finish and the arming gate.
fn detectors(s: &Settings) -> (Detector, Detector) {
    let n = s.channels.len();
    let mut finish = Detector::new(n, detect_config(s));
    finish.set_fixed(&s.detection.fixed(false, n));
    finish.set_min_rise(&s.detection.min_rise(false, n));
    let mut arming = Detector::new(n, detect_config(s));
    arming.set_fixed(&s.detection.fixed(true, n));
    arming.set_min_rise(&s.detection.min_rise(true, n));
    (finish, arming)
}

fn detect_config(s: &Settings) -> DetectConfig {
    DetectConfig {
        min_lap_us: (s.min_lap_s * 1e6) as i64,
        min_rise_db: s.detection.min_rise_db,
    }
}

/// A lap time as it is spoken: "13,7" (seconds, German decimal comma).
fn spoken_seconds(us: i64) -> String {
    format!("{:.1}", us as f64 / 1e6).replace('.', ",")
}

/// `trim_session` keeps this much after the last pass.
const TRIM_AFTER_US: i64 = 10_000_000;

pub fn unix_now_us() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::DetectMode;

    /// A core with an in-memory database and the sound cache in a temp folder.
    fn core() -> Core {
        let dir = std::env::temp_dir().join(format!("laptimer-core-test-{}", std::process::id()));
        let (tx, _) = broadcast::channel(16);
        let (hooks, _) = std::sync::mpsc::channel();
        Core::new(
            Db::open(":memory:").unwrap(),
            dir.clone(),
            Instant::now(),
            Sounds::new(dir.join("clips"), dir.join("tts"), tx.clone()),
            tx.clone(),
            Events::new(tx, hooks),
        )
    }

    fn seat(pilot_id: Option<i64>, preset_id: Option<i64>) -> SeatSetting {
        SeatSetting {
            pilot_id,
            preset_id,
            ..Default::default()
        }
    }

    #[test]
    fn presets_follow_the_pilot() {
        let mut c = core();
        let anna = c.add_pilot("Anna", "").unwrap();
        let ben = c.add_pilot("Ben", "").unwrap();
        // Slot 0 manual at -20/-26 with 10 dB: saved as Anna's preset.
        let mut s = c.settings.clone();
        s.seats[0] = seat(Some(anna), None);
        s.detection.finish = vec![Thresholds {
            enter_db: -20.0,
            exit_db: -26.0,
            mode: Some(DetectMode::Classic),
            min_rise_db: Some(10.0),
        }];
        c.set_settings(s).unwrap();
        let whoop = c.add_preset(anna, "Whoop", 0).unwrap();
        assert_eq!(c.settings.seats[0].preset_id, Some(whoop));

        // Anna moves to slot 2 (swap with nobody): her only preset comes along.
        let mut s = c.settings.clone();
        s.seats[0] = seat(Some(ben), None);
        s.seats[2] = seat(Some(anna), None);
        c.set_settings(s).unwrap();
        assert_eq!(c.settings.seats[2].preset_id, Some(whoop));
        assert_eq!(c.settings.detection.finish[2].enter_db, -20.0);
        assert_eq!(c.settings.detection.finish[2].mode, Some(DetectMode::Classic));
        assert_eq!(c.settings.detection.min_rise(false, 3)[2], 10.0);
        // Ben has none: slot 0 keeps its values.
        assert_eq!(c.settings.seats[0].preset_id, None);
        assert_eq!(c.settings.detection.finish[0].enter_db, -20.0);

        // Tuned by hand afterwards: stays, the preset is not applied again.
        let mut s = c.settings.clone();
        s.detection.finish[2].enter_db = -18.0;
        c.set_settings(s).unwrap();
        assert_eq!(c.settings.detection.finish[2].enter_db, -18.0);

        // Two presets: none is picked on its own, one can be chosen.
        let race = c.add_preset(anna, "5 Zoll", 2).unwrap();
        assert!(c.add_preset(anna, "5 Zoll", 2).is_err()); // names are unique per pilot
        let mut s = c.settings.clone();
        s.seats[2] = seat(None, None);
        s.seats[3] = seat(Some(anna), None);
        c.set_settings(s).unwrap();
        assert_eq!(c.settings.seats[3].preset_id, None);
        let mut s = c.settings.clone();
        s.seats[3].preset_id = Some(race);
        c.set_settings(s).unwrap();
        assert_eq!(c.settings.detection.finish[3].enter_db, -18.0);

        // Free text: no preset. A deleted preset leaves the values.
        let mut s = c.settings.clone();
        s.seats[3].label = "Gast".into();
        c.set_settings(s).unwrap();
        assert_eq!(c.settings.seats[3].preset_id, None);
        let mut s = c.settings.clone();
        s.seats[1] = seat(Some(anna), Some(whoop));
        c.set_settings(s).unwrap();
        c.delete_preset(whoop).unwrap();
        assert_eq!(c.settings.seats[1].preset_id, None);
        assert_eq!(c.settings.detection.finish[1].enter_db, -20.0);
    }
}
