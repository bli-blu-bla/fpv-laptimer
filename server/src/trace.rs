//! The level graph of a session. Its level file (written while it ran) goes
//! through the detector once more, which records every 10-ms slot; the page
//! then asks for a time range at the resolution it can draw.
//!
//! The same detector settings and the same levels give the same envelope and
//! thresholds as during the session, so the graph shows what the detection saw.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::detect::{DetectConfig, Detector, Event, TRACE_BOARDS, TracePoint};
use crate::esp::board::Level;
use crate::esp::frame::rssi_dbfs;

pub struct SessionTrace {
    /// host_us + offset_us = wall clock µs.
    offset_us: i64,
    channels: Vec<Vec<TracePoint>>,
    /// What the detector reported (passes and retractions), host time.
    events: Vec<Event>,
}

/// A time range of one channel, ready to draw. Times are wall clock µs,
/// levels dBFS (None: no data).
#[derive(Debug, Serialize)]
pub struct Graph {
    pub time_us: Vec<i64>,
    pub level: Vec<Option<f32>>,
    /// Per board.
    pub boards: Vec<Vec<Option<f32>>>,
    pub enter: Vec<Option<f32>>,
    pub exit: Vec<Option<f32>>,
}

/// Reads a level file (`core.rs` writes one per session, `laptimer record`
/// writes the same format) and runs it through the detector: the finish-gate
/// boards only (from the "# roles:" line; without it, all boards), with the
/// session's fixed thresholds in classic mode.
pub fn load(path: &Path, config: DetectConfig, fixed: Vec<Option<(f32, f32)>>) -> Result<SessionTrace> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut offset_us = 0;
    let mut det: Option<Detector> = None;
    let mut n_channels = 0;
    // Boards that are not on the finish gate ("1=arming", "2=off").
    let mut skip: Vec<usize> = Vec::new();
    let mut events: Vec<Event> = Vec::new();
    for (n, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if let Some(rest) = line.strip_prefix("# channels:") {
            n_channels = rest.split_whitespace().count();
            continue;
        }
        if let Some(rest) = line.strip_prefix("# roles:") {
            skip = rest
                .split_whitespace()
                .filter_map(|r| r.split_once('='))
                .filter(|(_, role)| *role != "finish")
                .filter_map(|(b, _)| b.parse().ok())
                .collect();
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
        if skip.contains(&(f[0] as usize)) {
            continue;
        }
        let d = det.get_or_insert_with(|| {
            let mut d = Detector::with_trace(n_channels.max(1), config);
            d.set_fixed(&fixed);
            d
        });
        let level = Level {
            host_us: f[2],
            board_us: f[3] as u64,
            channel: f[1] as u8,
            rssi: f[4] as u8,
            flags: f[5] as u8,
        };
        d.push(f[0] as usize, &[level]);
        if n % 16 == 0 {
            events.extend(d.advance());
        }
    }
    let channels = match det {
        Some(mut d) => {
            events.extend(d.flush());
            d.take_trace()
        }
        None => Vec::new(),
    };
    Ok(SessionTrace { offset_us, channels, events })
}

fn db(code: u8) -> Option<f32> {
    (code > 0).then(|| rssi_dbfs(code))
}

impl SessionTrace {
    fn points(&self, channel: usize) -> &[TracePoint] {
        self.channels.get(channel).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// host_us + this = wall clock µs.
    pub fn offset_us(&self) -> i64 {
        self.offset_us
    }

    /// Every slot of a channel (host time).
    pub fn slots(&self, channel: usize) -> &[TracePoint] {
        self.points(channel)
    }

    /// The detector's passes and retractions, in the order it reported them.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// First and last time of a channel, wall clock µs.
    pub fn bounds(&self, channel: usize) -> Option<(i64, i64)> {
        let p = self.points(channel);
        Some((p.first()?.time_us + self.offset_us, p.last()?.time_us + self.offset_us))
    }

    /// `from_us`..`to_us` (wall clock) in at most `max_points` points: each
    /// point the highest level of its part of the range, so no peak is lost.
    pub fn graph(&self, channel: usize, from_us: i64, to_us: i64, max_points: usize) -> Graph {
        let pts = self.points(channel);
        let (from, to) = (from_us - self.offset_us, to_us - self.offset_us);
        let start = pts.partition_point(|p| p.time_us < from);
        let end = pts.partition_point(|p| p.time_us <= to);
        let slice = &pts[start..end];
        let per = slice.len().div_ceil(max_points.max(1)).max(1);
        let mut g = Graph {
            time_us: Vec::new(),
            level: Vec::new(),
            boards: vec![Vec::new(); TRACE_BOARDS],
            enter: Vec::new(),
            exit: Vec::new(),
        };
        for chunk in slice.chunks(per) {
            // The point with the highest level stands for the chunk.
            let top = chunk.iter().max_by_key(|p| p.level).unwrap();
            g.time_us.push(top.time_us + self.offset_us);
            g.level.push(db(top.level));
            for (b, out) in g.boards.iter_mut().enumerate() {
                out.push(db(chunk.iter().map(|p| p.boards[b]).max().unwrap_or(0)));
            }
            g.enter.push(db(top.enter));
            g.exit.push(db(top.exit));
        }
        // Boards that never had data are left out.
        g.boards.retain(|b| b.iter().any(|x| x.is_some()));
        g
    }

    /// The pass nearest to `time_us` as the detector would place it: the
    /// highest level within `window_us` (followed uphill if it is the slope
    /// of a peak just outside); around it the hump down to 3 dB
    /// below; in that, from the first to the last point within 1 dB of the
    /// top, the middle (dips in between don't matter, as in the detector).
    /// Returns (wall clock µs, peak dBFS).
    pub fn snap(&self, channel: usize, time_us: i64, window_us: i64) -> Option<(i64, f32)> {
        let pts = self.points(channel);
        let t = time_us - self.offset_us;
        let start = pts.partition_point(|p| p.time_us < t - window_us);
        let end = pts.partition_point(|p| p.time_us <= t + window_us);
        // The highest point in the window (indices into all points from here)...
        let mut i_max = (start..end).max_by_key(|&i| pts[i].level)?;
        // ...and if that is on the slope of a peak outside the window, climb
        // to its top: move to the highest point within ±5 slots (50 ms, to
        // step over small wiggles) while that is higher.
        loop {
            let near = i_max.saturating_sub(5)..(i_max + 6).min(pts.len());
            let best = near.max_by_key(|&i| pts[i].level).unwrap();
            if pts[best].level <= pts[i_max].level {
                break;
            }
            i_max = best;
        }
        let top = pts[i_max].level;
        if top == 0 {
            return None;
        }
        // The hump: walk out from the top while within 3 dB (12 codes).
        let in_hump = |p: &TracePoint| p.level as u16 + 12 >= top as u16;
        let mut a = i_max;
        while a > 0 && in_hump(&pts[a - 1]) {
            a -= 1;
        }
        let mut b = i_max;
        while b + 1 < pts.len() && in_hump(&pts[b + 1]) {
            b += 1;
        }
        // In it, the first and the last point within 1 dB (4 codes) of the top.
        let hump = &pts[a..=b];
        let near = |p: &&TracePoint| p.level + 4 >= top;
        let first = hump.iter().find(near).unwrap();
        let last = hump.iter().rev().find(near).unwrap();
        let mid = (first.time_us + last.time_us) / 2;
        Some((mid + self.offset_us, rssi_dbfs(top)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::SLOT_US;

    fn trace(levels: &[u8]) -> SessionTrace {
        let pts = levels
            .iter()
            .enumerate()
            .map(|(i, &l)| TracePoint {
                time_us: i as i64 * SLOT_US,
                level: l,
                ..Default::default()
            })
            .collect();
        SessionTrace {
            offset_us: 1_000_000,
            channels: vec![pts],
        }
    }

    #[test]
    fn snap_to_the_middle_of_the_top() {
        // A hump from slot 8 to 18; within 1 dB (4 codes) of the top at 10,
        // 11, 13 and 16 (with a dip at 14-15 in between).
        let mut l = vec![100u8; 30];
        for (i, v) in [
            (8, 145),
            (9, 147),
            (10, 150),
            (11, 152),
            (12, 147),
            (13, 149),
            (14, 142),
            (15, 141),
            (16, 150),
            (17, 146),
            (18, 140),
        ] {
            l[i] = v;
        }
        let t = trace(&l);
        let (time, peak) = t.snap(0, 1_000_000 + 5 * SLOT_US, 20 * SLOT_US).unwrap();
        assert_eq!(time, 1_000_000 + 13 * SLOT_US); // middle of 10 and 16
        assert_eq!(peak, rssi_dbfs(152));
        // A tap 9 slots after the peak, with a window of 4: the window only
        // holds the slope (slots 16-18 are in it), the snap still finds the top.
        let (time, _) = t.snap(0, 1_000_000 + 20 * SLOT_US, 4 * SLOT_US).unwrap();
        assert_eq!(time, 1_000_000 + 13 * SLOT_US);
    }

    #[test]
    fn graph_keeps_peaks() {
        let mut l = vec![100u8; 1000];
        l[537] = 200;
        let g = trace(&l).graph(0, 0, i64::MAX, 50);
        assert!(g.time_us.len() <= 50);
        assert!(g.level.contains(&Some(rssi_dbfs(200))));
        assert!(g.boards.is_empty()); // no board data in this trace
    }
}
