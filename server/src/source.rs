//! Where the levels come from: the boards on USB, or a recording played back
//! in real time (`laptimer serve --replay <csv>`, for working on the server
//! and the web page without flying).

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::esp::board::{Board, Event, Level, ScanConfig, find_ports};

/// The running source; dropping it stops it.
pub enum Source {
    Boards(Vec<Board>),
    Replay {
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    },
}

impl Drop for Source {
    fn drop(&mut self) {
        match self {
            Source::Boards(boards) => boards.iter_mut().for_each(|b| b.stop()),
            Source::Replay { stop, thread } => {
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
                });
                boards.push(b);
            }
            Err(e) => eprintln!("{}: {e:#}", port.name),
        }
    }
    Ok((Source::Boards(boards), names))
}

/// Plays a recording (as written by `laptimer record`) in a loop, with its
/// times moved to now.
pub fn start_replay(path: &Path, origin: Instant, tx: Sender<Event>) -> Result<(Source, BoardNames)> {
    let mut levels: Vec<(usize, Level)> = Vec::new();
    let mut names = Vec::new();
    for line in BufReader::new(File::open(path).with_context(|| format!("opening {}", path.display()))?).lines() {
        let line = line?;
        if let Some(rest) = line.strip_prefix("# board ") {
            names.push(BoardName {
                port: format!("replay {}", path.display()),
                id: format!("replay-{}", names.len()),
                info: rest.to_string(),
                name: None,
            });
            continue;
        }
        if line.starts_with('#') || line.starts_with("board") || line.is_empty() {
            continue;
        }
        let f: Vec<i64> = line.split(',').filter_map(|x| x.parse().ok()).collect();
        if f.len() < 6 {
            continue;
        }
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

    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);
    let path = PathBuf::from(path);
    let thread = thread::spawn(move || {
        // Each round of the loop starts where the previous one ended.
        let mut shift = origin.elapsed().as_micros() as i64 - first;
        let n_boards = levels.iter().map(|(b, _)| b + 1).max().unwrap_or(1);
        while !stop_thread.load(Ordering::Relaxed) {
            let mut batch: Vec<Vec<Level>> = vec![Vec::new(); n_boards];
            let mut batch_end = first + 10_000;
            for (board, l) in &levels {
                if l.host_us >= batch_end {
                    // Wait until the batch's time has come, then send it.
                    let due = batch_end + shift;
                    let now = origin.elapsed().as_micros() as i64;
                    if due > now {
                        thread::sleep(Duration::from_micros((due - now) as u64));
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
            thread: Some(thread),
        },
        names,
    ))
}
