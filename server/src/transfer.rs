//! Sessions from one laptimer to another, e.g. the Pi's recordings into a
//! test setup (Sessions → Auswählen → Exportieren → Paket; Sessions →
//! Importieren).
//!
//! - `GET /api/export/package?sessions=3,5`: a `.tar.gz` with
//!   `manifest.json` and per session `session_<id>.json` (the session with
//!   seats and config, all passes also deleted ones, the pilots in its seats)
//!   and `session_<id>.csv` (its level file, if there is one). Streamed
//!   while it is built: level files are tens of MB, the Pi 3 has 1 GB.
//! - `POST /api/import` (multipart field `package`): the upload goes to a
//!   file first, then every session gets a new id here. Pilots are matched by
//!   callsign, missing ones are created. A session that is already here
//!   (same creation and start, to the µs) is skipped, so importing the same
//!   package twice does nothing. An ended race without a confirmed result
//!   counts as confirmed (it would block starting a session here). The
//!   times stay as they were (wall clock), so the level files fit.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Multipart, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::core::unix_now_us;
use crate::db::{PassChange, Pilot, SessionRow};
use crate::laps::PassRecord;
use crate::server::App;

/// `manifest.json`'s `format`; `version` goes up when the package changes.
const FORMAT: &str = "laptimer-sessions";
const VERSION: u32 = 1;
/// Uploads up to 2 GB (a whole evening's level files).
const MAX_UPLOAD: usize = 2 << 30;

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/export/package", get(export))
        .route("/import", post(import).layer(DefaultBodyLimit::max(MAX_UPLOAD)))
}

type St = State<Arc<App>>;

fn bad(e: anyhow::Error) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("{e:#}") }))).into_response()
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    laptimer_version: String,
    exported_us: i64,
    /// The session ids in the package (as they were there).
    sessions: Vec<i64>,
}

/// `session_<id>.json`.
#[derive(Serialize, Deserialize)]
struct Packed {
    session: SessionRow,
    passes: Vec<PassRecord>,
    /// The change log of the passes (packages from before it: none).
    #[serde(default)]
    changes: Vec<PassChange>,
    /// The pilots of the seats (`seats[].pilot_id`), to match them here.
    pilots: Vec<Pilot>,
}

// --- export ---

#[derive(Deserialize)]
struct ExportQuery {
    /// "3,5": the session ids.
    sessions: String,
}

async fn export(State(app): St, Query(q): Query<ExportQuery>) -> Response {
    // Checked before the answer starts: later errors can only cut the file.
    let app2 = Arc::clone(&app);
    let list = match tokio::task::spawn_blocking(move || collect(&app2, &q.sessions)).await {
        Ok(Ok(list)) => list,
        Ok(Err(e)) => return bad(e),
        Err(e) => return bad(e.into()),
    };
    let ids: Vec<String> = list.iter().map(|(p, _)| p.session.id.to_string()).collect();
    let name = if ids.len() <= 5 {
        format!("laptimer_sessions_{}.tar.gz", ids.join("_"))
    } else {
        format!("laptimer_{}_sessions.tar.gz", ids.len())
    };

    // The package is written on a blocking thread into a channel; the
    // response body reads from it (at most 8 chunks waiting).
    let (tx, rx) = tokio::sync::mpsc::channel::<io::Result<Bytes>>(8);
    tokio::task::spawn_blocking(move || {
        let mut out = ChannelWriter {
            tx: tx.clone(),
            buf: Vec::new(),
        };
        if let Err(e) = write_package(&list, &mut out) {
            eprintln!("export package: {e:#}");
            let _ = tx.blocking_send(Err(io::Error::other(format!("{e:#}"))));
        }
    });
    let stream = futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|chunk| (chunk, rx)) });
    (
        [
            (header::CONTENT_TYPE, "application/gzip".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        ],
        Body::from_stream(stream),
    )
        .into_response()
}

/// The sessions to export, with their level files (oldest first).
fn collect(app: &App, ids: &str) -> Result<Vec<(Packed, Option<PathBuf>)>> {
    let ids: Vec<i64> = ids
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().with_context(|| format!("Ungültige Session „{s}“.")))
        .collect::<Result<_>>()?;
    if ids.is_empty() {
        bail!("Keine Session gewählt.");
    }
    let mut core = app.core.lock().unwrap();
    let pilots = core.db.pilots()?;
    let mut list = Vec::new();
    for id in ids {
        if core.active_id() == Some(id) {
            bail!("Session {id} läuft noch – erst stoppen.");
        }
        let mut session = core.db.session(id)?.with_context(|| format!("Session {id} gibt es nicht."))?;
        core.flush_levels(id)?;
        let levels = session.levels_file.take().map(PathBuf::from).filter(|p| p.is_file());
        let passes = core.db.passes(id)?;
        let changes = core.db.pass_changes(id)?;
        let pilots = pilots
            .iter()
            .filter(|p| session.seats.iter().any(|s| s.pilot_id == Some(p.id)))
            .cloned()
            .collect();
        list.push((
            Packed {
                session,
                passes,
                changes,
                pilots,
            },
            levels,
        ));
    }
    list.sort_by_key(|(p, _)| p.session.created_us);
    Ok(list)
}

fn write_package(list: &[(Packed, Option<PathBuf>)], out: &mut ChannelWriter) -> Result<()> {
    let mut tar = tar::Builder::new(GzEncoder::new(&mut *out, Compression::fast()));
    let now = (unix_now_us() / 1_000_000) as u64;
    let add = |tar: &mut tar::Builder<_>, name: &str, data: &[u8]| -> Result<()> {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(now);
        tar.append_data(&mut header, format!("laptimer_sessions/{name}"), data)?;
        Ok(())
    };
    let manifest = Manifest {
        format: FORMAT.into(),
        version: VERSION,
        laptimer_version: crate::VERSION.into(),
        exported_us: unix_now_us(),
        sessions: list.iter().map(|(p, _)| p.session.id).collect(),
    };
    add(&mut tar, "manifest.json", &serde_json::to_vec_pretty(&manifest)?)?;
    for (packed, levels) in list {
        let id = packed.session.id;
        add(&mut tar, &format!("session_{id}.json"), &serde_json::to_vec_pretty(packed)?)?;
        if let Some(path) = levels {
            let mut file = File::open(path).with_context(|| format!("{}", path.display()))?;
            tar.append_file(format!("laptimer_sessions/session_{id}.csv"), &mut file)?;
        }
    }
    tar.into_inner()?.finish()?;
    out.flush()?;
    Ok(())
}

/// Hands what is written to the response body in chunks of 64 KB. Fails
/// when the browser went away (the receiver is gone), which ends the export.
struct ChannelWriter {
    tx: tokio::sync::mpsc::Sender<io::Result<Bytes>>,
    buf: Vec<u8>,
}

impl ChannelWriter {
    fn send(&mut self) -> io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let chunk = Bytes::from(std::mem::take(&mut self.buf));
        // `blocking_send` waits while the body is behind; fine on a blocking thread.
        self.tx
            .blocking_send(Ok(chunk))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "Download abgebrochen"))
    }
}

impl Write for ChannelWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.extend_from_slice(data);
        if self.buf.len() >= 64 << 10 {
            self.send()?;
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.send()
    }
}

// --- import ---

/// Form field `package`: a package from `GET /api/export/package`.
async fn import(State(app): St, mut form: Multipart) -> Response {
    let data_dir = app.core.lock().unwrap().data_dir().to_path_buf();
    let tmp = data_dir.join(format!("import_{}.tar.gz", unix_now_us()));
    let saved = save_upload(&mut form, &tmp).await;
    let result = match saved {
        Ok(true) => {
            let tmp2 = tmp.clone();
            match tokio::task::spawn_blocking(move || import_file(&app, &tmp2)).await {
                Ok(r) => r,
                Err(e) => Err(e.into()),
            }
        }
        Ok(false) => Err(anyhow::anyhow!("Bitte eine Datei wählen (laptimer_sessions_….tar.gz).")),
        Err(e) => Err(e),
    };
    let _ = tokio::fs::remove_file(&tmp).await;
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => bad(e),
    }
}

/// Writes the field `package` to `path` piece by piece (no 2 GB in memory).
/// False: there was none.
async fn save_upload(form: &mut Multipart, path: &Path) -> Result<bool> {
    while let Some(mut field) = form.next_field().await.context("Upload abgebrochen")? {
        if field.name() != Some("package") {
            continue;
        }
        let mut file = tokio::fs::File::create(path).await?;
        while let Some(chunk) = field.chunk().await.context("Upload abgebrochen")? {
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        return Ok(true);
    }
    Ok(false)
}

/// Unpacks the package and adds its sessions. Returns what happened:
/// `{imported: [{id, from}], skipped: [{from, id}], pilots_created, confirmed}`.
fn import_file(app: &App, path: &Path) -> Result<Value> {
    let levels_dir = app.core.lock().unwrap().data_dir().join("levels");
    fs::create_dir_all(&levels_dir)?;
    let stamp = unix_now_us();
    // Level files go to a temporary name first (the new id comes later);
    // what is not renamed in the end is removed again.
    let mut tmps: Vec<PathBuf> = Vec::new();
    let result = (|| -> Result<Value> {
        let mut manifest: Option<Manifest> = None;
        let mut sessions: HashMap<i64, Packed> = HashMap::new();
        let mut levels: HashMap<i64, PathBuf> = HashMap::new();
        let mut archive = tar::Archive::new(GzDecoder::new(File::open(path)?));
        let not_ours = "Das ist kein Session-Paket des Laptimers";
        for entry in archive.entries().context(not_ours)? {
            let mut entry = entry.context(not_ours)?;
            let name = entry.path()?.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let id = |suffix: &str| -> Option<i64> { name.strip_prefix("session_")?.strip_suffix(suffix)?.parse().ok() };
            if name == "manifest.json" {
                manifest = Some(serde_json::from_reader(&mut entry).context("manifest.json")?);
            } else if let Some(id) = id(".json") {
                let packed = serde_json::from_reader(&mut entry).with_context(|| format!("{name}"))?;
                sessions.insert(id, packed);
            } else if let Some(id) = id(".csv") {
                let tmp = levels_dir.join(format!("import_{stamp}_{id}.tmp"));
                tmps.push(tmp.clone());
                io::copy(&mut entry, &mut File::create(&tmp)?)?;
                levels.insert(id, tmp);
            }
        }
        let manifest = manifest.context(format!("{not_ours}."))?;
        if manifest.format != FORMAT {
            bail!("{not_ours}.");
        }
        if manifest.version > VERSION {
            bail!(
                "Das Paket ist von einer neueren Version des Laptimers ({}): erst hier updaten.",
                manifest.laptimer_version
            );
        }

        let now = unix_now_us();
        let mut list: Vec<(i64, Packed)> = sessions.into_iter().collect();
        list.sort_by_key(|(_, p)| p.session.created_us);
        let (mut imported, mut skipped, mut pilots_created, mut confirmed) = (vec![], vec![], vec![], vec![]);
        let mut core = app.core.lock().unwrap();
        for (from, packed) in list {
            let mut row = packed.session;
            if let Some(id) = core.db.session_at(row.created_us, row.start_us)? {
                skipped.push(json!({ "from": from, "id": id }));
                continue;
            }
            // The seats' pilots: the one with the same callsign here, or a new one.
            for seat in &mut row.seats {
                let Some(p) = seat.pilot_id.and_then(|id| packed.pilots.iter().find(|p| p.id == id)) else {
                    seat.pilot_id = None;
                    continue;
                };
                let here = core.db.pilots()?.into_iter().find(|q| q.name == p.name);
                seat.pilot_id = Some(match here {
                    Some(q) => q.id,
                    None => {
                        pilots_created.push(p.name.clone());
                        core.add_pilot(&p.name, &p.full_name)?
                    }
                });
            }
            if row.ended_us.is_none() {
                row.ended_us = Some(packed.passes.iter().map(|p| p.time_us).max().unwrap_or(row.created_us));
            }
            if row.config.race && row.confirmed_us.is_none() {
                row.confirmed_us = Some(now);
                confirmed.push(from);
            }
            let id = core.db.import_session(&row, &packed.passes, &packed.changes, None)?;
            if let Some(tmp) = levels.remove(&from) {
                let dest = levels_dir.join(format!("session_{id}.csv"));
                fs::rename(&tmp, &dest)?;
                core.db.set_levels_file(id, &dest.to_string_lossy())?;
            }
            imported.push(json!({ "id": id, "from": from }));
        }
        if !imported.is_empty() {
            core.sessions_imported();
        }
        Ok(json!({
            "imported": imported,
            "skipped": skipped,
            "pilots_created": pilots_created,
            "confirmed": confirmed,
        }))
    })();
    for tmp in tmps {
        let _ = fs::remove_file(tmp); // the renamed ones are gone already
    }
    result
}
