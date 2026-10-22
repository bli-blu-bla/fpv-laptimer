//! Announcements: each one is a list of parts, played one after the other in
//! a thread of its own (so the timing code never waits for audio).
//!
//! A part is a ready-made clip from the sounds folder (`<name>.mp3`) or a
//! text. Texts are turned into WAV files by speech synthesis (`say` on
//! macOS, on the Pi `pico2wave` or else `espeak-ng`) and kept in a cache
//! folder that is emptied at every server start. Pilot names are prepared
//! when a session starts (`prepare`), so they are ready when needed. A clip
//! that is missing is replaced by its text (if it has one), so any clip can
//! be left out.
//!
//! Clips used (`tools/make_sounds.sh` makes placeholders):
//! - start: `piloten_bereit`, `rennen_beginnt`, `1` … `15`, `start` (tone),
//!   `training_beginnt`
//! - laps: `runde` (tone), `ist_in_runde`, `pilot`, `neue_bestzeit`
//! - end: `ist_im_ziel`, `rennen_beendet`
//!
//! Output: the server's speaker (`afplay` on macOS, `mpg123`/`aplay` on the
//! Pi) or the browsers: then the parts go out as the WebSocket message
//! `announce` with their URLs (`/api/audio/clips/…`, `/api/audio/tts/…`),
//! and every browser that is switched on as a speaker plays them.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;

use serde_json::json;
use tokio::sync::broadcast;

/// One piece of an announcement.
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    /// `<name>.mp3` from the sounds folder; if it is missing, `text` is
    /// spoken instead (empty: nothing, e.g. for tones).
    Clip { name: String, text: String },
    /// Spoken by speech synthesis.
    Text(String),
}

impl Part {
    pub fn clip(name: &str, text: &str) -> Part {
        Part::Clip {
            name: name.to_string(),
            text: text.to_string(),
        }
    }

    /// A tone: a clip without a spoken replacement.
    pub fn tone(name: &str) -> Part {
        Part::clip(name, "")
    }

    /// A number: clips exist for 1–15, the rest is spoken.
    pub fn number(n: usize) -> Part {
        Part::clip(&n.to_string(), &n.to_string())
    }

    pub fn text(text: &str) -> Part {
        Part::Text(text.to_string())
    }
}

pub struct Sounds {
    /// The queue: one announcement per entry.
    tx: Sender<Vec<Part>>,
    /// Off: announcements are dropped (also those already waiting).
    enabled: Arc<AtomicBool>,
    /// True: the browsers play them, not the server.
    browser: Arc<AtomicBool>,
    cache: PathBuf,
}

impl Sounds {
    /// `clips`: the sounds folder; `cache`: where spoken texts go (emptied
    /// here); `web`: the WebSocket broadcast, for the browser output.
    pub fn new(clips: PathBuf, cache: PathBuf, web: broadcast::Sender<String>) -> Sounds {
        let _ = std::fs::remove_dir_all(&cache);
        if let Err(e) = std::fs::create_dir_all(&cache) {
            eprintln!("sound cache {}: {e}", cache.display());
        }
        let (tx, rx) = mpsc::channel::<Vec<Part>>();
        let enabled = Arc::new(AtomicBool::new(true));
        let browser = Arc::new(AtomicBool::new(false));
        // The thread gets its own handles to the shared flags.
        let on = Arc::clone(&enabled);
        let to_browser = Arc::clone(&browser);
        let cache_dir = cache.clone();
        thread::spawn(move || {
            // `for … in rx` ends when every Sender is gone.
            for parts in rx {
                if !on.load(Ordering::Relaxed) {
                    continue;
                }
                let files = resolve(&clips, &cache_dir, &parts);
                if to_browser.load(Ordering::Relaxed) {
                    send_to_browsers(&web, &clips, &files);
                } else {
                    for f in &files {
                        if let Resolved::File(path) = f {
                            play_file(path);
                        }
                    }
                }
            }
        });
        Sounds {
            tx,
            enabled,
            browser,
            cache,
        }
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    pub fn set_browser(&self, browser: bool) {
        self.browser.store(browser, Ordering::Relaxed);
    }

    /// Queues an announcement.
    pub fn say(&self, parts: Vec<Part>) {
        if !parts.is_empty() {
            let _ = self.tx.send(parts);
        }
    }

    /// A single clip (countdown, tones).
    pub fn play(&self, name: &str, text: &str) {
        self.say(vec![Part::clip(name, text)]);
    }

    /// Synthesizes these texts now, in a thread of its own (so the queue
    /// does not wait), so they are ready when an announcement needs them.
    pub fn prepare(&self, texts: Vec<String>) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        let cache = self.cache.clone();
        thread::spawn(move || {
            for t in texts {
                synthesize(&cache, &t);
            }
        });
    }
}

/// A part, ready to play.
enum Resolved {
    File(PathBuf),
    /// Speech synthesis failed: the browser can still speak it itself.
    Text(String),
}

fn resolve(clips: &Path, cache: &Path, parts: &[Part]) -> Vec<Resolved> {
    let mut out = Vec::new();
    for p in parts {
        let text = match p {
            Part::Clip { name, text } => {
                let path = clips.join(format!("{name}.mp3"));
                if path.exists() {
                    out.push(Resolved::File(path));
                    continue;
                }
                text
            }
            Part::Text(text) => text,
        };
        if text.trim().is_empty() {
            continue;
        }
        match synthesize(cache, text) {
            Some(path) => out.push(Resolved::File(path)),
            None => out.push(Resolved::Text(text.clone())),
        }
    }
    out
}

/// The WebSocket message `announce`: `items` is a list of `{url}` or
/// `{text}` (the browser speaks those itself).
fn send_to_browsers(web: &broadcast::Sender<String>, clips: &Path, files: &[Resolved]) {
    let items: Vec<serde_json::Value> = files
        .iter()
        .map(|f| match f {
            Resolved::File(path) => {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let dir = if path.starts_with(clips) { "clips" } else { "tts" };
                json!({ "url": format!("/api/audio/{dir}/{name}") })
            }
            Resolved::Text(text) => json!({ "text": text }),
        })
        .collect();
    let _ = web.send(json!({ "type": "announce", "items": items }).to_string());
}

/// Plays an MP3 or WAV file and waits until it is over.
fn play_file(path: &Path) {
    let wav = path.extension().is_some_and(|e| e == "wav");
    let mut cmd = if cfg!(target_os = "macos") {
        Command::new("afplay")
    } else if wav {
        let mut c = Command::new("aplay");
        c.arg("-q");
        c
    } else {
        let mut c = Command::new("mpg123");
        c.arg("-q");
        c
    };
    // `status()` waits until the sound is over.
    if let Err(e) = cmd.arg(path).status() {
        eprintln!("sound {}: {e}", path.display());
    }
}

/// The WAV file with `text` spoken in German, made if it is not in the
/// cache yet. None: no speech synthesis installed.
fn synthesize(cache: &Path, text: &str) -> Option<PathBuf> {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    let path = cache.join(format!("{:016x}.wav", h.finish()));
    if path.exists() {
        return Some(path);
    }
    // Written under a temporary name and renamed when complete: two threads
    // may make the same text, and nobody must play half a file.
    let tmp = cache.join(format!("{:016x}.{:?}.tmp.wav", h.finish(), thread::current().id()));
    let ok = if cfg!(target_os = "macos") {
        run(Command::new("say")
            .args(["-v", "Anna", "--file-format=WAVE", "--data-format=LEI16@22050", "-o"])
            .arg(&tmp)
            .arg(text))
    } else {
        // pico2wave sounds better; the file name must end in `.wav`.
        run(Command::new("pico2wave")
            .args(["-l", "de-DE", "-w"])
            .arg(&tmp)
            .arg(text))
            || run(Command::new("espeak-ng").args(["-v", "de", "-w"]).arg(&tmp).arg(text))
    };
    if ok && std::fs::rename(&tmp, &path).is_ok() {
        return Some(path);
    }
    let _ = std::fs::remove_file(&tmp);
    eprintln!("no speech synthesis for {text:?} (say, pico2wave or espeak-ng)");
    None
}

fn run(cmd: &mut Command) -> bool {
    cmd.status().is_ok_and(|s| s.success())
}
