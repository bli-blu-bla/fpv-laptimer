//! Where the levels come from: the boards on USB, or a recording played back
//! in real time (`laptimer serve --replay <csv>`, or Boards → Quelle on the
//! page; for working on the server and the web page without flying).

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::core::BoardRole;
use crate::esp::board::{Board, Event, Level, ScanConfig, find_ports};

/// The running source; dropping it stops it.
pub enum Source {
    Boards(Vec<Board>),
    Replay {
        stop: Arc<AtomicBool>,
        /// Set to play the recording from the beginning again (`Core::start_session`).
        rewind: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    },
}

impl Source {
    /// The replay's rewind flag (for the core, to set at a session start);
    /// `None` for the boards.
    pub fn rewind_flag(&self) -> Option<Arc<AtomicBool>> {
        match self {
            Source::Boards(_) => None,
            Source::Replay { rewind, .. } => Some(Arc::clone(rewind)),
        }
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        match self {
            Source::Boards(boards) => boards.iter_mut().for_each(|b| b.stop()),
            Source::Replay { stop, thread, .. } => {
                stop.store(true, Ordering::Relaxed);
                if let Some(t) = thread.take() {
                    let _ = t.join();
                }
            }
        }
    }
}

/// What the page shows about each board, in board-index order.
pub struct BoardName {
    pub port: String,
    /// Stable across plugging (USB serial number), for the board's role.
    pub id: String,
    pub info: String,
    /// The name saved on the board, if any.
    pub name: Option<String>,
    /// Replay: the board's role in the recording (`# roles:`), used while
    /// the page has none set for it.
    pub role: Option<BoardRole>,
}
pub type BoardNames = Vec<BoardName>;

/// Opens every board found (a board that fails is reported, the others run).
pub fn start_boards(config: &ScanConfig, origin: Instant, tx: &Sender<Event>) -> Result<(Source, BoardNames)> {
    let mut boards = Vec::new();
    let mut names = Vec::new();
    for port in find_ports()? {
        match Board::start(boards.len(), &port.name, config, origin, tx.clone()) {
            Ok(b) => {
                names.push(BoardName {
                    port: port.name.clone(),
                    id: port.id.clone(),
                    info: b.info.clone(),
                    name: b.name.clone(),
                    role: None,
                });
                boards.push(b);
            }
            Err(e) => eprintln!("{}: {e:#}", port.name),
        }
    }
    Ok((Source::Boards(boards), names))
}

/// Plays a recording (as written by `laptimer record`, the board debugging
/// or a session) in a loop, with its times moved to now. Setting the rewind
/// flag starts it from the beginning (a recorded race then starts with the
/// new session, countdown included).
pub fn start_replay(path: &Path, origin: Instant, tx: Sender<Event>) -> Result<(Source, BoardNames)> {
    let mut levels: Vec<(usize, Level)> = Vec::new();
    let mut infos: Vec<String> = Vec::new();
    let mut roles: Vec<BoardRole> = Vec::new();
    for line in BufReader::new(File::open(path).with_context(|| format!("opening {}", path.display()))?).lines() {
        let line = line?;
        if let Some(rest) = line.strip_prefix("# board ") {
            infos.push(rest.to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("# roles:") {
            roles = crate::debug::parse_roles(rest);
            continue;
        }
        if line.starts_with('#') || line.starts_with("board") || line.is_empty() {
            continue;
        }
        let Some(f) = crate::record::parse_row(&line) else {
            continue;
        };
        let level = Level {
            host_us: f[2],
            board_us: f[3] as u64,
            channel: f[1] as u8,
            rssi: f[4] as u8,
            flags: f[5] as u8,
        };
        levels.push((f[0] as usize, level));
    }
    let Some(first) = levels.first().map(|(_, l)| l.host_us) else {
        anyhow::bail!("{}: no levels", path.display())
    };
    let last = levels.last().unwrap().1.host_us;
    // One board per index in the file (session files have no "# board" lines).
    let n_boards = levels.iter().map(|(b, _)| b + 1).max().unwrap_or(1).max(infos.len());
    let file = path.file_name().map_or_else(|| path.display().to_string(), |f| f.to_string_lossy().to_string());
    let names = (0..n_boards)
        .map(|i| BoardName {
            port: format!("Wiedergabe {file}"),
            id: format!("replay-{i}"),
            info: infos.get(i).cloned().unwrap_or_else(|| format!("Board {} der Aufnahme", i + 1)),
            name: None,
            role: roles.get(i).copied(),
        })
        .collect();

    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);
    let rewind = Arc::new(AtomicBool::new(false));
    let rewind_thread = Arc::clone(&rewind);
    let path = PathBuf::from(path);
    let thread = thread::spawn(move || {
        // Each round of the loop starts where the previous one ended.
        let mut shift = origin.elapsed().as_micros() as i64 - first;
        'play: while !stop_thread.load(Ordering::Relaxed) {
            let mut batch: Vec<Vec<Level>> = vec![Vec::new(); n_boards];
            let mut batch_end = first + 10_000;
            for (board, l) in &levels {
                if l.host_us >= batch_end {
                    // Wait until the batch's time has come, then send it.
                    // In short steps, so that a stop or rewind doesn't wait
                    // for a long gap in the recording.
                    let due = batch_end + shift;
                    loop {
                        let now = origin.elapsed().as_micros() as i64;
                        if due <= now || stop_thread.load(Ordering::Relaxed) || rewind_thread.load(Ordering::Relaxed) {
                            break;
                        }
                        thread::sleep(Duration::from_micros((due - now).min(50_000) as u64));
                    }
                    if rewind_thread.swap(false, Ordering::Relaxed) {
                        // The unsent batch is dropped; the times go on from
                        // now, so they keep rising.
                        shift = origin.elapsed().as_micros() as i64 - first;
                        eprintln!("replay of {} starts again (rewind)", path.display());
                        continue 'play;
                    }
                    for (b, lv) in batch.iter_mut().enumerate() {
                        if !lv.is_empty()
                            && tx
                                .send(Event::Levels {
                                    board: b,
                                    levels: std::mem::take(lv),
                                })
                                .is_err()
                        {
                            return;
                        }
                    }
                    batch_end = l.host_us + 10_000;
                    if stop_thread.load(Ordering::Relaxed) {
                        return;
                    }
                }
                batch[*board].push(Level {
                    host_us: l.host_us + shift,
                    ..*l
                });
            }
            shift += last - first + 1_000_000;
            eprintln!("replay of {} starts again", path.display());
        }
    });
    Ok((
        Source::Replay {
            stop,
            rewind,
            thread: Some(thread),
        },
        names,
    ))
}
