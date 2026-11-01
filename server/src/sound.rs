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

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Lang { #[default] En, De }

#[derive(Clone, Copy)]
pub enum Msg { TrainingStarted, PilotsReady, RaceStartsIn, NewBestLap, SessionRunning, Start, Pilot, HasFinished, IsInRound, RaceOver}

impl Msg {
    /// Language-neutral clip name (the mp3 file name).
    pub fn clip(self) -> &'static str {
        match self {
            Msg::TrainingStarted => "training_started",
            Msg::PilotsReady     => "pilots_ready",
            Msg::RaceStartsIn    => "race_starts_in",
            Msg::NewBestLap      => "new_best_lap",
            Msg::SessionRunning  => "session_running",
            Msg::Start           => "start",
            Msg::Pilot           => "pilot",
            Msg::HasFinished     => "ist_im_ziel",
            Msg::IsInRound       => "ist_in_runde",
            Msg::RaceOver        => "rennen_beendet",
        }
    }
}

impl Lang {
    pub fn text(self, m: Msg) -> &'static str {
        use {Lang::*, Msg::*};
        match (self, m) {
            (De, TrainingStarted) => "Training läuft",
            (En, TrainingStarted) => "Training started",
            (De, PilotsReady)     => "Piloten bereit",
            (En, PilotsReady)     => "Pilots, arm your quads",
            (De, RaceStartsIn)    => "Rennen beginnt in",
            (En, RaceStartsIn)    => "Race starts in",
            (De, NewBestLap)      => "hat eine neue Bestzeit erzielt",
            (En, NewBestLap)      => "has set a new best lap",
            (De, SessionRunning)  => "Es läuft schon eine Session - erst stoppen.",
            (En, SessionRunning)  => "A session is already running. Stop it first.",
            (De, Start)           => "Start",
            (En, Start)           => "Start",
            (En, Pilot)           => "pilot",
            (De, Pilot)           => "Pilot",
            (En, HasFinished)     => "has finished",
            (De, HasFinished)     => "ist im Ziel",
            (En, IsInRound)       => "is in round",
            (De, IsInRound)       => "ist in Runde",
            (En, RaceOver)        => "Race over",
            (De, RaceOver)        => "Rennen beendet",
        }
    }

    /// `pico2wave -l …`
    pub fn pico(self) -> &'static str {
        match self { Lang::De => "de-DE", Lang::En => "en-US" }
    }
    /// `espeak-ng -v …`
    pub fn espeak(self) -> &'static str {
        match self { Lang::De => "de", Lang::En => "en-us" }
    }
    /// For the browser's `utterance.lang`.
    pub fn bcp47(self) -> &'static str {
        match self { Lang::De => "de-DE", Lang::En => "en-US" }
    }

    /// `say -v …` on macOS.
    fn macos_voice(self) -> &'static str {
        match self { Lang::De => "Anna", Lang::En => "Daniel" }
    }

}

/// One piece of an announcement.
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    /// A sound from the sounds folder (`<name>.mp3`), e.g. the start tone
    /// or a beep. Language-neutral; if the file is missing, nothing plays.
    Clip(String),

    /// Spoken by speech synthesis, in the given language.
    Text { text: String, lang: Lang },
}

impl Part {
    /// A sound file, without the `.mp3`.
    pub fn clip(name: &str) -> Part {
        Part::Clip(name.to_string())
    }

    /// Arbitrary text (pilot names, times).
    pub fn text(text: impl Into<String>, lang: Lang) -> Part {
        Part::Text { text: text.into(), lang }
    }

    /// A fixed phrase in the announcement language.
    pub fn say(msg: Msg, lang: Lang) -> Part {
        Part::text(lang.text(msg), lang)
    }

    /// A countdown number, spoken.
    pub fn number(n: usize, lang: Lang) -> Part {
        Part::text(n.to_string(), lang)
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
                        if let Some(path) = &f.file {
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

    pub fn play(&self, part: Part) {
        self.say(vec![part])
    }

    /// Synthesizes these texts now, in a thread of its own (so the queue
    /// does not wait), so they are ready when an announcement needs them.
    pub fn prepare(&self, texts: Vec<String>, lang: Lang) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        let cache = self.cache.clone();
        thread::spawn(move || {
            for t in texts {
                synthesize(&cache, &t, lang);
            }
        });
    }
}

/// A part, ready to play: a clip has only the file; a text has the text
/// and, if speech synthesis worked, also its WAV file.
struct Resolved {
    file: Option<PathBuf>,
    text: Option<(String, Lang)>,
}

fn resolve(clips: &Path, cache: &Path, parts: &[Part]) -> Vec<Resolved> {
    let mut out = Vec::new();
    for p in parts {
        let (text, lang) = match p {
            Part::Clip(name) => {
                let path = clips.join(format!("{name}.mp3"));
                if path.exists() {
                    out.push(Resolved { file: Some(path), text: None });
                }
                continue;
            }
            Part::Text { text, lang } => (text, *lang),
        };
        if text.trim().is_empty() {
            continue;
        }
        out.push(Resolved {
            file: synthesize(cache, text, lang),
            text: Some((text.clone(), lang)),
        });
    }
    out
}

/// The WebSocket message `announce`: `items` is a list of `{url}` (clip),
/// `{url, text, lang}` (spoken text with the server's WAV) or `{text, lang}`
/// (no speech synthesis on the server). The browser chooses what to play.
fn send_to_browsers(web: &broadcast::Sender<String>, clips: &Path, files: &[Resolved]) {
    let items: Vec<serde_json::Value> = files
        .iter()
        .map(|f| {
            let mut item = json!({});
            if let Some(path) = &f.file {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let dir = if path.starts_with(clips) { "clips" } else { "tts" };
                item["url"] = json!(format!("/api/audio/{dir}/{name}"));
            }
            if let Some((text, lang)) = &f.text {
                item["text"] = json!(text);
                item["lang"] = json!(lang.bcp47());
            }
            item
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

/// The WAV file with `text` spoken in `lang`, made if it is not in the
/// cache yet. None: no speech synthesis installed.
fn synthesize(cache: &Path, text: &str, lang: Lang) -> Option<PathBuf> {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    lang.bcp47().hash(&mut h);
    let path = cache.join(format!("{:016x}.wav", h.finish()));
    if path.exists() {
        return Some(path);
    }
    // Written under a temporary name and renamed when complete: two threads
    // may make the same text, and nobody must play half a file.
    let tmp = cache.join(format!("{:016x}.{:?}.tmp.wav", h.finish(), thread::current().id()));
    let ok = if cfg!(target_os = "macos") {
        run(Command::new("say")
            .args(["-v", lang.macos_voice(), "--file-format=WAVE", "--data-format=LEI16@22050", "-o"])
            .arg(&tmp)
            .arg(text))
    } else {
        // pico2wave sounds better; the file name must end in `.wav`.
        run(Command::new("pico2wave")
            .args(["-l", lang.pico(), "-w"])
            .arg(&tmp)
            .arg(text))
            || run(Command::new("espeak-ng").args(["-v", lang.espeak(), "-w"]).arg(&tmp).arg(text))
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
