//! Writes the firmware to a board on USB (the crate `espflash`), for the web
//! page's "Firmware aufspielen". The files come from `<web>/firmware/`
//! (`tools/stage_firmware.py`), the same ones the page's own flasher
//! (Web Serial) uses.
//!
//! espflash knows the ESP32-C5 but not its watchdog registers, so two steps
//! are done here by hand, as in widscan58's web flasher: switch the RTC
//! watchdogs off before writing (USB Serial/JTAG does not reset them, and a
//! watchdog the bootloader armed can reset the chip mid-flash), and restart
//! through the watchdog afterwards (a reset over USB leaves the C5 in
//! download mode).

use std::path::Path;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use espflash::connection::{Connection, ResetAfterOperation, ResetBeforeOperation};
use espflash::flasher::Flasher;
use espflash::image_format::Segment;
use espflash::target::{Chip, ProgressCallbacks};
use serde::{Deserialize, Serialize};
use serialport::UsbPortInfo;

/// `manifest.json`: ESP Web Tools' format plus size and sha256 per part.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Manifest {
    pub name: String,
    /// The firmware's `git describe`, as its `INFO` reports it.
    pub version: String,
    pub date: Option<String>,
    pub builds: Vec<Build>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Build {
    /// "ESP32-C5"
    #[serde(rename = "chipFamily")]
    pub chip_family: String,
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Part {
    /// Relative to the firmware folder, e.g. "esp32c5/firmware.bin".
    pub path: String,
    pub offset: u32,
    pub size: usize,
    pub sha256: String,
}

pub fn load_manifest(dir: &Path) -> Result<Manifest> {
    let path = dir.join("manifest.json");
    let text = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "Keine Firmware bereitgestellt ({} fehlt; tools/stage_firmware.py ausführen).",
            path.display()
        )
    })?;
    serde_json::from_str(&text).with_context(|| format!("{} ist kaputt", path.display()))
}

/// The ESP32-C5's LP watchdog registers (esptool's targets/esp32c5.py).
const WDT_CONFIG0: u32 = 0x600b_1c00;
const WDT_CONFIG1: u32 = 0x600b_1c04;
const WDT_PROTECT: u32 = 0x600b_1c18;
const WDT_KEY: u32 = 0x50d8_3aa1;
const SWD_CONFIG: u32 = 0x600b_1c1c;
const SWD_PROTECT: u32 = 0x600b_1c20;
const SWD_KEY: u32 = 0x50d8_3aa1;
const SWD_AUTO_FEED: u32 = 1 << 18;

/// Espressif's USB Serial/JTAG. espflash picks its reset sequence by the
/// product id; on Linux without libudev the port doesn't tell us, but
/// `find_ports` only lists such ports anyway.
const ESPRESSIF_VID: u16 = 0x303a;
const USB_SERIAL_JTAG_PID: u16 = 0x1001;

/// Flashes the board at `port_name` (nobody else may have it open).
/// `report(stage, progress)` is called along the way, progress 0–1 while
/// writing. Returns the version written.
pub fn flash(port_name: &str, dir: &Path, report: &mut dyn FnMut(&str, Option<f64>)) -> Result<String> {
    let manifest = load_manifest(dir)?;
    let build = manifest
        .builds
        .iter()
        .find(|b| b.chip_family == "ESP32-C5")
        .context("Die bereitgestellte Firmware ist nicht für den ESP32-C5.")?;
    let mut files = Vec::new();
    for p in &build.parts {
        let data = std::fs::read(dir.join(&p.path)).with_context(|| format!("{} fehlt", p.path))?;
        if data.len() != p.size {
            bail!("{}: Größe passt nicht zum manifest.json", p.path);
        }
        files.push((p.offset, data));
    }

    report("Board anhalten …", None);
    quiet(port_name);

    report("Mit dem Bootloader verbinden …", None);
    let mut flasher = connect(port_name)?;
    disable_watchdogs(flasher.connection())?;

    report(&format!("Schreibe {} …", manifest.version), Some(0.0));
    let segments: Vec<Segment> = files.iter().map(|(offset, data)| Segment::new(*offset, data)).collect();
    let mut progress = Progress {
        report,
        sizes: files.iter().map(|(_, d)| d.len()).collect(),
        segment: 0,
        segment_steps: 0,
        stage: format!("Schreibe {} …", manifest.version),
    };
    flasher
        .write_bins_to_flash(&segments, &mut progress)
        .context("Schreiben fehlgeschlagen")?;

    report("Board neu starten …", Some(1.0));
    watchdog_reset(flasher.connection());
    drop(flasher); // closes the port
    Ok(manifest.version)
}

/// Stops a running scan, so the reset doesn't race a stream of frames.
fn quiet(port_name: &str) {
    if let Ok(mut port) = serialport::new(port_name, 115_200)
        .timeout(Duration::from_millis(100))
        .open()
    {
        let _ = port.write_all(b"\nSCAN STOP\nPROTOCOL RH\n");
        thread::sleep(Duration::from_millis(300));
    }
}

/// Into download mode and connected to the flasher stub: first by a reset
/// over USB, then (for a board put into download mode by hand with BOOT +
/// RESET, whose reset may not work) without one.
fn connect(port_name: &str) -> Result<Flasher> {
    let mut last_error = None;
    for before in [ResetBeforeOperation::UsbReset, ResetBeforeOperation::NoReset] {
        let port = serialport::new(port_name, 115_200)
            .timeout(Duration::from_secs(3))
            .open_native()
            .with_context(|| format!("{port_name} öffnen (benutzt ein anderes Programm den Port?)"))?;
        let info = UsbPortInfo {
            vid: ESPRESSIF_VID,
            pid: USB_SERIAL_JTAG_PID,
            serial_number: None,
            manufacturer: None,
            product: None,
        };
        // NoResetNoStub: espflash leaves the restart to us (watchdog_reset).
        let connection = Connection::new(port, info, ResetAfterOperation::NoResetNoStub, before, 115_200);
        // use_stub, verify (MD5 after writing), no skipping, the C5 only.
        match Flasher::connect(connection, true, true, false, Some(Chip::Esp32c5), None) {
            Ok(f) => return Ok(f),
            Err(e) => last_error = Some(e),
        }
    }
    Err(anyhow::anyhow!(
        "Kein Kontakt zum Bootloader ({}). BOOT halten, RESET drücken, BOOT loslassen und noch einmal versuchen.",
        last_error.map(|e| e.to_string()).unwrap_or_default()
    ))
}

fn disable_watchdogs(c: &mut Connection) -> Result<()> {
    c.write_reg(WDT_PROTECT, WDT_KEY, None)?;
    c.write_reg(WDT_CONFIG0, 0, None)?;
    c.write_reg(WDT_PROTECT, 0, None)?;
    c.write_reg(SWD_PROTECT, SWD_KEY, None)?;
    let swd = c.read_reg(SWD_CONFIG)?;
    c.write_reg(SWD_CONFIG, swd | SWD_AUTO_FEED, None)?;
    c.write_reg(SWD_PROTECT, 0, None)?;
    Ok(())
}

/// Restart into the new firmware: the RTC watchdog resets the whole chip
/// (esptool's `--after watchdog-reset`).
fn watchdog_reset(c: &mut Connection) {
    let _ = c.write_reg(WDT_PROTECT, WDT_KEY, None);
    let _ = c.write_reg(WDT_CONFIG1, 2000, None);
    let _ = c.write_reg(WDT_CONFIG0, (1 << 31) | (5 << 28) | (1 << 8) | 2, None);
    // The reset may come before this reply does.
    let _ = c.write_reg(WDT_PROTECT, 0, None);
    thread::sleep(Duration::from_millis(500));
}

/// espflash reports per segment (bootloader, partition table, app), in
/// steps of its own; this turns it into one fraction over all bytes.
struct Progress<'a> {
    report: &'a mut dyn FnMut(&str, Option<f64>),
    /// Bytes per segment.
    sizes: Vec<usize>,
    /// The segment being written.
    segment: usize,
    /// Its number of steps (from `init`).
    segment_steps: usize,
    stage: String,
}

impl ProgressCallbacks for Progress<'_> {
    fn init(&mut self, _addr: u32, total: usize) {
        self.segment_steps = total;
    }

    fn update(&mut self, current: usize) {
        let total: usize = self.sizes.iter().sum();
        let before: usize = self.sizes[..self.segment.min(self.sizes.len())].iter().sum();
        let this = self.sizes.get(self.segment).copied().unwrap_or(0) as f64;
        let part = current as f64 / self.segment_steps.max(1) as f64;
        let f = (before as f64 + this * part) / total.max(1) as f64;
        (self.report)(&self.stage, Some(f.min(1.0)));
    }

    fn verifying(&mut self) {}

    fn finish(&mut self, _skipped: bool) {
        self.segment += 1;
    }
}
