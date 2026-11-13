//! Pass detection: from the boards' levels to "pilot on channel X passed the
//! gate at time T". No I/O here, so it runs the same live, on a recording
//! (`laptimer replay`) and in the tests. `tools/detect_passes.py` is the same
//! idea in Python, for trying things out on recordings.
//!
//! Steps, per channel:
//! 1. **Slots:** time is cut into 10 ms slots; per board, the highest level
//!    in each slot. (DJI sends in packets: about every 5th capture falls into
//!    a pause and reads only noise. The maximum ignores those.)
//! 2. **Envelope:** per board, the maximum over 2 slots (20 ms), then the
//!    median over the last 10 of those (100 ms) against short spikes.
//! 3. **Boards combined:** the highest envelope of all boards. Where one
//!    board sees the quad badly for a moment, another usually doesn't.
//! 4. **Thresholds from the flight itself:** "course" is the median envelope
//!    while the transmitter is on (last 3 minutes), "peak" the median of the
//!    pilot's last pass peaks (before the 3rd pass: a high percentile). A pass
//!    starts above course + 60 % of (peak − course) and ends below 20 %.
//! 5. **Pass time:** the middle of the part within 1 dB of the pass maximum.
//! 6. **Merging:** a pass is reported 1 s after it ended, so that a second
//!    crossing right after it (a dip in the middle of the gate) can still be
//!    merged into it. Crossings closer than the minimum lap to the previous
//!    pass are one pass.
//!    6b. **Replacing:** a crossing at most 2 s after the reported pass and
//!    at least 6 dB stronger replaces it (`Event::Retract`, then the new
//!    pass): the reported one was a hump before the gate (in all 16 such
//!    cases in sessions 14-23 the arming pass lay between the two). Only at
//!    the finish gate (`set_replace`).
//! 7. **Start-up:** until a pilot has 3 passes, the peak is only estimated, and
//!    the course's highest moments can count as a pass. Once 3 passes give a
//!    real peak, the early passes that would not have reached the threshold
//!    are taken back (`Event::Retract`).
//! 8. **Late first thresholds:** a channel only gets learned thresholds after
//!    5 s of levels above -46 dBFS. A pilot who is quiet on the course can
//!    pass the gate before that (session 32, 2026-10-31: R8's holeshot at
//!    31.0 s, thresholds at 33.2 s). So until then each channel keeps its last
//!    10 s, and when its first thresholds come, goes through them once: a pass
//!    found there is reported late, with its real time.
//!
//! **Experimental mode `AutoRelative`** ("Automatisch (experimentell)"): step
//! 3 differs, everything else is the same. A board that hears the whole
//! course loudly has a high course level and humps that rise little above it;
//! with the plain maximum it sets the course for the gate, and the clean
//! humps of a quieter board stay below the threshold (session 57,
//! 2026-11-13). So each board is measured against its own course level:
//! - per board and channel the course is learned as in step 4 (median of the
//!   last 3 minutes above -46 dBFS, after 5 s of them);
//! - the board with the highest course is the reference; every other board
//!   is shifted up by (reference course - its own course), so that all
//!   course levels lie on top of each other;
//! - the maximum of these shifted envelopes goes on to steps 4-8, unchanged.
//!
//! So the level, the thresholds and the pass peaks of such a channel stay in
//! dBFS, "as the reference board would have seen it": rise above the own
//! course + the reference board's course. One board alone gives exactly the
//! plain automatic mode. Boards that have not learned their course yet, or
//! that are below -46 dBFS right now (transmitter off for them), are left
//! out; while no board has a course, the plain maximum is used (nothing is
//! detected then anyway, and step 8 can look back through it). For the graph
//! of one board alone, `TracePoint::shifts` / `Detector::board_shifts` say
//! how far it was shifted: its thresholds are the channel's minus that.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::esp::board::Level;
use crate::esp::frame::rssi_dbfs;

/// Length of a slot, µs.
pub const SLOT_US: i64 = 10_000;
/// Envelope: maximum over this many slots, then the median over MEDIAN_SLOTS.
const MAX_SLOTS: usize = 2;
const MEDIAN_SLOTS: usize = 10;
/// The envelope computed at the end of slot k describes the middle of its
/// windows, this much before the start of slot k.
const ENVELOPE_DELAY_US: i64 = 45_000;
/// Level statistics cover this many slots (3 minutes).
const STATS_SLOTS: usize = 18_000;
/// "Transmitter on": the envelope above this. The ADC noise alone gives
/// -57 to -54 dBFS (envelope, gains 30-45).
const ON_DB: f32 = -46.0;
/// No passes before the transmitter has been on this long (slots).
const WARMUP_SLOTS: u32 = 500;
const ENTER_FRACTION: f32 = 0.6;
const EXIT_FRACTION: f32 = 0.2;
/// Pass peaks the threshold is taken from (their median).
const PEAK_HISTORY: usize = 5;
/// Before this many passes, the peak is a percentile of the levels instead.
const MIN_PEAKS: usize = 3;
const TOP_CODES: u8 = 4; // 1 dB
/// No thresholds while the peak is less than this above the course (codes):
/// the levels only wander, nobody has flown through the gate yet.
const MIN_CONTRAST: f32 = 3.0 * 4.0;
/// A pass must rise at least this far above the course level, whatever the
/// learned peaks say (default of `DetectionSettings::min_rise_db`). With 3 dB, wiggles 4-5 dB above the course
/// counted (session 1 of 2026-10-22: 5 false passes before the first real
/// one); real passes rose 13-26 dB in every recording so far. In session 4
/// (2026-10-23, Pi) the pilot mostly flew 1-2 m above the gate: those flybys
/// rose 14 dB over the course and counted at 35 % of the peak (8 passes), but
/// not with 12 dB.
pub const MIN_RISE_DB: f32 = 12.0;
/// A pass is reported this long after its crossing ended.
const HOLD_US: i64 = 1_000_000;
/// A pass this much stronger, at most this long after the reported one,
/// replaces it (step 6b): the reported one was a hump before the gate.
const REPLACE_WITHIN_US: i64 = 2_000_000;
const REPLACE_STRONGER_DB: f32 = 6.0;
/// Slots a channel keeps until its first learned thresholds (step 8), 10 s.
const LOOKBACK_SLOTS: usize = 1000;
/// A board that sent nothing for this long no longer holds the others back.
const BOARD_TIMEOUT_US: i64 = 500_000;

/// How the thresholds come about (the user's choice, in the settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectMode {
    /// Learned from the flight (steps 4 and 7 above).
    #[default]
    Auto,
    /// Fixed per channel, as RotorHazard's EnterAt/ExitAt.
    Classic,
    /// Learned as `Auto`, but each board measured against its own course
    /// level (experimental, see the head of this file).
    AutoRelative,
}

impl DetectMode {
    /// Every mode, for lists and error texts. A new mode: here, in
    /// `Detector::set_detection` (what the detector does with it) and in
    /// `frontend/src/model.js::DETECT_MODES` (its name on the page).
    pub const ALL: [DetectMode; 3] = [DetectMode::Auto, DetectMode::AutoRelative, DetectMode::Classic];

    /// The name in the JSON and in query parameters ("auto_relative").
    pub fn name(self) -> &'static str {
        match self {
            DetectMode::Auto => "auto",
            DetectMode::Classic => "classic",
            DetectMode::AutoRelative => "auto_relative",
        }
    }

    /// From its name; None for anything else.
    pub fn from_name(name: &str) -> Option<DetectMode> {
        DetectMode::ALL.into_iter().find(|m| m.name() == name)
    }
}

/// One channel's detection: its mode (None: the gate's), its fixed
/// thresholds (dBFS, used in classic mode) and its minimum swing (automatic
/// mode; None: `DetectionSettings::min_rise_db`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Thresholds {
    pub enter_db: f32,
    pub exit_db: f32,
    #[serde(default)]
    pub mode: Option<DetectMode>,
    #[serde(default)]
    pub min_rise_db: Option<f32>,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            enter_db: -25.0,
            exit_db: -30.0,
            mode: None,
            min_rise_db: None,
        }
    }
}

/// One channel's settings, resolved (`DetectionSettings::channel`): what the
/// detector uses for it. Also what "compute again" in the editor changes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ChannelParams {
    pub mode: DetectMode,
    /// Fixed thresholds, dBFS (only used in classic mode).
    pub enter_db: f32,
    pub exit_db: f32,
    /// Minimum swing, dB (only used in the automatic modes).
    pub min_rise_db: f32,
}

/// The detection settings of the finish and the arming gate: per gate a
/// mode for its channels, and per channel its own mode (if set) and its
/// fixed thresholds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectionSettings {
    /// `alias`: settings saved when there was one mode for both gates.
    #[serde(default, alias = "mode")]
    pub finish_mode: DetectMode,
    #[serde(default)]
    pub arming_mode: DetectMode,
    #[serde(default)]
    pub finish: Vec<Thresholds>,
    #[serde(default)]
    pub arming: Vec<Thresholds>,
    /// Minimum swing (dB above the course) for a pass in automatic mode:
    /// keeps flybys above the gate from counting. A stronger transmitter or
    /// another quad may need another value (a channel's own `min_rise_db`).
    #[serde(default = "default_min_rise_db")]
    pub min_rise_db: f32,
}

fn default_min_rise_db() -> f32 {
    MIN_RISE_DB
}

impl Default for DetectionSettings {
    fn default() -> Self {
        DetectionSettings {
            finish_mode: DetectMode::default(),
            arming_mode: DetectMode::default(),
            finish: Vec::new(),
            arming: Vec::new(),
            min_rise_db: MIN_RISE_DB,
        }
    }
}

impl DetectionSettings {
    /// Per channel its mode: its own, else the gate's.
    pub fn modes(&self, arming: bool, n_channels: usize) -> Vec<DetectMode> {
        let (gate_mode, list) = if arming {
            (self.arming_mode, &self.arming)
        } else {
            (self.finish_mode, &self.finish)
        };
        (0..n_channels)
            .map(|c| list.get(c).and_then(|t| t.mode).unwrap_or(gate_mode))
            .collect()
    }

    /// For `Detector::set_fixed`: per channel, (enter, exit) if the channel
    /// is in classic mode (its own mode, else the gate's), None if automatic.
    pub fn fixed(&self, arming: bool, n_channels: usize) -> Vec<Option<(f32, f32)>> {
        let list = if arming { &self.arming } else { &self.finish };
        self.modes(arming, n_channels)
            .into_iter()
            .enumerate()
            .map(|(c, mode)| {
                let t = list.get(c).copied().unwrap_or_default();
                (mode == DetectMode::Classic).then_some((t.enter_db, t.exit_db))
            })
            .collect()
    }

    /// One channel's settings with nothing left open: its mode and minimum
    /// swing (its own, else the gate's / the common one), its thresholds.
    pub fn channel(&self, arming: bool, c: usize) -> ChannelParams {
        let list = if arming { &self.arming } else { &self.finish };
        let t = list.get(c).copied().unwrap_or_default();
        ChannelParams {
            mode: t.mode.unwrap_or(if arming { self.arming_mode } else { self.finish_mode }),
            enter_db: t.enter_db,
            exit_db: t.exit_db,
            min_rise_db: t.min_rise_db.unwrap_or(self.min_rise_db),
        }
    }

    /// Gives one channel exactly these settings (as its own; the other
    /// channels and the gate's mode stay).
    pub fn set_channel(&mut self, arming: bool, c: usize, p: ChannelParams) {
        let list = if arming { &mut self.arming } else { &mut self.finish };
        while list.len() <= c {
            list.push(Thresholds::default());
        }
        list[c] = Thresholds {
            enter_db: p.enter_db,
            exit_db: p.exit_db,
            mode: Some(p.mode),
            min_rise_db: Some(p.min_rise_db),
        };
    }

    /// For `Detector::set_min_rise`: per channel, its own minimum swing or
    /// else the common one.
    pub fn min_rise(&self, arming: bool, n_channels: usize) -> Vec<f32> {
        let list = if arming { &self.arming } else { &self.finish };
        (0..n_channels)
            .map(|c| list.get(c).and_then(|t| t.min_rise_db).unwrap_or(self.min_rise_db))
            .collect()
    }
}

/// Settings the user may change.
#[derive(Debug, Clone, Copy)]
pub struct DetectConfig {
    /// Crossings closer than this to the previous pass count as the same pass.
    pub min_lap_us: i64,
    /// A pass must rise at least this many dB above the course level
    /// (channels with their own value: `Detector::set_min_rise`).
    pub min_rise_db: f32,
}

impl Default for DetectConfig {
    fn default() -> Self {
        DetectConfig {
            min_lap_us: 3_000_000,
            min_rise_db: MIN_RISE_DB,
        }
    }
}

/// A detected pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pass {
    pub channel: u8,
    /// Host time, µs.
    pub time_us: i64,
    /// Highest envelope during the pass, dBFS.
    pub peak_db: f32,
}

/// What the detector reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    Pass(Pass),
    /// An earlier `Pass` was a mistake (see step 7 above).
    Retract(Pass),
}

/// What the display shows per channel.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct ChannelStatus {
    /// Latest combined envelope, dBFS (`AutoRelative`: of the shifted
    /// boards, see the head of this file; the thresholds fit it).
    pub level_db: Option<f32>,
    /// None while warming up.
    pub enter_db: Option<f32>,
    pub exit_db: Option<f32>,
    pub in_pass: bool,
}

/// The detector for all channels and boards.
pub struct Detector {
    config: DetectConfig,
    n_channels: usize,
    /// Per board: the latest host time seen, and per channel its slots.
    boards: Vec<BoardState>,
    channels: Vec<ChannelDetector>,
    /// The next slot to finish; None before the first level.
    next_slot: Option<i64>,
    /// With `with_trace`: what each slot looked like, per channel (for the
    /// level graph of a session).
    trace: Option<Vec<Vec<TracePoint>>>,
}

/// Boards kept in a trace point.
pub const TRACE_BOARDS: usize = 4;

/// One slot of one channel, as the detector saw it. Levels are codes
/// (0.25 dB, see `rssi_dbfs`); 0 means "none".
#[derive(Debug, Clone, Copy, Default)]
pub struct TracePoint {
    /// Host time, µs (envelope delay taken out, as pass times).
    pub time_us: i64,
    /// The boards combined.
    pub level: u8,
    pub boards: [u8; TRACE_BOARDS],
    pub enter: u8,
    pub exit: u8,
    /// `AutoRelative`: how far each board was shifted up (codes, 0.25 dB)
    /// before the maximum; `SHIFT_NONE`: the board was left out (no course
    /// learned yet). Other modes: 0. A board's own thresholds are `enter`
    /// and `exit` minus its shift.
    pub shifts: [u8; TRACE_BOARDS],
}

/// `TracePoint::shifts`: this board did not count in this slot.
pub const SHIFT_NONE: u8 = 255;

struct BoardState {
    latest_us: i64,
    channels: Vec<BoardChannel>,
}

/// One board's view of one channel.
#[derive(Default)]
struct BoardChannel {
    /// Highest level per slot not finished yet, oldest first.
    pending: VecDeque<(i64, u8)>,
    /// The slot maxima of the last MAX_SLOTS slots.
    recent_max: VecDeque<Option<u8>>,
    /// The last MEDIAN_SLOTS 20-ms maxima.
    recent_env: VecDeque<u8>,
    /// The latest envelope (for showing each board's level).
    last: Option<u8>,
    /// The latest slot gave an envelope (`last` is from this slot).
    fresh: bool,
    /// This board's own levels of the last 3 minutes, for its course
    /// (`AutoRelative`). Always kept, so that switching a channel to that
    /// mode during a session works at once. `Option` only because
    /// `Default` can't make a `LevelStats`; filled on the first level.
    stats: Option<LevelStats>,
    /// This board's course (code), once learned; refreshed every second.
    course: Option<u8>,
    /// `AutoRelative`: the shift used in the latest slot (codes); None:
    /// left out.
    shift: Option<u8>,
}

impl Detector {
    pub fn new(n_channels: usize, config: DetectConfig) -> Detector {
        Detector {
            config,
            n_channels,
            boards: Vec::new(),
            channels: (0..n_channels).map(|c| ChannelDetector::new(c as u8, config.min_rise_db)).collect(),
            next_slot: None,
            trace: None,
        }
    }

    /// Fixed thresholds ("classic" mode, as RotorHazard's EnterAt/ExitAt),
    /// dBFS, per channel; None (or no entry): learned from the flight.
    pub fn set_fixed(&mut self, fixed: &[Option<(f32, f32)>]) {
        for (c, ch) in self.channels.iter_mut().enumerate() {
            ch.fixed = fixed
                .get(c)
                .copied()
                .flatten()
                .map(|(e, x)| (db_code_f(e), db_code_f(x)));
            ch.update_thresholds();
        }
    }

    /// Minimum swing per channel, dB (instead of `DetectConfig::min_rise_db`).
    pub fn set_min_rise(&mut self, min_rise_db: &[f32]) {
        for (ch, &m) in self.channels.iter_mut().zip(min_rise_db) {
            ch.min_rise_db = m;
            ch.update_thresholds();
        }
    }

    /// New settings for the finish gate (`arming` false) or the arming gate,
    /// without starting over: what was learned, a pass in progress and the
    /// last pass (for the minimum lap) stay. So the race director can move
    /// a channel's thresholds during a session.
    pub fn set_detection(&mut self, d: &DetectionSettings, arming: bool) {
        self.set_fixed(&d.fixed(arming, self.n_channels));
        self.set_min_rise(&d.min_rise(arming, self.n_channels));
        let relative: Vec<bool> = d
            .modes(arming, self.n_channels)
            .into_iter()
            .map(|m| m == DetectMode::AutoRelative)
            .collect();
        self.set_relative(&relative);
        // Step 6b only at the finish gate: there the time is the lap. At the
        // arming gate an earlier pass arms just as well (1 s slack), and a
        // replacement there only changes what is learned (session 23: an
        // arming pass then no longer reached the threshold).
        self.set_replace(!arming);
    }

    /// Step 6b (replacing a reported pass by a much stronger one right
    /// after it) on or off; on by default.
    pub fn set_replace(&mut self, on: bool) {
        for ch in &mut self.channels {
            ch.replace = on;
        }
    }

    /// Per channel: measure each board against its own course level
    /// (`DetectMode::AutoRelative`). No entry: no.
    pub fn set_relative(&mut self, relative: &[bool]) {
        for (c, ch) in self.channels.iter_mut().enumerate() {
            ch.relative = relative.get(c).copied().unwrap_or(false);
        }
    }

    /// `AutoRelative`: how far each board is shifted up on this channel
    /// right now, dB, by board index. None: not in that mode, or the board
    /// does not count (no data here, no course learned yet). A board's own
    /// thresholds are the channel's (`status`) minus this.
    pub fn board_shifts(&self, channel: usize) -> Vec<Option<f32>> {
        let relative = self.channels.get(channel).is_some_and(|c| c.relative);
        self.boards
            .iter()
            .map(|b| {
                let shift = b.channels.get(channel).and_then(|c| c.shift)?;
                relative.then_some(shift as f32 / 4.0)
            })
            .collect()
    }

    /// One board's current level: the highest envelope of its channels, dBFS.
    /// Each board's latest envelope on one channel, dBFS, by board index
    /// (None: the board has no data there, e.g. it is on another gate).
    pub fn board_levels(&self, channel: usize) -> Vec<Option<f32>> {
        self.boards
            .iter()
            .map(|b| b.channels.get(channel).and_then(|c| c.last).map(rssi_dbfs))
            .collect()
    }

    pub fn board_level(&self, board: usize) -> Option<f32> {
        let b = self.boards.get(board)?;
        b.channels.iter().filter_map(|c| c.last).max().map(rssi_dbfs)
    }

    /// A detector that also records every slot (`take_trace`).
    pub fn with_trace(n_channels: usize, config: DetectConfig) -> Detector {
        let mut d = Detector::new(n_channels, config);
        d.trace = Some(vec![Vec::new(); n_channels]);
        d
    }

    /// The recorded slots, per channel (empty without `with_trace`).
    pub fn take_trace(&mut self) -> Vec<Vec<TracePoint>> {
        self.trace.take().unwrap_or_default()
    }

    /// The slots recorded since the last call, per channel; keeps recording
    /// (the live board debugging takes them every few ms).
    pub fn drain_trace(&mut self) -> Vec<Vec<TracePoint>> {
        match &mut self.trace {
            // `mem::take` leaves an empty Vec in place of each channel's.
            Some(t) => t.iter_mut().map(std::mem::take).collect(),
            None => Vec::new(),
        }
    }

    /// Adds one board's levels (in the order the board sent them).
    pub fn push(&mut self, board: usize, levels: &[Level]) {
        while self.boards.len() <= board {
            self.boards.push(BoardState {
                latest_us: i64::MIN,
                channels: (0..self.n_channels).map(|_| BoardChannel::default()).collect(),
            });
        }
        let b = &mut self.boards[board];
        for l in levels {
            let Some(ch) = b.channels.get_mut(l.channel as usize) else {
                continue;
            };
            let slot = l.host_us.div_euclid(SLOT_US);
            match ch.pending.back_mut() {
                Some((s, max)) if *s == slot => *max = (*max).max(l.rssi),
                _ => ch.pending.push_back((slot, l.rssi)),
            }
            b.latest_us = b.latest_us.max(l.host_us);
            if self.next_slot.is_none() {
                self.next_slot = Some(slot);
            }
        }
    }

    /// Finishes every slot all active boards have sent data past, and
    /// returns the passes that became final.
    pub fn advance(&mut self) -> Vec<Event> {
        let newest = self.boards.iter().map(|b| b.latest_us).max().unwrap_or(i64::MIN);
        // A board that went quiet (unplugged, failed) must not stop the others.
        let watermark = self
            .boards
            .iter()
            .map(|b| b.latest_us)
            // A board without any data yet (i64::MIN) waits for nobody; the
            // subtraction is saturating so that it doesn't overflow.
            .filter(|&t| t != i64::MIN && newest.saturating_sub(t) < BOARD_TIMEOUT_US)
            .min()
            .unwrap_or(i64::MIN);
        self.finish_slots_until(watermark)
    }

    /// At the end of a recording: finish everything.
    pub fn flush(&mut self) -> Vec<Event> {
        let newest = self.boards.iter().map(|b| b.latest_us).max().unwrap_or(i64::MIN);
        let mut events = self.finish_slots_until(newest + SLOT_US);
        // Let the last pass's hold time run out.
        let end = newest + HOLD_US + SLOT_US;
        for ch in &mut self.channels {
            ch.finish_pending(end, true, &mut events);
        }
        events
    }

    pub fn status(&self, channel: usize) -> ChannelStatus {
        self.channels.get(channel).map(|c| c.status()).unwrap_or_default()
    }

    fn finish_slots_until(&mut self, until_us: i64) -> Vec<Event> {
        let mut events = Vec::new();
        let Some(mut slot) = self.next_slot else { return events };
        while (slot + 1) * SLOT_US <= until_us {
            // Each board's course: refreshed once a second, as the thresholds.
            let refresh = slot % 100 == 0;
            for c in 0..self.n_channels {
                // The combined envelope: the highest of the boards'.
                let mut combined: Option<u8> = None;
                let mut per_board = [0u8; TRACE_BOARDS];
                for (i, b) in self.boards.iter_mut().enumerate() {
                    let bc = &mut b.channels[c];
                    let e = bc.finish_slot(slot);
                    if refresh {
                        bc.course = bc.stats.as_ref().and_then(|s| s.course());
                    }
                    if let Some(e) = e {
                        combined = Some(combined.map_or(e, |x| x.max(e)));
                        if i < TRACE_BOARDS {
                            per_board[i] = e;
                        }
                    }
                }
                let mut shifts = [0u8; TRACE_BOARDS];
                if self.channels[c].relative {
                    combined = relative_level(&mut self.boards, c, combined, &mut shifts);
                }
                let time_us = slot * SLOT_US - ENVELOPE_DELAY_US;
                self.channels[c].step(time_us, combined, &self.config, &mut events);
                if let Some(trace) = &mut self.trace {
                    let ch = &self.channels[c];
                    let code = |x: Option<f32>| x.map_or(0, |v| v.round().clamp(1.0, 255.0) as u8);
                    trace[c].push(TracePoint {
                        time_us,
                        level: combined.unwrap_or(0),
                        boards: per_board,
                        enter: code(ch.enter),
                        exit: code(ch.exit),
                        shifts,
                    });
                }
            }
            slot += 1;
        }
        self.next_slot = Some(slot);
        events
    }
}

/// `AutoRelative`, one slot of channel `c`: the boards' envelopes of this
/// slot (`BoardChannel::last`), each shifted so that its course lies on the
/// highest course of the boards (the reference), and of those the maximum.
/// `plain` is the plain maximum, returned while that can't be done.
/// Notes each board's shift in `shifts` (for the trace) and in the board.
fn relative_level(boards: &mut [BoardState], c: usize, plain: Option<u8>, shifts: &mut [u8; TRACE_BOARDS]) -> Option<u8> {
    let on = db_code(ON_DB);
    // The reference: the highest course among the boards that know theirs.
    let reference = boards.iter().filter_map(|b| b.channels[c].course).max();
    let mut level: Option<u8> = None;
    for (i, b) in boards.iter_mut().enumerate() {
        let bc = &mut b.channels[c];
        // `zip`: Some only if both are (a board without a course is left out).
        bc.shift = reference.zip(bc.course).map(|(r, own)| r - own);
        if i < TRACE_BOARDS {
            shifts[i] = bc.shift.unwrap_or(SHIFT_NONE);
        }
        // Only boards that hear the transmitter right now: a quiet board's
        // noise, shifted up, would look like "transmitter on" to the
        // channel's statistics and pull its course down.
        if let (Some(shift), Some(e)) = (bc.shift, bc.last)
            && bc.fresh
            && e >= on
        {
            let shifted = e.saturating_add(shift);
            level = Some(level.map_or(shifted, |x| x.max(shifted)));
        }
    }
    // No board counts (none has a course yet, or the transmitter is off):
    // the plain maximum. Below -46 dBFS it is ignored by the statistics; in
    // the first seconds it is what the plain automatic mode would see.
    level.or(plain)
}

impl BoardChannel {
    /// The envelope after `slot`, or None while there is too little data.
    /// Also feeds the board's own statistics.
    fn finish_slot(&mut self, slot: i64) -> Option<u8> {
        let e = self.envelope(slot);
        self.fresh = e.is_some();
        if let Some(e) = e {
            // `get_or_insert_with`: makes the statistics on the first level.
            self.stats.get_or_insert_with(LevelStats::new).add(e);
        }
        e
    }

    fn envelope(&mut self, slot: i64) -> Option<u8> {
        // Drop anything older (can only happen if a board's clock jumped).
        while self.pending.front().is_some_and(|(s, _)| *s < slot) {
            self.pending.pop_front();
        }
        let this = match self.pending.front() {
            Some((s, max)) if *s == slot => {
                let m = *max;
                self.pending.pop_front();
                Some(m)
            }
            _ => None,
        };
        self.recent_max.push_back(this);
        if self.recent_max.len() > MAX_SLOTS {
            self.recent_max.pop_front();
        }
        // `flatten` skips the slots without data.
        let max20 = self.recent_max.iter().flatten().max().copied()?;
        self.recent_env.push_back(max20);
        if self.recent_env.len() > MEDIAN_SLOTS {
            self.recent_env.pop_front();
        }
        if self.recent_env.len() < MEDIAN_SLOTS {
            return None;
        }
        self.last = Some(median(self.recent_env.iter().copied()));
        self.last
    }
}

/// The pass state machine of one channel.
struct ChannelDetector {
    channel: u8,
    /// `DetectConfig::min_rise_db`, or the channel's own (`set_min_rise`).
    min_rise_db: f32,
    stats: LevelStats,
    /// The last pass peaks (codes).
    peaks: VecDeque<u8>,
    enter: Option<f32>,
    exit: Option<f32>,
    last_level: Option<u8>,
    /// The crossing in progress: (time, level) of each slot in it.
    crossing: Vec<(i64, u8)>,
    /// A pass waiting for HOLD_US, and when its crossing ended.
    pending: Option<(Pass, i64)>,
    last_reported_us: Option<i64>,
    /// The last reported pass (for step 6b), and the one the pending pass
    /// will replace, if any.
    last_pass: Option<Pass>,
    replaces: Option<Pass>,
    /// Step 6b on (the finish gate).
    replace: bool,
    /// Passes reported during start-up (step 7), until it is over.
    provisional: Vec<Pass>,
    started: bool,
    /// Fixed (enter, exit) thresholds in codes, instead of learned ones.
    fixed: Option<(f32, f32)>,
    /// The last slots (time, level) until the first learned thresholds, to
    /// look back through then (step 8); None once that is done.
    lookback: Option<VecDeque<(i64, u8)>>,
    /// `DetectMode::AutoRelative`: the level this channel gets is the
    /// maximum of the boards shifted to a common course (`relative_level`).
    relative: bool,
}

impl ChannelDetector {
    fn new(channel: u8, min_rise_db: f32) -> ChannelDetector {
        ChannelDetector {
            channel,
            min_rise_db,
            stats: LevelStats::new(),
            peaks: VecDeque::new(),
            enter: None,
            exit: None,
            last_level: None,
            crossing: Vec::new(),
            pending: None,
            last_reported_us: None,
            last_pass: None,
            replaces: None,
            replace: true,
            provisional: Vec::new(),
            started: false,
            fixed: None,
            lookback: Some(VecDeque::new()),
            relative: false,
        }
    }

    fn status(&self) -> ChannelStatus {
        ChannelStatus {
            level_db: self.last_level.map(rssi_dbfs),
            enter_db: self.enter.map(code_db),
            exit_db: self.exit.map(code_db),
            in_pass: !self.crossing.is_empty(),
        }
    }

    fn step(&mut self, time_us: i64, level: Option<u8>, config: &DetectConfig, out: &mut Vec<Event>) {
        self.last_level = level;
        if let Some(l) = level {
            self.stats.add(l);
        }
        // Thresholds: recomputed once a second, cheap enough and steady.
        if time_us.div_euclid(SLOT_US) % 100 == 0 {
            self.update_thresholds();
        }
        // Step 8: keep the slots until the first learned thresholds, then go
        // through them (this slot included) instead of only this one.
        // `take` moves the buffer out, leaving None: it is done only once.
        if let (Some(buf), Some(l)) = (&mut self.lookback, level) {
            buf.push_back((time_us, l));
            if buf.len() > LOOKBACK_SLOTS {
                buf.pop_front();
            }
        }
        if self.enter.is_some() && self.lookback.is_some() {
            let buf = self.lookback.take().unwrap();
            // Fixed thresholds were there from the start: nothing was missed.
            if self.fixed.is_none() {
                for (t, l) in buf {
                    self.cross(t, l, config);
                    // As live: a pass whose hold time is over gets reported.
                    self.finish_pending(t, false, out);
                }
                self.finish_pending(time_us, false, out);
                return;
            }
        }
        if let Some(l) = level {
            self.cross(time_us, l, config);
        }
        self.finish_pending(time_us, false, out);
    }

    /// One slot through the crossing state machine (needs thresholds).
    fn cross(&mut self, time_us: i64, l: u8, config: &DetectConfig) {
        let (Some(enter), Some(exit)) = (self.enter, self.exit) else { return };
        let l_f = l as f32;
        if self.crossing.is_empty() {
            if l_f >= enter {
                self.crossing.push((time_us, l));
            }
        } else if l_f >= exit {
            self.crossing.push((time_us, l));
        } else {
            self.end_crossing(time_us, config);
        }
    }

    /// The level fell below exit: turn the crossing into a pass.
    fn end_crossing(&mut self, time_us: i64, config: &DetectConfig) {
        let crossing = std::mem::take(&mut self.crossing);
        let peak = crossing.iter().map(|(_, l)| *l).max().unwrap();
        let top: Vec<i64> = crossing
            .iter()
            .filter(|(_, l)| *l + TOP_CODES >= peak)
            .map(|(t, _)| *t)
            .collect();
        let pass = Pass {
            channel: self.channel,
            time_us: (top[0] + top[top.len() - 1]) / 2,
            peak_db: rssi_dbfs(peak),
        };

        if let Some((p, _)) = &self.pending
            && pass.time_us - p.time_us < config.min_lap_us
        {
            // The same pass (a dip in the gate): keep the stronger one.
            if pass.peak_db > p.peak_db {
                self.pending = Some((pass, time_us));
            } else if let Some(pending) = &mut self.pending {
                pending.1 = time_us; // hold on a bit longer
            }
            return;
        }
        if self
            .last_reported_us
            .is_some_and(|t| pass.time_us - t < config.min_lap_us)
        {
            // Step 6b: a much stronger pass shortly after the reported one
            // replaces it. Measured 2026-11-13 on sessions 14-23: the reported
            // one was a weaker hump 0.7-1.7 s before the real pass (the
            // arming pass lay between the two in all 16 cases). A pilot
            // flying through again is not caught: turning back takes longer,
            // and two clean passes are about equally strong.
            if self.replace
                && let Some(last) = self.last_pass
                && pass.time_us - last.time_us <= REPLACE_WITHIN_US
                && pass.peak_db >= last.peak_db + REPLACE_STRONGER_DB
            {
                self.pending = Some((pass, time_us));
                self.replaces = Some(last);
            }
            return; // too soon after the reported pass
        }
        // A new pass. (A pending one that is too old was reported by
        // finish_pending already, as HOLD_US < min lap.)
        self.pending = Some((pass, time_us));
    }

    /// Reports the pending pass once its hold time is over (or `force`).
    fn finish_pending(&mut self, now_us: i64, force: bool, out: &mut Vec<Event>) {
        let Some((pass, ended)) = self.pending else { return };
        if !force && now_us - ended < HOLD_US {
            return;
        }
        self.pending = None;
        // Step 6b: first take back the pass this one replaces.
        if let Some(old) = self.replaces.take() {
            out.push(Event::Retract(old));
            if let Some(i) = self.peaks.iter().rposition(|&c| c == db_code(old.peak_db)) {
                self.peaks.remove(i);
            }
            self.provisional.retain(|p| p.time_us != old.time_us);
        }
        self.last_reported_us = Some(pass.time_us);
        self.last_pass = Some(pass);
        self.peaks.push_back(db_code(pass.peak_db));
        if self.peaks.len() > PEAK_HISTORY {
            self.peaks.pop_front();
        }
        out.push(Event::Pass(pass));
        // Learning (and taking back early passes) is only for learned thresholds.
        if !self.started && self.fixed.is_none() {
            self.provisional.push(pass);
            if self.peaks.len() >= MIN_PEAKS {
                self.review_start(out);
            }
        }
    }

    /// Step 7: with a real peak known, take back the early passes that
    /// would not have reached the threshold.
    fn review_start(&mut self, out: &mut Vec<Event>) {
        let Some((course, _)) = self.stats.course_and_high() else {
            return;
        };
        let peak = median(self.peaks.iter().copied()) as f32;
        let enter = course + ENTER_FRACTION * (peak - course);
        // `retain` keeps the passes for which the closure returns true.
        self.provisional.retain(|p| {
            let keep = db_code(p.peak_db) as f32 >= enter;
            if !keep {
                out.push(Event::Retract(*p));
                if let Some(i) = self.peaks.iter().position(|&c| c == db_code(p.peak_db)) {
                    self.peaks.remove(i);
                }
            }
            keep
        });
        self.last_reported_us = self.provisional.last().map(|p| p.time_us);
        self.last_pass = self.provisional.last().copied();
        if self.peaks.len() >= MIN_PEAKS {
            self.started = true;
            self.provisional.clear();
        }
        self.update_thresholds();
    }

    fn update_thresholds(&mut self) {
        if let Some((enter, exit)) = self.fixed {
            self.enter = Some(enter);
            self.exit = Some(exit.min(enter - 1.0));
            return;
        }
        let Some((course, high)) = self.stats.course_and_high() else {
            self.enter = None;
            self.exit = None;
            return;
        };
        let known = self.peaks.len() >= MIN_PEAKS;
        let peak = if known {
            median(self.peaks.iter().copied()) as f32
        } else {
            high
        };
        if peak - course < MIN_CONTRAST {
            self.enter = None;
            self.exit = None;
            return;
        }
        let mut enter = course + ENTER_FRACTION * (peak - course);
        // The estimate can sit in the course's own wiggles (start-up), or low
        // peaks (flybys 1-2 m above the gate) can pull the median down: always
        // demand a clear rise above the course.
        enter = enter.max(course + self.min_rise_db * 4.0);
        self.enter = Some(enter);
        self.exit = Some((course + EXIT_FRACTION * (peak - course)).min(enter - 4.0));
    }
}

/// A histogram of the combined envelope over the last STATS_SLOTS slots.
struct LevelStats {
    window: VecDeque<u8>,
    counts: [u32; 256],
}

impl LevelStats {
    fn new() -> LevelStats {
        LevelStats {
            window: VecDeque::new(),
            counts: [0; 256],
        }
    }

    fn add(&mut self, level: u8) {
        self.window.push_back(level);
        self.counts[level as usize] += 1;
        if self.window.len() > STATS_SLOTS {
            let old = self.window.pop_front().unwrap();
            self.counts[old as usize] -= 1;
        }
    }

    /// (course, high) in codes: the median and the 99.5th percentile of the
    /// levels while the transmitter is on. None while warming up.
    fn course_and_high(&self) -> Option<(f32, f32)> {
        let on_from = db_code(ON_DB) as usize;
        let on: u32 = self.counts[on_from..].iter().sum();
        if on < WARMUP_SLOTS {
            return None;
        }
        let course = self.percentile(on_from, 0.5)?;
        let high = self.percentile(on_from, 0.995)?;
        Some((course as f32, high as f32))
    }

    /// Only the course (one board's, `AutoRelative`), as a code.
    fn course(&self) -> Option<u8> {
        let on_from = db_code(ON_DB) as usize;
        let on: u32 = self.counts[on_from..].iter().sum();
        if on < WARMUP_SLOTS {
            return None;
        }
        self.percentile(on_from, 0.5).map(|c| c as u8)
    }

    /// The code below which `fraction` of the levels from code `from` up lie.
    fn percentile(&self, from: usize, fraction: f64) -> Option<u32> {
        let total: u32 = self.counts[from..].iter().sum();
        if total == 0 {
            return None;
        }
        let target = (fraction * total as f64).ceil().max(1.0) as u32;
        let mut seen = 0;
        for (code, &n) in self.counts.iter().enumerate().skip(from) {
            seen += n;
            if seen >= target {
                return Some(code as u32);
            }
        }
        None
    }
}

fn median(values: impl Iterator<Item = u8>) -> u8 {
    let mut v: Vec<u8> = values.collect();
    v.sort_unstable();
    v[v.len() / 2]
}

fn code_db(code: f32) -> f32 {
    code / 4.0 - 63.75
}

/// dBFS → level code, unrounded (thresholds).
fn db_code_f(db: f32) -> f32 {
    (db + 63.75) * 4.0
}

fn db_code(db: f32) -> u8 {
    ((db + 63.75) * 4.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two boards, one channel. Every 2 ms a level: noise in every 5th
    /// capture (DJI pauses), else the course level `level_at(t)` or a pass
    /// (board 1 2 dB weaker).
    fn simulate(passes_s: &[f64], seconds: f64, level_at: impl Fn(f64) -> f64) -> Vec<Pass> {
        let events = simulate_events(passes_s, seconds, level_at);
        final_passes(&events)
    }

    /// The passes left after the retractions.
    fn final_passes(events: &[Event]) -> Vec<Pass> {
        let mut passes = Vec::new();
        for e in events {
            match e {
                Event::Pass(p) => passes.push(*p),
                Event::Retract(p) => passes.retain(|q| q != p),
            }
        }
        passes
    }

    fn simulate_events(passes_s: &[f64], seconds: f64, level_at: impl Fn(f64) -> f64) -> Vec<Event> {
        let mut det = Detector::new(1, DetectConfig::default());
        let mut found = Vec::new();
        let mut k = 0u64;
        let mut t = 0.0;
        while t < seconds {
            for board in 0..2 {
                let mut db = level_at(t);
                for p in passes_s {
                    // At the gate the level peaks at -15 dBFS whatever it
                    // was before, over about half a second.
                    db = db.max(-15.0 - 30.0 * (1.0 - (-((t - p) / 0.2).powi(2)).exp()));
                }
                db -= board as f64 * 2.0;
                if k % 5 == 0 {
                    db = -56.0;
                }
                let rssi = db_code(db as f32);
                let host_us = (t * 1e6) as i64;
                det.push(
                    board,
                    &[Level {
                        host_us,
                        board_us: host_us as u64,
                        channel: 0,
                        rssi,
                        flags: 0,
                    }],
                );
            }
            k += 1;
            t += 0.002;
            if k % 5 == 0 {
                found.extend(det.advance());
            }
        }
        found.extend(det.flush());
        found
    }

    #[test]
    fn finds_passes_despite_dropouts() {
        let passes = [20.0, 27.5, 34.0, 41.2, 48.9];
        // Course level wandering between -35 and -25 dBFS: its high moments
        // must not count (the detector demands a rise of MIN_RISE_DB).
        let events = simulate_events(&passes, 55.0, |t| -30.0 + 5.0 * (t * 0.7).sin());
        assert!(!events.iter().any(|e| matches!(e, Event::Retract(_))), "{events:?}");
        let found = final_passes(&events);
        let times: Vec<f64> = found.iter().map(|p| p.time_us as f64 / 1e6).collect();
        assert_eq!(found.len(), passes.len(), "found {times:?}");
        for (f, p) in times.iter().zip(passes) {
            // The envelope's delay is taken out: within 30 ms.
            assert!((f - p).abs() < 0.03, "pass at {p} found at {f}");
        }
    }

    #[test]
    fn a_pass_before_the_first_thresholds() {
        // Step 8: the first pass at 3 s, before the channel has its 5 s of
        // levels: found late, with its real time, and kept.
        let passes = [3.0, 12.0, 21.0, 30.0];
        let events = simulate_events(&passes, 36.0, |_| -40.0);
        let found = final_passes(&events);
        let times: Vec<f64> = found.iter().map(|p| p.time_us as f64 / 1e6).collect();
        assert_eq!(found.len(), passes.len(), "found {times:?}");
        for (f, p) in times.iter().zip(passes) {
            assert!((f - p).abs() < 0.03, "pass at {p} found at {f}");
        }
    }

    /// Like `simulate_events`, but each hump with its own peak (dBFS):
    /// (time s, peak dBFS). Course -45 dBFS, both boards alike.
    fn simulate_humps(humps: &[(f64, f64)], seconds: f64) -> Vec<Event> {
        let mut det = Detector::new(1, DetectConfig::default());
        let mut found = Vec::new();
        let mut k = 0u64;
        let mut t = 0.0;
        while t < seconds {
            let mut db: f64 = -45.0;
            for &(at, peak) in humps {
                db = db.max(peak - 30.0 * (1.0 - (-((t - at) / 0.2).powi(2)).exp()));
            }
            let rssi = db_code(db as f32);
            let host_us = (t * 1e6) as i64;
            for board in 0..2 {
                det.push(board, &[Level { host_us, board_us: host_us as u64, channel: 0, rssi, flags: 0 }]);
            }
            k += 1;
            t += 0.002;
            if k % 5 == 0 {
                found.extend(det.advance());
            }
        }
        found.extend(det.flush());
        found
    }

    /// Five clean passes at -15 dBFS every 10 s, plus `extra`.
    fn laps_with(extra: &[(f64, f64)]) -> Vec<f64> {
        let mut humps: Vec<(f64, f64)> = [10.0, 20.0, 30.0, 40.0, 50.0].iter().map(|&t| (t, -15.0)).collect();
        humps.extend_from_slice(extra);
        let events = simulate_humps(&humps, 56.0);
        final_passes(&events).iter().map(|p| p.time_us as f64 / 1e6).collect()
    }

    #[test]
    fn a_hump_before_the_gate_is_replaced_by_the_pass() {
        // Step 6b: a weak hump (-24 dBFS) 1.4 s before the pass at 40 s is
        // reported first; the pass, 9 dB stronger, replaces it.
        let times = laps_with(&[(38.6, -24.0)]);
        assert_eq!(times.len(), 5, "{times:?}");
        assert!(times.iter().any(|t| (t - 40.0).abs() < 0.03), "{times:?}");
        assert!(!times.iter().any(|t| (t - 38.6).abs() < 0.1), "{times:?}");
    }

    #[test]
    fn flying_through_again_keeps_the_first() {
        // The pilot flies through at 40 s and again 1.5 s later, just as
        // strong: the first counts, the second is too soon.
        let times = laps_with(&[(41.5, -15.0)]);
        assert_eq!(times.len(), 5, "{times:?}");
        assert!(times.iter().any(|t| (t - 40.0).abs() < 0.03), "{times:?}");
        // Stronger, but more than 2 s later: also not replaced.
        let times = laps_with(&[(42.3, -9.0)]);
        assert!(times.iter().any(|t| (t - 40.0).abs() < 0.03), "{times:?}");
        assert!(!times.iter().any(|t| (t - 42.3).abs() < 0.1), "{times:?}");
    }

    #[test]
    fn nothing_while_the_level_only_wobbles() {
        // ±1 dB of quick wobble around -30 dBFS, no pass.
        let found = simulate(&[], 40.0, |t| -30.0 + (t * 37.0).sin() * (t * 5.3).cos());
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn fixed_thresholds() {
        // Classic mode with the enter level above the simulated peaks (-15
        // dBFS): nothing counts; just below them: every pass from the first.
        let run = |enter: f32| {
            let mut det = Detector::new(1, DetectConfig::default());
            det.set_fixed(&[Some((enter, enter - 6.0))]);
            let mut found = Vec::new();
            for k in 0..15_000u64 {
                let t = k as f64 * 0.002;
                let mut db = -40.0f64;
                for p in [5.0, 12.0, 19.0] {
                    db = db.max(-15.0 - 30.0 * (1.0 - (-((t - p) / 0.2f64).powi(2)).exp()));
                }
                let l = Level {
                    host_us: (t * 1e6) as i64,
                    board_us: 0,
                    channel: 0,
                    rssi: db_code(db as f32),
                    flags: 0,
                };
                det.push(0, &[l]);
                found.extend(det.advance());
            }
            found.extend(det.flush());
            final_passes(&found).len()
        };
        assert_eq!(run(-12.0), 0);
        assert_eq!(run(-20.0), 3);
    }

    #[test]
    fn thresholds_changed_during_the_run() {
        // Fixed thresholds, peaks at -15 dBFS. `switch`: at this time (s) the
        // enter level changes from `before` to `after`.
        let run = |passes: &[f64], switch: f64, before: f32, after: f32| {
            let fixed = |enter: f32| DetectionSettings {
                finish_mode: DetectMode::Classic,
                finish: vec![Thresholds {
                    enter_db: enter,
                    exit_db: enter - 6.0,
                    mode: None,
                    min_rise_db: None,
                }],
                ..Default::default()
            };
            let mut det = Detector::new(1, DetectConfig::default());
            det.set_detection(&fixed(before), false);
            let mut found = Vec::new();
            let mut switched = false;
            for k in 0..15_000u64 {
                let t = k as f64 * 0.002;
                if !switched && t >= switch {
                    det.set_detection(&fixed(after), false);
                    switched = true;
                }
                let mut db = -40.0f64;
                for p in passes {
                    db = db.max(-15.0 - 30.0 * (1.0 - (-((t - p) / 0.2f64).powi(2)).exp()));
                }
                let l = Level {
                    host_us: (t * 1e6) as i64,
                    board_us: 0,
                    channel: 0,
                    rssi: db_code(db as f32),
                    flags: 0,
                };
                det.push(0, &[l]);
                found.extend(det.advance());
            }
            found.extend(det.flush());
            final_passes(&found).len()
        };
        // Too high at first: the first pass is missed, the rest count.
        assert_eq!(run(&[5.0, 12.0, 19.0], 8.0, -12.0, -20.0), 2);
        // The minimum lap still holds across the change: 7.5 s is too close
        // to the pass at 5 s (a fresh detector would count it).
        assert_eq!(run(&[5.0, 7.5], 6.5, -20.0, -21.0), 1);
    }

    #[test]
    fn detection_settings_per_gate() {
        // Saved before the modes were split: "mode" becomes the finish gate's.
        let old: DetectionSettings = serde_json::from_str(r#"{"mode":"classic","finish":[],"arming":[]}"#).unwrap();
        assert_eq!(
            (old.finish_mode, old.arming_mode),
            (DetectMode::Classic, DetectMode::Auto)
        );
        assert!(old.fixed(false, 2).iter().all(|f| f.is_some()));
        assert!(old.fixed(true, 2).iter().all(|f| f.is_none()));
        // A channel's own mode wins over the gate's.
        let mut d = old;
        d.finish = vec![Thresholds {
            mode: Some(DetectMode::Auto),
            ..Default::default()
        }];
        assert_eq!(d.fixed(false, 2), vec![None, Some((-25.0, -30.0))]);
        // A channel's own minimum swing, else the common one.
        d.finish[0].min_rise_db = Some(8.0);
        assert_eq!(d.min_rise(false, 2), vec![8.0, MIN_RISE_DB]);
        assert_eq!(d.min_rise(true, 1), vec![MIN_RISE_DB]);
    }

    /// Two boards with their own levels: `level_at(board, t)` in dBFS,
    /// every 2 ms, noise in every 5th capture. Returns the passes left and
    /// the detector (for its status).
    fn simulate_boards(relative: bool, seconds: f64, level_at: impl Fn(usize, f64) -> f64) -> (Vec<Pass>, Detector) {
        let mut det = Detector::new(1, DetectConfig::default());
        det.set_relative(&[relative]);
        let mut events = Vec::new();
        let mut k = 0u64;
        let mut t = 0.0;
        while t < seconds {
            for board in 0..2 {
                let db = if k % 5 == 0 { -56.0 } else { level_at(board, t) };
                let host_us = (t * 1e6) as i64;
                let l = Level {
                    host_us,
                    board_us: host_us as u64,
                    channel: 0,
                    rssi: db_code(db as f32),
                    flags: 0,
                };
                det.push(board, &[l]);
            }
            k += 1;
            t += 0.002;
            if k % 5 == 0 {
                events.extend(det.advance());
            }
        }
        events.extend(det.flush());
        (final_passes(&events), det)
    }

    /// A hump of `rise` dB at `at` (s), about half a second wide.
    fn hump(t: f64, at: f64, rise: f64) -> f64 {
        rise * (-((t - at) / 0.2).powi(2)).exp()
    }

    #[test]
    fn relative_a_loud_board_does_not_hide_a_quiet_one() {
        // As session 57: board 0 hears the whole course loudly (-35 dBFS)
        // and rises only 8 dB at the gate, except at 30 and 60 s (19 dB).
        // Board 1 is quiet (-44 dBFS) with a clean hump of 17 dB every time,
        // which only reaches -27 dBFS.
        let passes = [10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0];
        let level = |board: usize, t: f64| {
            let mut db: f64 = if board == 0 { -35.0 } else { -44.0 };
            for p in passes {
                let strong = p == 30.0 || p == 60.0;
                db += hump(t, p, if board == 1 { 17.0 } else if strong { 19.0 } else { 8.0 });
            }
            db
        };
        // The plain maximum: board 0 sets the course, 12 dB above it is
        // -23 dBFS, and only its two strong passes count.
        let (plain, _) = simulate_boards(false, 78.0, level);
        assert_eq!(plain.len(), 2, "{plain:?}");
        // Each board against its own course: all seven.
        let (found, det) = simulate_boards(true, 78.0, level);
        let times: Vec<f64> = found.iter().map(|p| p.time_us as f64 / 1e6).collect();
        assert_eq!(found.len(), passes.len(), "found {times:?}");
        for (f, p) in times.iter().zip(passes) {
            assert!((f - p).abs() < 0.03, "pass at {p} found at {f}");
        }
        // Board 0 is the reference (shift 0), board 1 is shifted by the
        // difference of the courses (9 dB); the peaks are "as board 0":
        // -44 + 17 + 9 = -18 dBFS.
        let shifts = det.board_shifts(0);
        assert_eq!(shifts[0], Some(0.0));
        assert!((shifts[1].unwrap() - 9.0).abs() <= 0.5, "{shifts:?}");
        assert!((found[0].peak_db + 18.0).abs() < 1.0, "{found:?}");
    }

    #[test]
    fn relative_wiggles_and_flybys_do_not_count() {
        // Board 0 loud and steady. Board 1 quiet, its course wanders ±2.5 dB
        // (waves of 4-5 dB, session 1), and every 7 s a flyby lifts it 9 dB
        // above its course: measured against its own course that is more
        // than against board 0's, but still below the minimum swing.
        let flybys = [8.0, 15.0, 22.0, 29.0, 36.0, 43.0, 50.0];
        let wiggly = |board: usize, t: f64| {
            if board == 0 {
                return -35.0;
            }
            let mut db = -44.0 + 2.5 * (t * 0.9).sin();
            for f in flybys {
                db += hump(t, f, 9.0);
            }
            db
        };
        let (found, _) = simulate_boards(true, 56.0, wiggly);
        assert!(found.is_empty(), "{found:?}");
        // With real passes (20 dB on both boards) in between: only those.
        let passes = [11.5, 25.5, 39.5, 53.5];
        let (found, _) = simulate_boards(true, 60.0, |board, t| {
            let mut db = wiggly(board, t);
            for p in passes {
                db += hump(t, p, 20.0);
            }
            db
        });
        let times: Vec<f64> = found.iter().map(|p| p.time_us as f64 / 1e6).collect();
        assert_eq!(found.len(), passes.len(), "found {times:?}");
        for (f, p) in times.iter().zip(passes) {
            assert!((f - p).abs() < 0.03, "pass at {p} found at {f}");
        }
    }

    #[test]
    fn relative_with_one_board_is_the_plain_mode() {
        // One board is its own reference (shift 0): nothing changes.
        let run = |relative: bool| {
            let mut det = Detector::new(1, DetectConfig::default());
            det.set_relative(&[relative]);
            let mut events = Vec::new();
            for k in 0..30_000u64 {
                let t = k as f64 * 0.002;
                let mut db = -38.0 + 3.0 * (t * 0.7).sin();
                for p in [9.0, 18.0, 27.0, 36.0, 45.0, 54.0] {
                    db += hump(t, p, 18.0);
                }
                let host_us = (t * 1e6) as i64;
                let l = Level { host_us, board_us: 0, channel: 0, rssi: db_code(db as f32), flags: 0 };
                det.push(0, &[l]);
                events.extend(det.advance());
            }
            events.extend(det.flush());
            events
        };
        let plain = run(false);
        assert_eq!(final_passes(&plain).len(), 6);
        assert_eq!(plain, run(true));
    }

    #[test]
    fn modes_in_the_settings() {
        // The names in the JSON; a channel's own mode wins over the gate's.
        let d: DetectionSettings = serde_json::from_str(
            r#"{"finish_mode":"auto_relative","finish":[{"enter_db":-20,"exit_db":-26,"mode":"classic"},{"enter_db":-20,"exit_db":-26}]}"#,
        )
        .unwrap();
        assert_eq!(d.modes(false, 3), vec![DetectMode::Classic, DetectMode::AutoRelative, DetectMode::AutoRelative]);
        assert_eq!(d.modes(true, 1), vec![DetectMode::Auto]);
        // Only classic channels have fixed thresholds.
        assert_eq!(d.fixed(false, 3), vec![Some((-20.0, -26.0)), None, None]);
        for m in DetectMode::ALL {
            assert_eq!(serde_json::to_value(m).unwrap(), serde_json::json!(m.name()));
            assert_eq!(DetectMode::from_name(m.name()), Some(m));
        }
        assert_eq!(DetectMode::from_name("x"), None);
    }

    #[test]
    fn only_board_1_sends() {
        // E.g. the arming-gate detector: board 0 belongs to the finish gate.
        let mut det = Detector::new(1, DetectConfig::default());
        for k in 0..100 {
            let l = Level {
                host_us: k * 10_000,
                board_us: 0,
                channel: 0,
                rssi: 100,
                flags: 0,
            };
            det.push(1, &[l]);
        }
        det.advance();
        assert!(det.next_slot.unwrap() >= 98);
        assert!(det.board_level(1).is_some());
    }

    #[test]
    fn a_quiet_board_does_not_stall_the_others() {
        let mut det = Detector::new(1, DetectConfig::default());
        let level = |host_us| Level {
            host_us,
            board_us: 0,
            channel: 0,
            rssi: 100,
            flags: 0,
        };
        det.push(0, &[level(0)]);
        det.push(1, &[level(0)]);
        for k in 1..100 {
            det.push(0, &[level(k * 10_000)]); // board 1 says nothing more
        }
        det.advance();
        // Board 1 is 990 ms behind, so it no longer holds the slots back.
        assert!(det.next_slot.unwrap() >= 98);
    }
}
