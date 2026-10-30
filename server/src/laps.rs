//! From passes to laps: who flew which lap when, depending on how the session
//! starts and ends. A pure function of the session settings and the passes,
//! so a correction (a pass deleted or added) simply means computing again.

use serde::{Deserialize, Serialize};

/// How the first lap starts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartMode {
    /// After the start signal, the first pass is the holeshot (its own time,
    /// not a lap); laps count from there.
    Holeshot,
    /// Lap 1 runs from the start signal to the first pass.
    FromStart,
    /// Every pilot's first pass starts their laps (training).
    Flying,
}

/// When a pilot is done.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum EndRule {
    /// Never (training).
    Open,
    /// After this many laps.
    Laps(u32),
    /// The first pass after this many seconds from the start finishes.
    Seconds(u32),
}

/// Why the server deleted a pass on its own (not the user).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rejected {
    /// The arming rule: no arming-gate pass since the last counted one.
    NoArming,
}

/// One pass as stored, possibly corrected.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PassRecord {
    pub id: i64,
    pub channel: u8,
    /// Wall-clock time, µs since 1970.
    pub time_us: i64,
    pub peak_db: f32,
    /// Added by hand (a pass the detection missed).
    pub manual: bool,
    /// Deleted by hand (a pass that wasn't one); kept to allow undo.
    pub deleted: bool,
    /// 0: the finish gate; 1: the arming gate (no laps of its own).
    #[serde(default)]
    pub gate: u8,
    /// Deleted by the server, and why (None: by hand, or not deleted).
    #[serde(default)]
    pub rejected: Option<Rejected>,
    /// Where the detector put it; None: added by hand. Not `time_us`: moved by hand.
    #[serde(default)]
    pub detected_us: Option<i64>,
}

impl PassRecord {
    /// How the pass came about, for exports and the debug package:
    /// "erkannt", "verschoben" (detected, then moved by hand) or "von Hand".
    pub fn source(&self) -> &'static str {
        match self.detected_us {
            Some(t) if t == self.time_us => "erkannt",
            Some(_) => "verschoben",
            None => "von Hand",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Lap {
    /// 1, 2, …
    pub number: u32,
    pub duration_us: i64,
    /// The pass that ended the lap.
    pub end_us: i64,
    pub pass_id: i64,
}

/// One pilot's (channel's) result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PilotResult {
    pub channel: u8,
    /// Start signal to the first pass (`Holeshot` only).
    pub holeshot_us: Option<i64>,
    pub laps: Vec<Lap>,
    /// Reached the end rule; then the time of the finishing pass.
    pub finished_us: Option<i64>,
    /// From the start to the last counted pass (races with a start signal).
    pub total_us: Option<i64>,
    pub best_lap_us: Option<i64>,
    /// The fastest `consecutive` laps in a row: (sum, number of the first lap).
    pub best_consecutive: Option<(i64, u32)>,
    /// Passes that count for nothing: before the start or after finishing
    /// (`ignored_reason` tells which).
    pub ignored: Vec<i64>,
}

impl PilotResult {
    /// Why an ignored pass counts for nothing: "after_finish" if it came
    /// after the finishing pass, else "before_start".
    pub fn ignored_reason(&self, time_us: i64) -> &'static str {
        if self.finished_us.is_some_and(|f| time_us > f) {
            "after_finish"
        } else {
            "before_start"
        }
    }
}

/// Laps of one channel. `passes`: that channel's passes in any order (deleted
/// ones are skipped); `start_us`: the start signal (for `Flying`, the
/// session start: earlier passes are ignored too); `consecutive`: how many
/// laps in a row the "best consecutive" covers.
pub fn compute(
    channel: u8,
    passes: &[PassRecord],
    start_mode: StartMode,
    end: EndRule,
    start_us: i64,
    consecutive: u32,
) -> PilotResult {
    let mut times: Vec<&PassRecord> = passes
        .iter()
        .filter(|p| !p.deleted && p.channel == channel && p.gate == 0)
        .collect();
    times.sort_by_key(|p| p.time_us);

    let mut r = PilotResult {
        channel,
        holeshot_us: None,
        laps: Vec::new(),
        finished_us: None,
        total_us: None,
        best_lap_us: None,
        best_consecutive: None,
        ignored: Vec::new(),
    };
    // When the current lap began; None until the first lap starts.
    let mut lap_start: Option<i64> = match start_mode {
        StartMode::FromStart => Some(start_us),
        StartMode::Holeshot | StartMode::Flying => None,
    };
    for p in times {
        if p.time_us < start_us || r.finished_us.is_some() {
            r.ignored.push(p.id);
            continue;
        }
        let Some(begin) = lap_start else {
            // The first pass: it starts the laps.
            if start_mode == StartMode::Holeshot {
                r.holeshot_us = Some(p.time_us - start_us);
            }
            lap_start = Some(p.time_us);
            continue;
        };
        r.laps.push(Lap {
            number: r.laps.len() as u32 + 1,
            duration_us: p.time_us - begin,
            end_us: p.time_us,
            pass_id: p.id,
        });
        lap_start = Some(p.time_us);
        let done = match end {
            EndRule::Open => false,
            EndRule::Laps(n) => r.laps.len() as u32 >= n,
            EndRule::Seconds(s) => p.time_us - start_us >= s as i64 * 1_000_000,
        };
        if done {
            r.finished_us = Some(p.time_us);
        }
    }
    if start_mode != StartMode::Flying {
        r.total_us = r.laps.last().map(|l| l.end_us - start_us);
    }
    r.best_lap_us = r.laps.iter().map(|l| l.duration_us).min();
    let n = consecutive.max(1) as usize;
    r.best_consecutive = r
        .laps
        .windows(n)
        .map(|w| (w.iter().map(|l| l.duration_us).sum::<i64>(), w[0].number))
        .min_by_key(|(sum, _)| *sum);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: i64 = 1_000_000;

    fn passes(channel: u8, seconds: &[f64]) -> Vec<PassRecord> {
        seconds
            .iter()
            .enumerate()
            .map(|(i, s)| PassRecord {
                id: i as i64 + 1,
                channel,
                time_us: (s * 1e6) as i64,
                peak_db: -20.0,
                manual: false,
                detected_us: None,
                deleted: false,
                gate: 0,
                rejected: None,
            })
            .collect()
    }

    fn durations(r: &PilotResult) -> Vec<f64> {
        r.laps.iter().map(|l| l.duration_us as f64 / 1e6).collect()
    }

    #[test]
    fn holeshot() {
        let p = passes(0, &[5.0, 12.0, 20.0, 27.0, 40.0, 45.0]);
        let r = compute(0, &p, StartMode::Holeshot, EndRule::Laps(3), 10 * S, 2);
        // 5 s is before the start, 45 s after finishing: ignored. 12 s is the holeshot.
        assert_eq!(r.ignored, vec![1, 6]);
        assert_eq!(r.ignored_reason(5 * S), "before_start");
        assert_eq!(r.ignored_reason(45 * S), "after_finish");
        assert_eq!(r.holeshot_us, Some(2 * S));
        assert_eq!(durations(&r), vec![8.0, 7.0, 13.0]);
        assert_eq!(r.finished_us, Some(40 * S));
        assert_eq!(r.total_us, Some(30 * S));
        assert_eq!(r.best_lap_us, Some(7 * S));
        assert_eq!(r.best_consecutive, Some((15 * S, 1)));
    }

    #[test]
    fn from_start() {
        let p = passes(0, &[12.0, 20.0]);
        let r = compute(0, &p, StartMode::FromStart, EndRule::Laps(5), 10 * S, 3);
        assert_eq!(durations(&r), vec![2.0, 8.0]);
        assert_eq!(r.finished_us, None);
        assert_eq!(r.best_consecutive, None); // fewer than 3 laps
    }

    #[test]
    fn flying_and_open() {
        let p = passes(2, &[3.0, 9.0, 14.5]);
        let r = compute(2, &p, StartMode::Flying, EndRule::Open, 0, 3);
        assert_eq!(durations(&r), vec![6.0, 5.5]);
        assert_eq!(r.total_us, None);
    }

    #[test]
    fn time_limit_and_corrections() {
        let mut p = passes(0, &[11.0, 30.0, 50.0, 70.0, 75.0]);
        p[2].deleted = true; // 50 s was not a pass
        // 60 s race from 10 s: the first pass from 70 s on finishes, 75 s is extra.
        let r = compute(0, &p, StartMode::Holeshot, EndRule::Seconds(60), 10 * S, 3);
        assert_eq!(durations(&r), vec![19.0, 40.0]);
        assert_eq!(r.finished_us, Some(70 * S));
        assert_eq!(r.ignored, vec![5]);
        assert!(!r.ignored.contains(&3)); // deleted passes are not listed
    }

    #[test]
    fn other_channels_are_skipped() {
        let mut p = passes(0, &[1.0, 2.0]);
        p.extend(passes(1, &[5.0, 9.0]));
        let r = compute(1, &p, StartMode::Flying, EndRule::Open, 0, 3);
        assert_eq!(durations(&r), vec![4.0]);
    }
}
