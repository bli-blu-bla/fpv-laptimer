//! Sensor test (Boards → Sensortest): for a few seconds, collect every
//! board's raw levels per channel, then show them side by side. Two ways:
//! with every transmitter off (the noise floor: a damaged input or a spur
//! shows as a higher or restless floor), or with one reference transmitter
//! at a fixed spot (a weaker receive path shows as a lower level than the
//! other boards'). The judging is done by the page (`boardTest.js`); here
//! only the numbers.
//!
//! Only reads what comes in anyway: the boards keep running, nothing is sent
//! to them.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::esp::board::{Level, LinkStats};
use crate::esp::frame::rssi_dbfs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestKind {
    /// All transmitters off: the noise floor.
    Noise,
    /// One transmitter at a fixed spot.
    Reference,
}

/// One board on one channel.
#[derive(Clone)]
struct ChannelAcc {
    /// How often each level code came (0.25 dB steps).
    counts: [u32; 256],
    n: u64,
    last_us: Option<i64>,
    max_gap_us: i64,
}

impl Default for ChannelAcc {
    fn default() -> ChannelAcc {
        ChannelAcc { counts: [0; 256], n: 0, last_us: None, max_gap_us: 0 }
    }
}

impl ChannelAcc {
    fn add(&mut self, l: &Level) {
        self.counts[l.rssi as usize] += 1;
        self.n += 1;
        if let Some(t) = self.last_us {
            self.max_gap_us = self.max_gap_us.max(l.host_us - t);
        }
        self.last_us = Some(l.host_us);
    }

    /// The level below which `fraction` of the values lie, dBFS.
    fn percentile(&self, fraction: f64) -> Option<f32> {
        if self.n == 0 {
            return None;
        }
        let target = ((fraction * self.n as f64).ceil() as u64).max(1);
        let mut seen = 0u64;
        for (code, &c) in self.counts.iter().enumerate() {
            seen += c as u64;
            if seen >= target {
                return Some(rssi_dbfs(code as u8));
            }
        }
        None
    }
}

#[derive(Clone, Default)]
struct BoardAcc {
    channels: Vec<ChannelAcc>,
    /// Link counters when the test started and the latest ones.
    stats_first: Option<LinkStats>,
    stats_last: Option<LinkStats>,
}

pub struct BoardTest {
    pub kind: TestKind,
    pub started_us: i64,
    pub until_us: i64,
    n_channels: usize,
    boards: Vec<BoardAcc>,
}

impl BoardTest {
    /// `stats`: each board's link counters now (the test counts from here).
    pub fn new(kind: TestKind, now_us: i64, seconds: f64, n_channels: usize, stats: Vec<Option<LinkStats>>) -> BoardTest {
        let boards = stats
            .into_iter()
            .map(|s| BoardAcc {
                channels: vec![ChannelAcc::default(); n_channels],
                stats_first: s,
                stats_last: s,
            })
            .collect();
        BoardTest {
            kind,
            started_us: now_us,
            until_us: now_us + (seconds * 1e6) as i64,
            n_channels,
            boards,
        }
    }

    pub fn running(&self, now_us: i64) -> bool {
        now_us < self.until_us
    }

    pub fn push(&mut self, board: usize, levels: &[Level]) {
        while self.boards.len() <= board {
            self.boards.push(BoardAcc { channels: vec![ChannelAcc::default(); self.n_channels], ..Default::default() });
        }
        let b = &mut self.boards[board];
        for l in levels {
            // Only what was measured during the test.
            if l.host_us < self.started_us || l.host_us >= self.until_us {
                continue;
            }
            if let Some(c) = b.channels.get_mut(l.channel as usize) {
                c.add(l);
            }
        }
    }

    pub fn stats(&mut self, board: usize, stats: LinkStats) {
        if let Some(b) = self.boards.get_mut(board) {
            b.stats_first.get_or_insert(stats);
            b.stats_last = Some(stats);
        }
    }

    /// Progress and, per board and channel, the numbers. `boards`: (id,
    /// name, role, failed) of each board; `channels`: (name, MHz).
    pub fn json(&self, now_us: i64, boards: &[(String, Option<String>, Value, Option<String>)], channels: &[(String, u32)]) -> Value {
        let seconds = (self.until_us - self.started_us) as f64 / 1e6;
        let elapsed = ((now_us.min(self.until_us) - self.started_us) as f64 / 1e6).max(0.0);
        let list: Vec<Value> = self
            .boards
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let (id, name, role, failed) = boards.get(i).cloned().unwrap_or((format!("{}", i + 1), None, Value::Null, None));
                // Counters that went up during the test.
                let delta = |f: fn(&LinkStats) -> u64| match (b.stats_first, b.stats_last) {
                    (Some(a), Some(z)) => Some(f(&z).saturating_sub(f(&a))),
                    _ => None,
                };
                let chans: Vec<Value> = b
                    .channels
                    .iter()
                    .enumerate()
                    .map(|(c, acc)| {
                        let (cname, mhz) = channels.get(c).cloned().unwrap_or_default();
                        json!({
                            "name": cname,
                            "mhz": mhz,
                            "count": acc.n,
                            "per_s": if elapsed > 0.0 { acc.n as f64 / elapsed } else { 0.0 },
                            "p10_db": acc.percentile(0.10),
                            "median_db": acc.percentile(0.5),
                            "p90_db": acc.percentile(0.90),
                            "max_db": acc.percentile(1.0),
                            "max_gap_ms": acc.max_gap_us as f64 / 1000.0,
                        })
                    })
                    .collect();
                json!({
                    "index": i,
                    "id": id,
                    "name": name,
                    "role": role,
                    "failed": failed,
                    "lost": delta(|s| s.lost),
                    "crc_errors": delta(|s| s.crc_errors),
                    "clock_uncertainty_us": b.stats_last.map(|s| s.clock_uncertainty_us),
                    "drift_ppm": b.stats_last.and_then(|s| s.drift_ppm),
                    "channels": chans,
                })
            })
            .collect();
        json!({
            "kind": self.kind,
            "started_us": self.started_us,
            "seconds": seconds,
            "elapsed_s": elapsed,
            "running": self.running(now_us),
            "boards": list,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(host_us: i64, channel: u8, rssi: u8) -> Level {
        Level { host_us, board_us: 0, channel, rssi, flags: 0 }
    }

    #[test]
    fn counts_only_the_test_window_and_finds_gaps() {
        let mut t = BoardTest::new(TestKind::Noise, 1_000, 1.0, 2, vec![None]);
        // Before the start: not counted.
        t.push(0, &[level(500, 0, 200)]);
        // Channel 0: 4 values, one gap of 100 ms; channel 1 nothing.
        t.push(0, &[level(1_000, 0, 20), level(3_000, 0, 24), level(103_000, 0, 28), level(105_000, 0, 32)]);
        // After the end: not counted.
        t.push(0, &[level(1_001_000, 0, 250)]);
        let c = &t.boards[0].channels[0];
        assert_eq!(c.n, 4);
        assert_eq!(c.max_gap_us, 100_000);
        assert_eq!(c.percentile(0.5), Some(rssi_dbfs(24)));
        assert_eq!(c.percentile(1.0), Some(rssi_dbfs(32)));
        assert_eq!(t.boards[0].channels[1].percentile(0.5), None);
    }

    #[test]
    fn link_counters_count_from_the_start() {
        let s = |lost| LinkStats { crc_errors: 1, lost, clock_uncertainty_us: 0.5, drift_ppm: None };
        let mut t = BoardTest::new(TestKind::Noise, 0, 1.0, 1, vec![Some(s(10))]);
        t.stats(0, s(13));
        let j = t.json(500_000, &[], &[]);
        assert_eq!(j["boards"][0]["lost"], 3);
        assert_eq!(j["boards"][0]["crc_errors"], 0);
        assert_eq!(j["elapsed_s"], 0.5);
    }
}
