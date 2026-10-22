//! Announcements: plays `<name>.mp3` from the sounds folder, one after the
//! other, in a thread of its own (so the timing code never waits for audio).
//! The player is `afplay` on macOS and `mpg123` elsewhere (the Pi).
//!
//! Sounds used: `rennen_beginnt`, `3`, `2`, `1`, `start`, `runde`,
//! `pilot1_fertig` … `pilot8_fertig`, `rennen_beendet`, `bestzeit`.
//! A missing file is skipped, so any of them can be left out.
//!
//! Texts with names in them ("Anna ist in Runde 3") are spoken by speech
//! synthesis: `say` on macOS, on the Pi `pico2wave` + `aplay` (sounds better)
//! or else `espeak-ng`. None installed: the text is skipped.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;

/// One entry in the queue.
enum Item {
    /// `<name>.mp3` from the sounds folder.
    File(String),
    /// Text for speech synthesis.
    Speak(String),
}

pub struct Sounds {
    tx: Sender<Item>,
    /// Off: sounds are dropped (also those already waiting).
    enabled: Arc<AtomicBool>,
}

impl Sounds {
    pub fn new(dir: PathBuf) -> Sounds {
        let (tx, rx) = mpsc::channel::<Item>();
        let enabled = Arc::new(AtomicBool::new(true));
        let on = Arc::clone(&enabled);
        thread::spawn(move || {
            // `for … in rx` ends when every Sender is gone.
            for item in rx {
                if !on.load(Ordering::Relaxed) {
                    continue;
                }
                match item {
                    Item::File(name) => play_file(&dir, &name),
                    Item::Speak(text) => speak(&text),
                }
            }
        });
        Sounds { tx, enabled }
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    pub fn play(&self, name: &str) {
        let _ = self.tx.send(Item::File(name.to_string()));
    }

    pub fn speak(&self, text: &str) {
        let _ = self.tx.send(Item::Speak(text.to_string()));
    }
}

fn play_file(dir: &Path, name: &str) {
    let path = dir.join(format!("{name}.mp3"));
    if !path.exists() {
        return;
    }
    let mut cmd = if cfg!(target_os = "macos") {
        Command::new("afplay")
    } else {
        let mut c = Command::new("mpg123");
        c.arg("-q");
        c
    };
    // `status()` waits until the sound is over.
    if let Err(e) = cmd.arg(&path).status() {
        eprintln!("sound {name}: {e}");
    }
}

/// Speaks `text` in German and waits until it is over.
fn speak(text: &str) {
    if cfg!(target_os = "macos") {
        if let Err(e) = Command::new("say").args(["-v", "Anna", text]).status() {
            eprintln!("say: {e}");
        }
        return;
    }
    // pico2wave only writes a file; it must end in `.wav`.
    let wav = std::env::temp_dir().join("laptimer_speak.wav");
    let pico = Command::new("pico2wave").args(["-l", "de-DE", "-w"]).arg(&wav).arg(text).status();
    if pico.is_ok_and(|s| s.success()) {
        let _ = Command::new("aplay").arg("-q").arg(&wav).status();
        let _ = std::fs::remove_file(&wav);
        return;
    }
    if let Err(e) = Command::new("espeak-ng").args(["-v", "de", text]).status() {
        eprintln!("no speech synthesis (pico2wave or espeak-ng): {e}");
    }
}
