//! `laptimer record`: reads every board, writes all levels to a CSV file and
//! shows them live in the terminal. For test flights: the recordings are what
//! we tune the pass detection on.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

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
        find_ports()?
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

    let n_ch = opts.config.channels.len();
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
            draw(&views, &opts.config, &path, origin.elapsed(), total);
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
    println!("\n{total} levels written to {}", path.display());
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

fn draw(views: &[BoardView], config: &ScanConfig, path: &std::path::Path, elapsed: Duration, total: u64) {
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
