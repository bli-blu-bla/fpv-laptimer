//! Announcements: plays `<name>.mp3` from the sounds folder, one after the
//! other, in a thread of its own (so the timing code never waits for audio).
//! The player is `afplay` on macOS and `mpg123` elsewhere (the Pi).
//!
//! Sounds used: `rennen_beginnt`, `3`, `2`, `1`, `start`, `runde`,
//! `pilot1_fertig` … `pilot8_fertig`, `rennen_beendet`, `bestzeit`.
//! A missing file is skipped, so any of them can be left out.

use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Sender};
use std::thread;

pub struct Sounds {
    tx: Sender<String>,
}

impl Sounds {
    pub fn new(dir: PathBuf) -> Sounds {
        let (tx, rx) = mpsc::channel::<String>();
        thread::spawn(move || {
            // `for … in rx` ends when every Sender is gone.
            for name in rx {
                let path = dir.join(format!("{name}.mp3"));
                if !path.exists() {
                    continue;
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
        });
        Sounds { tx }
    }

    pub fn play(&self, name: &str) {
        let _ = self.tx.send(name.to_string());
    }
}
