//! `laptimer replay`: runs a recording through the pass detection, as if the
//! boards were sending it now. For checking changes to the detection.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};

use crate::detect::{DetectConfig, DetectMode, Detector, Event, Pass, TracePoint};
use crate::esp::board::Level;
use crate::esp::frame::rssi_dbfs;

/// What `laptimer replay` was asked for besides the file.
pub struct Options {
    /// Only this board's levels.
    pub only_board: Option<usize>,
    /// The boards of this gate, from the file's "# roles:" line: "finish",
    /// "arming", or "all" (every board, whatever its role). Files without
    /// roles: every board.
    pub gate: String,
    /// `Auto` or `AutoRelative` for every channel (fixed thresholds are not
    /// offered here).
    pub mode: DetectMode,
    /// Per pass each board's peak and its rise above its own course.
    pub detail: bool,
    pub config: DetectConfig,
}

pub fn run(path: &Path, opts: &Options) -> Result<()> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut names: Vec<String> = Vec::new();
    let mut det: Option<Detector> = None;
    let mut events: Vec<Event> = Vec::new();
    // Boards on another gate than the one asked for.
    let mut skip: Vec<usize> = Vec::new();

    for (n, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if let Some(rest) = line.strip_prefix("# channels:") {
            // "0=R1@5658 1=R3@5732 ..." → ["R1", "R3", ...]
            names = rest
                .split_whitespace()
                .filter_map(|c| {
                    c.split_once('=')
                        .map(|(_, v)| v.split('@').next().unwrap_or(v).to_string())
                })
                .collect();
            continue;
        }
        if let Some(rest) = line.strip_prefix("# roles:") {
            // "0=finish 1=arming 2=off"
            if opts.gate != "all" {
                skip = rest
                    .split_whitespace()
                    .filter_map(|r| r.split_once('='))
                    .filter(|(_, role)| *role != opts.gate)
                    .filter_map(|(b, _)| b.parse().ok())
                    .collect();
            }
            continue;
        }
        if line.starts_with('#') || line.starts_with("board") || line.is_empty() {
            continue;
        }
        let f: Vec<i64> = line
            .split(',')
            .map(|x| x.parse())
            .collect::<Result<_, _>>()
            .with_context(|| format!("line {}", n + 1))?;
        let board = f[0] as usize;
        if opts.only_board.is_some_and(|b| b != board) || skip.contains(&board) {
            continue;
        }
        // The detector is made once we know how many channels there are.
        let d = det.get_or_insert_with(|| {
            let n_channels = names.len().max(1);
            let mut d = if opts.detail {
                Detector::with_trace(n_channels, opts.config)
            } else {
                Detector::new(n_channels, opts.config)
            };
            d.set_relative(&vec![opts.mode == DetectMode::AutoRelative; n_channels]);
            d
        });
        let level = Level {
            host_us: f[2],
            board_us: f[3] as u64,
            channel: f[1] as u8,
            rssi: f[4] as u8,
            flags: f[5] as u8,
        };
        d.push(board, &[level]);
        // Live, the boards send a batch every 10 ms; advancing every line
        // gives the same result, just slower.
        if n % 16 == 0 {
            events.extend(d.advance());
        }
    }
    let mut trace: Vec<Vec<TracePoint>> = Vec::new();
    if let Some(d) = &mut det {
        events.extend(d.flush());
        trace = d.take_trace();
    }
    let mut passes: Vec<Pass> = Vec::new();
    for e in &events {
        match e {
            Event::Pass(p) => passes.push(*p),
            Event::Retract(p) => {
                println!(
                    "taken back: {} {:.2}s ({:.0} dB)",
                    names.get(p.channel as usize).map_or("?", |s| s),
                    p.time_us as f64 / 1e6,
                    p.peak_db
                );
                passes.retain(|q| q != p);
            }
        }
    }

    for (c, name) in names.iter().enumerate() {
        let mine: Vec<&Pass> = passes.iter().filter(|p| p.channel as usize == c).collect();
        if mine.is_empty() {
            continue;
        }
        let times: Vec<String> = mine
            .iter()
            .map(|p| format!("{:.2}s({:.0}dB)", p.time_us as f64 / 1e6, p.peak_db))
            .collect();
        println!("{name}: {} passes  {}", mine.len(), times.join("  "));
        let laps: Vec<String> = mine
            .windows(2)
            .map(|w| format!("{:.2}", (w[1].time_us - w[0].time_us) as f64 / 1e6))
            .collect();
        if !laps.is_empty() {
            println!("    laps: {}", laps.join(" "));
        }
        if let Some(slots) = trace.get(c) {
            for p in mine {
                println!("    {name} {:.2}s {:.1}dB |{}", p.time_us as f64 / 1e6, p.peak_db, board_detail(slots, p.time_us));
            }
        }
    }
    Ok(())
}

/// `--detail`: for the pass at `time_us`, per board " B<n> <peak> (+<rise>)":
/// its highest envelope within ±0.5 s, and how far that is above its own
/// course (the median of its levels above -46 dBFS in the 3 minutes before,
/// as the detector learns it; "?" with less than 5 s of them).
fn board_detail(slots: &[TracePoint], time_us: i64) -> String {
    let at = slots.partition_point(|p| p.time_us < time_us);
    // The slots are 10 ms apart: ±50 of them is ±0.5 s, 18 000 are 3 minutes.
    let near = &slots[at.saturating_sub(50)..(at + 50).min(slots.len())];
    let before = &slots[at.saturating_sub(18_000)..at];
    let on = ((-46.0f32 + 63.75) * 4.0) as u8;
    let mut out = String::new();
    for b in 0..crate::detect::TRACE_BOARDS {
        let peak = near.iter().map(|p| p.boards[b]).max().unwrap_or(0);
        if peak == 0 {
            continue; // this board has no data here
        }
        let mut levels: Vec<u8> = before.iter().map(|p| p.boards[b]).filter(|&l| l >= on).collect();
        levels.sort_unstable();
        let rise = if levels.len() >= 500 {
            format!("{:+.1}", (peak as f32 - levels[levels.len() / 2] as f32) / 4.0)
        } else {
            "?".to_string()
        };
        out += &format!(" B{b} {:.1} ({rise})", rssi_dbfs(peak));
    }
    out
}
