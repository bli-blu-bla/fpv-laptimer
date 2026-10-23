//! FPV lap timer: `laptimer serve` (the server), `record`/`replay` (test
//! flights), `ports`.

mod api;
mod auth;
mod clock;
mod config;
mod core;
mod data;
mod db;
mod detect;
mod esp;
mod events;
mod hooks;
mod laps;
mod record;
mod replay;
mod server;
mod sound;
mod source;
mod stats;
mod trace;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use esp::board::{ScanConfig, find_ports};

/// The version: the release's (CI), else `git describe` (`build.rs`).
pub const VERSION: &str = env!("LAPTIMER_VERSION");

/// The usual channels at our meetup.
const DEFAULT_CHANNELS: &str = "R1=5658,R3=5732,R6=5843,R8=5917";

#[derive(Parser)]
#[command(version = VERSION, about = "FPV lap timer with ESP32-C5 boards")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the serial ports that look like boards.
    Ports,
    /// Record the levels of all boards to a CSV file, with a live view.
    Record {
        /// Serial port of a board (repeat for several). Default: all found.
        #[arg(long = "port")]
        ports: Vec<String>,
        /// Channels as NAME=MHZ, comma separated.
        #[arg(long, default_value = DEFAULT_CHANNELS)]
        channels: String,
        /// Manual RX gain index.
        #[arg(long, default_value_t = 45)]
        gain: u8,
        /// Send interval of the boards' level batches, ms (1–100).
        #[arg(long, default_value_t = 10)]
        batch_ms: u8,
        /// Output file. Default: recordings/<date_time>.csv
        #[arg(long)]
        out: Option<PathBuf>,
        /// Stop after this many seconds (default: Ctrl-C).
        #[arg(long)]
        seconds: Option<u64>,
        /// Minimum lap, seconds.
        #[arg(long, default_value_t = 3.0)]
        min_lap: f64,
    },
    /// Run the lap timer: web page, database, announcements.
    ///
    /// Without options, everything comes from ~/.laptimer/config.toml (made
    /// with a random password if it is missing).
    Serve {
        /// The installation's settings. Default: ~/.laptimer/config.toml
        #[arg(long)]
        config: Option<PathBuf>,
        /// Web port (default 5800).
        #[arg(long)]
        port: Option<u16>,
        /// Database and session levels. Default: ~/.laptimer/data
        #[arg(long)]
        data: Option<PathBuf>,
        /// The built web page. Default: web/ next to bin/, else ../frontend/dist
        #[arg(long)]
        web: Option<PathBuf>,
        /// The announcement MP3s. Default: sounds/ next to bin/, else ../sounds
        #[arg(long)]
        sounds: Option<PathBuf>,
        /// Play this recording (in a loop) instead of reading boards.
        #[arg(long)]
        replay: Option<PathBuf>,
        /// The folder of the scripts hooks can run on events. Default: <data>/hooks
        #[arg(long)]
        hooks: Option<PathBuf>,
    },
    /// Run a recording through the pass detection.
    Replay {
        csv: PathBuf,
        /// Only this board's levels (default: all boards combined).
        #[arg(long)]
        board: Option<usize>,
        /// Minimum lap, seconds.
        #[arg(long, default_value_t = 3.0)]
        min_lap: f64,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Ports => {
            for p in find_ports()? {
                println!("{}  {}", p.name, p.id);
            }
            Ok(())
        }
        Command::Record {
            ports,
            channels,
            gain,
            batch_ms,
            out,
            seconds,
            min_lap,
        } => {
            if !(1..=100).contains(&batch_ms) {
                bail!("--batch-ms must be 1–100");
            }
            let config = ScanConfig {
                channels: parse_channels(&channels)?,
                gain,
                batch_ms,
            };
            record::run(record::RecordOptions {
                ports,
                config,
                out,
                seconds,
                detect: detect::DetectConfig {
                    min_lap_us: (min_lap * 1e6) as i64,
                },
            })
        }
        Command::Serve {
            config,
            port,
            data,
            web,
            sounds,
            replay,
            hooks,
        } => {
            let home = config::home_dir()?;
            let cfg = match &config {
                Some(path) => config::load(path, true)?,
                None => config::load(&home.join("config.toml"), false)?,
            };
            // Command line, then config.toml, then the default.
            let data = data.or(cfg.data).unwrap_or_else(|| home.join("data"));
            let hooks_dir = hooks.or(cfg.hooks).unwrap_or_else(|| data.join("hooks"));
            server::run(server::ServeOptions {
                port: port.or(cfg.port).unwrap_or(5800),
                hooks_dir,
                data_dir: data,
                web_dir: web.or(cfg.web).unwrap_or_else(|| installed("web", "../frontend/dist")),
                sounds_dir: sounds
                    .or(cfg.sounds)
                    .unwrap_or_else(|| installed("sounds", "../sounds")),
                replay,
                auth: cfg.auth,
            })
        }
        Command::Replay { csv, board, min_lap } => {
            let config = detect::DetectConfig {
                min_lap_us: (min_lap * 1e6) as i64,
            };
            replay::run(&csv, board, config)
        }
    }
}

/// A folder of the installed release (`<release>/bin/laptimer` →
/// `<release>/<name>`), or `dev` (relative to `server/`, for `cargo run`).
fn installed(name: &str, dev: &str) -> PathBuf {
    let release = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.join(name)));
    match release {
        Some(dir) if dir.is_dir() => dir,
        _ => PathBuf::from(dev),
    }
}

/// "R1=5658,R3=5732" → [("R1", 5658), ("R3", 5732)].
fn parse_channels(s: &str) -> Result<Vec<(String, u32)>> {
    let mut out = Vec::new();
    for part in s.split(',') {
        let (name, mhz) = part
            .split_once('=')
            .with_context(|| format!("channel '{part}': expected NAME=MHZ"))?;
        let mhz: u32 = mhz
            .trim()
            .parse()
            .with_context(|| format!("channel '{part}': bad frequency"))?;
        out.push((name.trim().to_string(), mhz));
    }
    if out.is_empty() || out.len() > 8 {
        bail!("1 to 8 channels");
    }
    Ok(out)
}
