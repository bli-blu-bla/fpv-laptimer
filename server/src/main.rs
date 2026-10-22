//! FPV lap timer. For now: `laptimer ports` and `laptimer record`.

mod clock;
mod esp;
mod record;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use esp::board::{ScanConfig, find_ports};

/// The usual channels at our meetup.
const DEFAULT_CHANNELS: &str = "R1=5658,R3=5732,R6=5843,R8=5917";

#[derive(Parser)]
#[command(version, about = "FPV lap timer with ESP32-C5 boards")]
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
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Ports => {
            for p in find_ports()? {
                println!("{p}");
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
            })
        }
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
