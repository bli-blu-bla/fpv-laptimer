//! `laptimer replay`: runs a recording through the pass detection, as if the
//! boards were sending it now. For checking changes to the detection.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{Context, Result};

use crate::detect::{DetectConfig, Detector, Event, Pass};
use crate::esp::board::Level;

pub fn run(path: &Path, only_board: Option<usize>, config: DetectConfig) -> Result<()> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut names: Vec<String> = Vec::new();
    let mut det: Option<Detector> = None;
    let mut events: Vec<Event> = Vec::new();

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
        if line.starts_with('#') || line.starts_with("board") || line.is_empty() {
            continue;
        }
        let f: Vec<i64> = line
            .split(',')
            .map(|x| x.parse())
            .collect::<Result<_, _>>()
            .with_context(|| format!("line {}", n + 1))?;
        let board = f[0] as usize;
        if only_board.is_some_and(|b| b != board) {
            continue;
        }
        // The detector is made once we know how many channels there are.
        let d = det.get_or_insert_with(|| Detector::new(names.len().max(1), config));
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
    if let Some(d) = &mut det {
        events.extend(d.flush());
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
    }
    Ok(())
}
