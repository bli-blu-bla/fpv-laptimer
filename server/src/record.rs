//! `laptimer record`: reads every board, writes all levels to a CSV file,
//! detects passes, and shows levels and laps live in the terminal. For test
//! flights: the recordings are what we tune the pass detection on
//! (`laptimer replay`). The passes go to `<recording>_passes.csv`.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::detect::{ChannelStatus, DetectConfig, Detector, Event as DetectEvent, Pass};
use crate::esp::board::{Board, Event, LinkStats, ScanConfig, find_ports};
use crate::esp::frame::rssi_dbfs;

/// Redraw the terminal this often.
const DISPLAY_EVERY: Duration = Duration::from_millis(200);
/// The level bar spans this range, dBFS.
const BAR_MIN_DB: f32 = -64.0;
const BAR_MAX_DB: f32 = -14.0;
const BAR_WIDTH: usize = 24;

pub struct RecordOptions {
    pub ports: Vec<String>,
    pub config: ScanConfig,
    pub out: Option<PathBuf>,
    pub seconds: Option<u64>,
    pub detect: DetectConfig,
}

/// What the display needs per board.
#[derive(Default, Clone)]
struct BoardView {
    /// Highest level per channel since the last redraw (None: no data).
    max_rssi: Vec<Option<u8>>,
    /// Levels received since the last redraw.
    count: u64,
    rate: f64,
    stats: LinkStats,
    failed: Option<String>,
}

pub fn run(opts: RecordOptions) -> Result<()> {
    let ports = if opts.ports.is_empty() {
        find_ports()?.into_iter().map(|p| p.name).collect()
    } else {
        opts.ports.clone()
    };
    if ports.is_empty() {
        bail!("no board found (plug one in, or name its port with --port)");
    }

    // Ctrl-C sets this flag instead of killing the program, so the file is
    // complete and the boards are told to stop.
    let stop = Arc::new(AtomicBool::new(false));
    let stop_handler = Arc::clone(&stop);
    ctrlc::set_handler(move || stop_handler.store(true, Ordering::Relaxed))?;

    let origin = Instant::now();
    let (tx, rx) = mpsc::channel();
    let mut boards = Vec::new();
    for (i, port) in ports.iter().enumerate() {
        eprintln!("Board {i}: {port} ...");
        // Each board gets a clone of the sending end of the channel.
        let board = Board::start(i, port, &opts.config, origin, tx.clone())?;
        eprintln!("Board {i}: {}", board.info);
        boards.push(board);
    }
    drop(tx); // only the boards send now

    let path = match opts.out {
        Some(p) => p,
        None => {
            fs::create_dir_all("recordings")?;
            PathBuf::from(format!(
                "recordings/{}.csv",
                chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
            ))
        }
    };
    let mut out = BufWriter::new(File::create(&path).with_context(|| format!("creating {}", path.display()))?);
    write_header(&mut out, &boards, &opts.config)?;
    let passes_path = PathBuf::from(path.to_string_lossy().trim_end_matches(".csv").to_string() + "_passes.csv");
    let mut passes_out = BufWriter::new(File::create(&passes_path)?);
    writeln!(passes_out, "event,channel,time_us,peak_db")?;

    let n_ch = opts.config.channels.len();
    let mut detector = Detector::new(n_ch, opts.detect);
    // The passes of each channel so far.
    let mut passes: Vec<Vec<Pass>> = vec![Vec::new(); n_ch];
    let mut views = vec![
        BoardView {
            max_rssi: vec![None; n_ch],
            ..Default::default()
        };
        boards.len()
    ];
    let mut next_draw = Instant::now() + DISPLAY_EVERY;
    let mut last_draw = Instant::now();
    let mut total: u64 = 0;
    let end = opts.seconds.map(|s| Instant::now() + Duration::from_secs(s));

    while !stop.load(Ordering::Relaxed) && end.is_none_or(|e| Instant::now() < e) {
        // Wait for the next event, but not past the next redraw.
        let wait = next_draw.saturating_duration_since(Instant::now());
        match rx.recv_timeout(wait) {
            Ok(Event::Levels { board, levels }) => {
                let v = &mut views[board];
                for l in &levels {
                    writeln!(
                        out,
                        "{board},{},{},{},{},{}",
                        l.channel, l.host_us, l.board_us, l.rssi, l.flags
                    )?;
                    if let Some(m) = v.max_rssi.get_mut(l.channel as usize) {
                        *m = Some(m.map_or(l.rssi, |old| old.max(l.rssi)));
                    }
                }
                v.count += levels.len() as u64;
                total += levels.len() as u64;
                detector.push(board, &levels);
                for e in detector.advance() {
                    match e {
                        DetectEvent::Pass(p) => {
                            writeln!(passes_out, "pass,{},{},{:.2}", p.channel, p.time_us, p.peak_db)?;
                            passes[p.channel as usize].push(p);
                            print!("\x07"); // the terminal's bell
                        }
                        DetectEvent::Retract(p) => {
                            writeln!(passes_out, "retract,{},{},{:.2}", p.channel, p.time_us, p.peak_db)?;
                            passes[p.channel as usize].retain(|q| *q != p);
                        }
                    }
                    passes_out.flush()?;
                }
            }
            Ok(Event::Stats { board, stats }) => views[board].stats = stats,
            Ok(Event::Failed { board, error }) => views[board].failed = Some(error),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break, // every board stopped
        }
        if Instant::now() >= next_draw {
            let dt = last_draw.elapsed().as_secs_f64();
            for v in &mut views {
                v.rate = v.count as f64 / dt;
                v.count = 0;
            }
            let status: Vec<ChannelStatus> = (0..n_ch).map(|c| detector.status(c)).collect();
            draw(&views, &opts.config, &path, origin.elapsed(), total, &status, &passes);
            for v in &mut views {
                v.max_rssi.iter_mut().for_each(|m| *m = None);
            }
            last_draw = Instant::now();
            next_draw = last_draw + DISPLAY_EVERY;
        }
    }

    for b in &mut boards {
        b.stop();
    }
    out.flush()?;
    println!(
        "\n{total} levels written to {}, passes to {}",
        path.display(),
        passes_path.display()
    );
    Ok(())
}

/// Comment lines (starting with '#') describing the recording, then the column names.
fn write_header(out: &mut impl Write, boards: &[Board], config: &ScanConfig) -> Result<()> {
    writeln!(out, "# laptimer recording, {}", chrono::Local::now().to_rfc3339())?;
    for (i, b) in boards.iter().enumerate() {
        writeln!(out, "# board {i}: {} {}", b.port_name, b.info)?;
    }
    let channels: Vec<String> = config
        .channels
        .iter()
        .enumerate()
        .map(|(i, (name, mhz))| format!("{i}={name}@{mhz}"))
        .collect();
    writeln!(out, "# channels: {}", channels.join(" "))?;
    writeln!(out, "# gain: {}, batch: {} ms", config.gain, config.batch_ms)?;
    writeln!(
        out,
        "# host_us: µs since the program started (all boards on one clock); board_us: the board's own clock"
    )?;
    writeln!(
        out,
        "# rssi: level code, dBFS = rssi / 4 - 63.75; flags: 2 = ADC clipped"
    )?;
    writeln!(out, "board,channel,host_us,board_us,rssi,flags")?;
    Ok(())
}

/// One data line `board,channel,host_us,board_us,rssi,flags` as six numbers;
/// `None` for anything else (comments, the column names, a line cut off at
/// the end). By hand instead of `split(',')` + `parse()`: a long session has
/// millions of lines, and this is about four times faster.
pub fn parse_row(line: &str) -> Option<[i64; 6]> {
    let mut f = [0i64; 6];
    let mut k = 0; // which field
    let mut digits = 0; // digits in the current field
    let mut neg = false;
    for &c in line.trim_end().as_bytes() {
        match c {
            b'0'..=b'9' => {
                f[k] = f[k].checked_mul(10)?.checked_add((c - b'0') as i64)?;
                digits += 1;
            }
            b'-' if digits == 0 && !neg => neg = true,
            b',' if digits > 0 && k < 5 => {
                if neg {
                    f[k] = -f[k];
                }
                (k, digits, neg) = (k + 1, 0, false);
            }
            _ => return None,
        }
    }
    if k != 5 || digits == 0 {
        return None;
    }
    if neg {
        f[5] = -f[5];
    }
    Some(f)
}

fn draw(
    views: &[BoardView],
    config: &ScanConfig,
    path: &std::path::Path,
    elapsed: Duration,
    total: u64,
    status: &[ChannelStatus],
    passes: &[Vec<Pass>],
) {
    let mut s = String::new();
    // "\x1b[H\x1b[2J": cursor home, clear screen (ANSI escape codes).
    s.push_str("\x1b[H\x1b[2J");
    s.push_str(&format!(
        "Recording {}  {:.0} s  {} levels   (Ctrl-C to stop)\n\n",
        path.display(),
        elapsed.as_secs_f64(),
        total
    ));
    s.push_str("        ");
    for i in 0..views.len() {
        s.push_str(&format!("Board {i}{:width$}", "", width = BAR_WIDTH + 4));
    }
    s.push('\n');
    for (ch, (name, _)) in config.channels.iter().enumerate() {
        s.push_str(&format!("{name:<6}  "));
        for v in views {
            match v.max_rssi.get(ch).copied().flatten() {
                Some(code) => {
                    let db = rssi_dbfs(code);
                    let fill = ((db - BAR_MIN_DB) / (BAR_MAX_DB - BAR_MIN_DB) * BAR_WIDTH as f32)
                        .clamp(0.0, BAR_WIDTH as f32) as usize;
                    s.push_str(&format!(
                        "{:>6.1} {}{} ",
                        db,
                        "#".repeat(fill),
                        ".".repeat(BAR_WIDTH - fill)
                    ));
                }
                None => s.push_str(&format!("{:>6} {} ", "-", " ".repeat(BAR_WIDTH))),
            }
        }
        s.push('\n');
    }
    s.push('\n');
    // Laps per channel: count, last, best, the last few.
    for (ch, (name, _)) in config.channels.iter().enumerate() {
        let st = status[ch];
        let laps: Vec<f64> = passes[ch]
            .windows(2)
            .map(|w| (w[1].time_us - w[0].time_us) as f64 / 1e6)
            .collect();
        let threshold = match (st.level_db, st.enter_db) {
            (Some(l), Some(e)) => format!("{l:>6.1}  enter {e:>6.1} exit {:>6.1}", st.exit_db.unwrap_or(e)),
            (Some(l), None) => format!("{l:>6.1}  (learning the levels)  "),
            _ => format!("{:>6}  {:24}", "-", ""),
        };
        let best = laps.iter().cloned().fold(f64::INFINITY, f64::min);
        let recent: Vec<String> = laps.iter().rev().take(5).map(|l| format!("{l:.2}")).collect();
        s.push_str(&format!(
            "{name:<6} {threshold} {} passes {:>3}  best {}  last: {}\n",
            if st.in_pass { "GATE" } else { "    " },
            passes[ch].len(),
            if best.is_finite() {
                format!("{best:>6.2}")
            } else {
                format!("{:>6}", "-")
            },
            recent.join("  ")
        ));
    }
    s.push('\n');
    for (i, v) in views.iter().enumerate() {
        let drift = v.stats.drift_ppm.map_or("?".to_string(), |d| format!("{d:+.1} ppm"));
        s.push_str(&format!(
            "Board {i}: {:.0} levels/s, lost {}, crc errors {}, clock ±{:.2} ms, drift {}\n",
            v.rate,
            v.stats.lost,
            v.stats.crc_errors,
            v.stats.clock_uncertainty_us / 1000.0,
            drift
        ));
        if let Some(e) = &v.failed {
            s.push_str(&format!("  FAILED: {e}\n"));
        }
    }
    print!("{s}");
    let _ = std::io::stdout().flush();
}

#[cfg(test)]
mod tests {
    use super::parse_row;

    #[test]
    fn rows() {
        assert_eq!(parse_row("1,2,-30,4000,150,0"), Some([1, 2, -30, 4000, 150, 0]));
        assert_eq!(parse_row("0,3,123456789,987654321,99,2\r\n"), Some([0, 3, 123456789, 987654321, 99, 2]));
        assert_eq!(parse_row("board,channel,host_us,board_us,rssi,flags"), None);
        assert_eq!(parse_row("# channels: 0=R1@5658"), None);
        assert_eq!(parse_row("1,2,3,4,5"), None); // cut off
        assert_eq!(parse_row("1,2,3,4,5,"), None);
        assert_eq!(parse_row("1,2,3,4,5,6,7"), None);
        assert_eq!(parse_row("1,,3,4,5,6"), None);
        assert_eq!(parse_row("1,2-,3,4,5,6"), None);
        assert_eq!(parse_row(""), None);
    }
}
