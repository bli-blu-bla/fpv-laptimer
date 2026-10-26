//! One ESP32-C5 on USB: set it up, line up its clock, then read its levels in
//! a thread of its own and pass them on through a channel.

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serialport::SerialPort;

use super::frame::{FRAME_LEVEL_BATCH, Item, LevelBatch, Parser};
use crate::clock::ClockSync;

/// How the boards scan.
#[derive(Debug, Clone)]
pub struct ScanConfig {
    /// Name and 5.8 GHz frequency (MHz) of each channel, e.g. ("R1", 5658).
    pub channels: Vec<(String, u32)>,
    /// Manual RX gain index (0–60 or so); the firmware's default is 45.
    pub gain: u8,
    /// How often the board sends its levels, in ms (`LEVELBATCH`).
    pub batch_ms: u8,
}

/// One measurement, with its time on the common clock.
#[derive(Debug, Clone, Copy)]
pub struct Level {
    /// µs since the program started (all boards on the same clock).
    pub host_us: i64,
    /// The board's own clock, kept for later analysis.
    pub board_us: u64,
    pub channel: u8,
    pub rssi: u8,
    pub flags: u8,
}

/// Health of a board's link, sent about once a second.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct LinkStats {
    pub crc_errors: u64,
    /// Measurements the board could not send (its queue was full).
    pub lost: u64,
    /// Half the fastest recent `TIME?` round trip, µs.
    pub clock_uncertainty_us: f64,
    pub drift_ppm: Option<f64>,
}

/// What the board threads send to the main thread.
pub enum Event {
    Levels {
        board: usize,
        levels: Vec<Level>,
    },
    Stats {
        board: usize,
        stats: LinkStats,
    },
    /// The board thread stopped because of this error.
    Failed {
        board: usize,
        error: String,
    },
}

/// A board that is set up and being read.
pub struct Board {
    pub port_name: String,
    /// The board's `INFO` reply (firmware version etc.).
    pub info: String,
    /// The name saved on the board (`NAME?`), `None` without one or with
    /// firmware that doesn't know the command.
    pub name: Option<String>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Board {
    /// Opens the board at `port_name`, sets it up and starts its reader thread.
    /// `index` names the board in the events; `origin` is the common clock's zero.
    pub fn start(
        index: usize,
        port_name: &str,
        config: &ScanConfig,
        origin: Instant,
        events: Sender<Event>,
    ) -> Result<Board> {
        let mut link = Link::open(port_name, origin)?;
        let (info, name) = link.setup(config).with_context(|| format!("setting up {port_name}"))?;
        let stop = Arc::new(AtomicBool::new(false));
        // The thread gets its own handle to the stop flag; `Arc` counts the
        // owners, so the flag lives as long as either side needs it.
        let stop_thread = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            if let Err(e) = link.run(index, &stop_thread, &events) {
                let _ = events.send(Event::Failed {
                    board: index,
                    error: format!("{e:#}"),
                });
            }
            // Leave the board quiet for the next program that opens it.
            let _ = link.port.write_all(b"LEVELBATCH 0\nSCAN STOP\n");
        });
        Ok(Board {
            port_name: port_name.to_string(),
            info,
            name,
            stop,
            thread: Some(thread),
        })
    }

    /// Stops the reader thread and waits for it.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Board {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A serial port that looks like an ESP32-C5 board (its USB Serial/JTAG port).
#[derive(Debug, Clone)]
pub struct PortInfo {
    pub name: String,
    /// The USB serial number (the chip's MAC address, e.g. "10:BD:A3:C7:F7:FC"):
    /// stays the same when the board is plugged in elsewhere. The port name
    /// when it can't be read.
    pub id: String,
}

pub fn find_ports() -> Result<Vec<PortInfo>> {
    let by_id = linux_serial_ids();
    let mut ports: Vec<PortInfo> = serialport::available_ports()?
        .into_iter()
        // macOS: /dev/cu.usbmodem*, Linux: /dev/ttyACM* (USB Serial/JTAG).
        .filter(|p| p.port_name.starts_with("/dev/cu.usbmodem") || p.port_name.starts_with("/dev/ttyACM"))
        .map(|p| {
            let usb = match &p.port_type {
                serialport::SerialPortType::UsbPort(u) => u.serial_number.clone(),
                _ => None,
            };
            let id = usb
                .or_else(|| {
                    by_id
                        .iter()
                        .find(|(dev, _)| *dev == p.port_name)
                        .map(|(_, id)| id.clone())
                })
                .unwrap_or_else(|| p.port_name.clone());
            PortInfo { name: p.port_name, id }
        })
        .collect();
    ports.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(ports)
}

/// Linux without libudev: the serial numbers from the names in
/// /dev/serial/by-id, e.g. "usb-Espressif_USB_JTAG_serial_debug_unit_10:BD:A3:C7:F7:FC-if00"
/// → ("/dev/ttyACM0", "10:BD:A3:C7:F7:FC").
fn linux_serial_ids() -> Vec<(String, String)> {
    let Ok(dir) = std::fs::read_dir("/dev/serial/by-id") else {
        return Vec::new();
    };
    dir.filter_map(|e| {
        let e = e.ok()?;
        let name = e.file_name().to_string_lossy().to_string();
        let target = std::fs::canonicalize(e.path()).ok()?.to_string_lossy().to_string();
        let id = name.trim_end_matches("-if00").rsplit('_').next()?.to_string();
        Some((target, id))
    })
    .collect()
}

/// The serial connection, with what has been read but not used yet.
struct Link {
    port: Box<dyn SerialPort>,
    parser: Parser,
    items: VecDeque<Item>,
    origin: Instant,
    clock: ClockSync,
}

impl Link {
    fn open(port_name: &str, origin: Instant) -> Result<Link> {
        // The baud rate means nothing on USB; the timeout is how long a read
        // waits for data, which sets how quickly the thread notices `stop`.
        let port = serialport::new(port_name, 115_200)
            .timeout(Duration::from_millis(5))
            .open()
            .with_context(|| format!("opening {port_name} (is another program using it?)"))?;
        Ok(Link {
            port,
            parser: Parser::new(),
            items: VecDeque::new(),
            origin,
            clock: ClockSync::new(),
        })
    }

    /// µs since the program started.
    fn now_us(&self) -> u64 {
        self.origin.elapsed().as_micros() as u64
    }

    /// Reads whatever the port has (waits up to its timeout) into `items`.
    fn read_some(&mut self) -> Result<()> {
        let mut buf = [0u8; 4096];
        match self.port.read(&mut buf) {
            Ok(n) => {
                let mut out = Vec::new();
                self.parser.push(&buf[..n], &mut out);
                self.items.extend(out);
                Ok(())
            }
            Err(e) if e.kind() == ErrorKind::TimedOut => Ok(()),
            Err(e) => Err(e).context("reading the board"),
        }
    }

    /// Waits for the next text line, skipping frames.
    fn next_line(&mut self, timeout: Duration) -> Result<Option<String>> {
        let until = Instant::now() + timeout;
        loop {
            while let Some(item) = self.items.pop_front() {
                if let Item::Line(line) = item {
                    return Ok(Some(line));
                }
            }
            if Instant::now() >= until {
                return Ok(None);
            }
            self.read_some()?;
        }
    }

    /// Sends a command and returns its reply line; an `ERR …` reply is an error.
    ///
    /// The reply must start with `expect` ("OK" for most commands). Other
    /// lines are skipped: a late reply to an earlier command can still be on
    /// its way when we send the next one.
    fn command(&mut self, line: &str, expect: &str) -> Result<String> {
        self.items.clear(); // older replies don't belong to this command
        self.port.write_all(format!("{line}\n").as_bytes())?;
        let until = Instant::now() + Duration::from_secs(1);
        while let Some(reply) = self.next_line(until.saturating_duration_since(Instant::now()))? {
            if reply.starts_with("ERR") {
                bail!("{line}: {reply}");
            }
            if reply.starts_with(expect) {
                return Ok(reply);
            }
        }
        bail!("{line}: no reply")
    }

    /// Puts the board into the mode we need. Returns its `INFO` text and name.
    fn setup(&mut self, config: &ScanConfig) -> Result<(String, Option<String>)> {
        // The firmware boots in RotorHazard mode and says nothing until asked.
        // Its text commands work there too, so switch first.
        self.command("PROTOCOL MONITOR", "OK")?;
        self.command("SCAN STOP", "OK")?;
        let info = self.command("INFO", "INFO ")?;
        if !info.starts_with("INFO widscan58|") {
            bail!("not our firmware: {info}");
        }
        // Older firmware answers `ERR command`: then the board has no name.
        let name = self.command("NAME?", "NAME").ok().and_then(|r| parse_name(&r));
        // Line up the clocks while the board is quiet: the answers come
        // fastest then.
        while !self.clock.ready() {
            self.time_query()?;
        }
        let channels: Vec<String> = config
            .channels
            .iter()
            .map(|(name, mhz)| format!("{name}={mhz}"))
            .collect();
        for cmd in [
            format!("CHANNELS {}", channels.join(" ")),
            "MODE GROUP".to_string(),
            format!("GAIN MANUAL {}", config.gain),
            "METRICS RSSI".to_string(),
            // 4 × 64-point FFTs per capture: the fastest setting, as in
            // the firmware's RotorHazard mode.
            "FFT 64".to_string(),
            "CAPN 256".to_string(),
            "SPEC 0 256 40000000 MAX".to_string(), // no display spectrum
            format!("LEVELBATCH {}", config.batch_ms),
            "SCAN START".to_string(),
        ] {
            self.command(&cmd, "OK")?;
        }
        Ok((info.trim_start_matches("INFO ").to_string(), name))
    }

    /// Asks for the board time once and waits for the answer (setup only).
    fn time_query(&mut self) -> Result<()> {
        self.items.clear();
        let sent = self.now_us();
        self.port.write_all(b"TIME?\n")?;
        while let Some(line) = self.next_line(Duration::from_millis(200))? {
            if let Some(board_us) = parse_time(&line) {
                let received = self.now_us();
                self.clock.add(sent, received, board_us);
                return Ok(());
            }
        }
        bail!("no answer to TIME?")
    }

    /// The reader loop: runs until `stop` is set or the board fails.
    fn run(&mut self, index: usize, stop: &AtomicBool, events: &Sender<Event>) -> Result<()> {
        // While streaming, ask for the time every 200 ms; with 10 answers per
        // estimate that is a new estimate every 2 s.
        const QUERY_EVERY_US: u64 = 200_000;
        const ANSWER_TIMEOUT_US: u64 = 500_000;
        let mut pending: Option<u64> = None; // when the open TIME? was sent
        let mut next_query = self.now_us();
        let mut stats = LinkStats::default();
        let mut next_stats = self.now_us() + 1_000_000;
        let mut last_seq: Option<u32> = None;

        while !stop.load(Ordering::Relaxed) {
            let now = self.now_us();
            if pending.is_some_and(|sent| now - sent > ANSWER_TIMEOUT_US) {
                pending = None; // lost; ask again
            }
            if pending.is_none() && now >= next_query {
                self.port.write_all(b"TIME?\n")?;
                pending = Some(now);
                next_query = now + QUERY_EVERY_US;
            }

            self.read_some()?;
            let mut levels = Vec::new();
            while let Some(item) = self.items.pop_front() {
                match item {
                    Item::Line(line) => {
                        if let (Some(board_us), Some(sent)) = (parse_time(&line), pending) {
                            self.clock.add(sent, self.now_us(), board_us);
                            pending = None;
                        }
                    }
                    Item::Frame {
                        kind: FRAME_LEVEL_BATCH,
                        payload,
                    } => {
                        let Some(batch) = LevelBatch::parse(&payload) else {
                            continue;
                        };
                        stats.lost += batch.lost as u64;
                        if let Some(prev) = last_seq {
                            // Batches that never arrived (the USB link dropped
                            // them) show as a gap in the sequence numbers.
                            stats.lost += batch.seq.wrapping_sub(prev).saturating_sub(1) as u64;
                        }
                        last_seq = Some(batch.seq);
                        for r in batch.records {
                            // `ready()` was true after setup, so this is Some.
                            let host_us = self.clock.to_host(r.board_us).unwrap_or(0);
                            levels.push(Level {
                                host_us,
                                board_us: r.board_us,
                                channel: r.channel,
                                rssi: r.rssi,
                                flags: r.flags,
                            });
                        }
                    }
                    Item::Frame { .. } => {} // telemetry etc.: not needed
                }
            }
            if !levels.is_empty() && events.send(Event::Levels { board: index, levels }).is_err() {
                break; // nobody is listening any more
            }
            if now >= next_stats {
                stats.crc_errors = self.parser.crc_errors;
                stats.clock_uncertainty_us = self.clock.uncertainty_us().unwrap_or(0.0);
                stats.drift_ppm = self.clock.drift_ppm();
                let _ = events.send(Event::Stats { board: index, stats });
                next_stats = now + 1_000_000;
            }
        }
        Ok(())
    }
}

/// Saves `name` on the board at `port_name` ("" clears it). The board must
/// not be in use (stop the boards first).
pub fn set_name(port_name: &str, name: &str) -> Result<()> {
    let mut link = Link::open(port_name, Instant::now())?;
    link.command("PROTOCOL MONITOR", "OK")?;
    link.command("SCAN STOP", "OK")?;
    let cmd = if name.is_empty() { "NAME".to_string() } else { format!("NAME {name}") };
    match link.command(&cmd, "OK") {
        Ok(_) => Ok(()),
        Err(e) if format!("{e}").contains("ERR command") => {
            bail!("Die Firmware des Boards kennt keine Namen – bitte erst neu flashen.")
        }
        Err(e) if format!("{e}").contains("ERR name") => {
            bail!("Ungültiger Name: höchstens 32 Bytes, ohne „|“.")
        }
        Err(e) => Err(e),
    }
}

/// Checks a board name before sending it: what the firmware accepts.
pub fn check_name(name: &str) -> Result<()> {
    if name.len() > 32 {
        bail!("Der Name ist zu lang (höchstens 32 Bytes).");
    }
    if name.chars().any(|c| c.is_control() || c == '|') {
        bail!("Der Name darf kein „|“ und keine Steuerzeichen enthalten.");
    }
    Ok(())
}

/// "NAME Ecke links" → Some("Ecke links"); "NAME" → None.
fn parse_name(line: &str) -> Option<String> {
    let name = line.strip_prefix("NAME")?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// "TIME 123456" → 123456.
fn parse_time(line: &str) -> Option<u64> {
    line.strip_prefix("TIME ")?.trim().parse().ok()
}
