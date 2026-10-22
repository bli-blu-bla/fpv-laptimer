//! Lines up a board's clock with ours.
//!
//! Every board counts microseconds since its own boot, and the crystals of two
//! boards run at slightly different speeds (tens of µs per second). To compare
//! the levels of several boards, we translate each board's times into one
//! common clock: microseconds since this program started ("host time").
//!
//! How: we ask the board for its time (`TIME?`) and note when we sent the
//! question and when the answer arrived. The board read its clock somewhere in
//! between; we assume the middle. The shorter the round trip, the less that
//! assumption can be off, so out of each group of answers we keep only the
//! fastest one: an "estimate" of the offset (board time − host time).
//!
//! Single estimates are off by up to half a round trip (~0.4 ms over USB), and
//! the offset grows slowly (the drift). So we fit a straight line through the
//! estimates of the last few minutes (least squares) and read the offset off
//! that line. The longer the line, the better its slope (the drift) is known.

use std::collections::VecDeque;

/// Answers per estimate; the fastest one wins.
pub const SAMPLES_PER_ESTIMATE: usize = 10;
/// Estimates older than this are dropped from the fit.
const FIT_WINDOW_US: f64 = 300e6;
/// The fit only uses the slope once the estimates span this long; before,
/// the drift is taken as zero.
const MIN_DRIFT_SPAN_US: f64 = 20e6;

#[derive(Debug, Clone, Copy)]
struct Estimate {
    /// Host time of the estimate (middle of the round trip).
    host_us: f64,
    /// board time − host time at that moment.
    offset_us: f64,
    round_trip_us: f64,
}

/// The fitted line: offset(h) = offset_us + drift · (h − host_us).
#[derive(Debug, Clone, Copy, Default)]
struct Line {
    host_us: f64,
    offset_us: f64,
    /// How much faster the board clock runs, as a fraction (1e-6 = 1 ppm).
    drift: f64,
}

#[derive(Default)]
pub struct ClockSync {
    /// (sent, received, board time) of the answers since the last estimate.
    samples: Vec<(u64, u64, u64)>,
    estimates: VecDeque<Estimate>,
    line: Option<Line>,
}

impl ClockSync {
    pub fn new() -> Self {
        Self::default()
    }

    /// One answer to `TIME?`: our time when we asked and when the answer came
    /// in, and the board time in the answer.
    pub fn add(&mut self, sent_us: u64, received_us: u64, board_us: u64) {
        self.samples.push((sent_us, received_us, board_us));
        if self.samples.len() < SAMPLES_PER_ESTIMATE {
            return;
        }
        // `min_by_key` returns a reference into `samples`; `*` copies the
        // tuple out, so we can clear the vector afterwards.
        let (sent, received, board) = *self.samples.iter().min_by_key(|(s, r, _)| r - s).unwrap();
        self.samples.clear();
        let host_us = (sent + received) as f64 / 2.0;
        self.estimates.push_back(Estimate {
            host_us,
            offset_us: board as f64 - host_us,
            round_trip_us: (received - sent) as f64,
        });
        while host_us - self.estimates[0].host_us > FIT_WINDOW_US {
            self.estimates.pop_front();
        }
        self.line = Some(fit(&self.estimates));
    }

    /// True once there is an estimate to convert times with.
    pub fn ready(&self) -> bool {
        self.line.is_some()
    }

    /// Half the round trip of the latest estimate: how far off a single
    /// estimate can be.
    pub fn uncertainty_us(&self) -> Option<f64> {
        self.estimates.back().map(|e| e.round_trip_us / 2.0)
    }

    /// The board's clock drift in ppm (parts per million), once measured.
    pub fn drift_ppm(&self) -> Option<f64> {
        let span = self.estimates.back()?.host_us - self.estimates.front()?.host_us;
        if span < MIN_DRIFT_SPAN_US {
            return None;
        }
        self.line.map(|l| l.drift * 1e6)
    }

    /// Converts a board time into host time (µs since program start).
    pub fn to_host(&self, board_us: u64) -> Option<i64> {
        let l = self.line?;
        // offset(h) = l.offset + drift · (h − l.host), and h = board − offset(h).
        // Solved for h:
        let h = (board_us as f64 - l.offset_us + l.drift * l.host_us) / (1.0 + l.drift);
        Some(h.round() as i64)
    }
}

/// Least-squares line through the estimates (at least one).
fn fit(estimates: &VecDeque<Estimate>) -> Line {
    let n = estimates.len() as f64;
    let mean_h = estimates.iter().map(|e| e.host_us).sum::<f64>() / n;
    let mean_o = estimates.iter().map(|e| e.offset_us).sum::<f64>() / n;
    let span = estimates.back().unwrap().host_us - estimates.front().unwrap().host_us;
    let mut drift = 0.0;
    if span >= MIN_DRIFT_SPAN_US {
        let mut num = 0.0;
        let mut den = 0.0;
        for e in estimates {
            num += (e.host_us - mean_h) * (e.offset_us - mean_o);
            den += (e.host_us - mean_h) * (e.host_us - mean_h);
        }
        drift = num / den;
    }
    Line {
        host_us: mean_h,
        offset_us: mean_o,
        drift,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A simulated board: its clock starts 5 s ahead and runs 20 ppm fast.
    fn board_time(host_us: u64) -> u64 {
        5_000_000 + (host_us as f64 * (1.0 + 20e-6)) as u64
    }

    /// One estimate's worth of answers, the fastest one with an asymmetric
    /// delay (±`skew` µs), as a real USB link has.
    fn sync_at(c: &mut ClockSync, host_us: u64, skew: i64) {
        for k in 0..SAMPLES_PER_ESTIMATE as u64 {
            let rtt = if k == 3 { 800 } else { 3000 };
            let sent = host_us + k * 10_000;
            let answered = (sent + rtt / 2) as i64 + if k == 3 { skew } else { 0 };
            c.add(sent, sent + rtt, board_time(answered as u64));
        }
    }

    #[test]
    fn offset_and_drift() {
        let mut c = ClockSync::new();
        assert!(c.to_host(0).is_none());
        // An estimate every 2 s for 2 minutes, each off by up to ±300 µs.
        for k in 0..60u64 {
            let skew = [-300, 0, 300, 150, -150][k as usize % 5];
            sync_at(&mut c, 1_000_000 + k * 2_000_000, skew);
        }
        assert_eq!(c.uncertainty_us(), Some(400.0));
        let drift = c.drift_ppm().unwrap();
        assert!((drift - 20.0).abs() < 1.0, "drift {drift} ppm");
        // 10 s after the last answer the error stays well under the
        // uncertainty of a single estimate.
        let host = 130_000_000;
        let err = c.to_host(board_time(host)).unwrap() - host as i64;
        assert!(err.abs() <= 100, "error {err} µs");
    }

    #[test]
    fn no_drift_from_a_short_span() {
        let mut c = ClockSync::new();
        sync_at(&mut c, 1_000_000, 300);
        sync_at(&mut c, 3_000_000, -300);
        assert!(c.drift_ppm().is_none());
        assert!(c.ready());
    }
}
