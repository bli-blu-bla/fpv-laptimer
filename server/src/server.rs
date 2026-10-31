//! `laptimer serve`: the web server. A REST API for actions, a WebSocket for
//! everything that changes, and the web page (`frontend/dist`).
//!
//! Threads:
//! - the board threads (`esp/board.rs`) send levels into a std channel;
//! - the "core" thread takes them, feeds `Core` and calls `Core::tick`;
//! - tokio runs the web server; each request locks `Core` briefly.
//!
//! `Core` sits in a `std::sync::Mutex`: never hold it across an `.await`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};
use tower_http::services::{ServeDir, ServeFile};

use crate::api;
use crate::auth::{self, Auth};
use crate::config::{AuthConfig, UpdateConfig};
use crate::core::{BoardView, Core, Settings, results};
use crate::db::{By, Db, PassEdit};
use crate::detect::DetectConfig;
use crate::esp::board::{self, Event, ScanConfig, find_ports};
use crate::esp::flash;
use crate::events::Events;
use crate::hooks::Hooks;
use crate::sound::Sounds;
use crate::source::{self, Source};
use crate::stats;
use crate::trace::{self, SessionTrace};
use crate::update::Updater;

pub struct ServeOptions {
    /// `localhost`, `0.0.0.0`, or an address of this machine.
    pub host: String,
    pub port: u16,
    pub data_dir: PathBuf,
    pub web_dir: PathBuf,
    pub sounds_dir: PathBuf,
    /// Play this recording instead of reading boards.
    pub replay: Option<PathBuf>,
    /// The hooks' scripts folder (`hooks.rs`).
    pub hooks_dir: PathBuf,
    /// The password for changes (`auth.rs`).
    pub auth: AuthConfig,
    /// Updates from GitHub or Forgejo (`update.rs`).
    pub update: UpdateConfig,
}

/// Shared by all request handlers (axum's "state").
pub struct App {
    pub core: Mutex<Core>,
    tx: broadcast::Sender<String>,
    /// The events for the API's WebSocket (`events.rs`, `api.rs`).
    pub api_tx: broadcast::Sender<String>,
    pub hooks: Arc<Hooks>,
    pub auth: Auth,
    /// The boards (or the replay); replaced when channels or gain change.
    source: Mutex<Option<Source>>,
    events: Sender<Event>,
    /// The recording played instead of the boards (None: the boards).
    replay: Mutex<Option<Replay>>,
    /// Level graphs of sessions: (size of the level file, graph). A file
    /// that grew (the running session) is read again.
    /// Key: session id and gate (true: the arming gate).
    pub traces: Mutex<HashMap<(i64, bool), (u64, Arc<SessionTrace>)>>,
    /// Level files worked through for the board debugging: (source, file
    /// size, result), at most 2 (`debug.rs`).
    pub debug_files: Mutex<Vec<(String, u64, Arc<crate::debug::FileAnalysis>)>>,
    /// The firmware the page offers (`<web>/firmware`, `tools/stage_firmware.py`).
    firmware_dir: PathBuf,
    /// The flash in progress or the last one (`/api/firmware`).
    flash: Mutex<Option<FlashStatus>>,
    /// Updates, rollback, restart (`update.rs`).
    pub update: Updater,
    /// The connection to a server of the user's (`remote.rs`).
    pub remote: crate::remote::Remote,
}

/// What the page shows while a board is flashed; also sent as the
/// WebSocket message `flash`.
#[derive(Clone, serde::Serialize)]
struct FlashStatus {
    port: String,
    stage: String,
    /// 0–1 while writing.
    progress: Option<f64>,
    done: bool,
    error: Option<String>,
}

/// A recording played instead of the boards: from `--replay`, or chosen on
/// the page (Boards → Quelle, `/api/source`). Not kept across a restart: a
/// forgotten replay would hide the real boards at the next meeting.
#[derive(Clone)]
struct Replay {
    /// "rec:<name>", "session:<id>" (as in the board debugging) or "file"
    /// (`--replay`).
    source: String,
    path: PathBuf,
    label: String,
}

impl App {
    /// A recording plays instead of the boards.
    fn replaying(&self) -> bool {
        self.replay.lock().unwrap().is_some()
    }

    /// A board is being flashed (no restart now).
    pub fn flashing(&self) -> bool {
        self.flash.lock().unwrap().as_ref().is_some_and(|s| !s.done)
    }
}

pub fn run(opts: ServeOptions) -> Result<()> {
    std::fs::create_dir_all(&opts.data_dir)?;
    let db = Db::open(&opts.data_dir.join("laptimer.db").to_string_lossy())?;
    let remote = crate::remote::Remote::new(db.get_setting("remote")?.unwrap_or_default());
    let (tx, _) = broadcast::channel(256);
    let (api_tx, _) = broadcast::channel(1024);
    let (hooks, hooks_tx) = Hooks::start(opts.hooks_dir.clone(), db.hooks()?)?;
    let (events, events_rx) = mpsc::channel();
    let origin = Instant::now();
    let core = Core::new(
        db,
        opts.data_dir.clone(),
        origin,
        Sounds::new(opts.sounds_dir.clone(), opts.data_dir.join("tts"), tx.clone()),
        tx.clone(),
        Events::new(api_tx.clone(), hooks_tx),
    );
    let app = Arc::new(App {
        core: Mutex::new(core),
        tx,
        api_tx,
        hooks,
        auth: Auth::new(&opts.auth),
        source: Mutex::new(None),
        events,
        replay: Mutex::new(opts.replay.as_ref().map(|path| Replay {
            source: "file".into(),
            path: path.clone(),
            label: path.file_name().map_or_else(|| path.display().to_string(), |f| f.to_string_lossy().to_string()),
        })),
        traces: Mutex::new(HashMap::new()),
        debug_files: Mutex::new(Vec::new()),
        firmware_dir: opts.web_dir.join("firmware"),
        flash: Mutex::new(None),
        update: Updater::new(opts.update.clone())?,
        remote,
    });
    restart_source(&app)?;

    // The core thread: levels in, ticks.
    let app_core = Arc::clone(&app);
    thread::spawn(move || {
        loop {
            let first = match events_rx.recv_timeout(Duration::from_millis(10)) {
                Ok(ev) => Some(ev),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            let mut core = app_core.core.lock().unwrap();
            // Take everything that is waiting under one lock.
            for ev in first.into_iter().chain(events_rx.try_iter()) {
                if let Err(e) = core.on_board_event(ev) {
                    eprintln!("{e:#}");
                }
            }
            core.tick();
        }
    });

    let api = Router::new()
        .route("/state", get(get_state))
        .route("/session/start", post(start_session))
        .route("/session/stop", post(stop_session))
        .route("/sessions", get(list_sessions))
        .route("/sessions/{id}", get(get_session).delete(delete_session))
        .route("/sessions/{id}/confirm", post(confirm_session))
        .route("/sessions/{id}/passes", post(add_pass))
        .route("/sessions/{id}/graph", get(get_graph))
        .route("/sessions/{id}/snap", get(get_snap))
        .route("/sessions/{id}/edit", post(edit_passes))
        .route("/sessions/{id}/trim", post(trim_session))
        .route("/passes/{id}", post(set_pass_deleted))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/pilots", post(add_pilot))
        .route("/pilots/{id}", axum::routing::put(update_pilot).delete(delete_pilot))
        .route("/presets", post(add_preset))
        .route("/presets/{id}", axum::routing::put(update_preset).delete(delete_preset))
        .route("/stats", get(get_stats))
        .route("/boards/rescan", post(rescan_boards))
        .route("/source", get(get_source).put(set_source))
        .route("/boards/name", post(name_board))
        .route("/firmware", get(get_firmware))
        .route("/firmware/flash", post(flash_board))
        .route("/announce/test", post(announce_test))
        .merge(auth::routes())
        .merge(api::page_routes())
        .merge(crate::data::routes())
        .merge(crate::transfer::routes())
        .merge(crate::debug::routes())
        .merge(crate::update::routes())
        .merge(crate::remote::routes())
        .nest("/v1", api::router(Arc::clone(&app)))
        // Announcements played by the browsers: the clips and the spoken texts.
        .nest_service("/audio/clips", ServeDir::new(&opts.sounds_dir))
        .nest_service("/audio/tts", ServeDir::new(opts.data_dir.join("tts")))
        // Changes need the login (reading stays open).
        .layer(middleware::from_fn_with_state(Arc::clone(&app), auth::require_login));
    let app_remote = Arc::clone(&app);
    let app_warm = Arc::clone(&app);
    let web = ServeDir::new(&opts.web_dir).fallback(ServeFile::new(opts.web_dir.join("index.html")));
    let router = Router::new()
        .nest("/api", api)
        .route("/ws", get(websocket))
        .fallback_service(web)
        // gzip for the browsers that ask for it (all do): graphs, lists and the
        // page's scripts shrink to a fifth or less, which counts on the WLAN.
        // Not for what is packed already (packages, backups) or MP3s.
        .layer(CompressionLayer::new().gzip(true).compress_when(
            DefaultPredicate::new()
                .and(NotForContentType::new("application/gzip"))
                .and(NotForContentType::new("audio/")),
        ))
        .with_state(app);

    let everywhere = opts.host == "0.0.0.0" || opts.host == "::";
    println!(
        "laptimer {}: http://{}:{}{}",
        crate::VERSION,
        if everywhere { "localhost" } else { opts.host.as_str() },
        opts.port,
        if everywhere { " (and this machine's address in the network)" } else { "" }
    );
    if opts.host == "localhost" || opts.host.starts_with("127.") {
        println!("laptimer: nur von diesem Rechner erreichbar (für Handys: host = \"0.0.0.0\" in config.toml)");
    }
    if !opts.auth.enabled {
        println!("laptimer: ohne Passwort ([auth] enabled = false): jeder im Netz kann alles ändern");
    }
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        tokio::spawn(crate::remote::run(app_remote));
        tokio::spawn(warm_traces(app_warm));
        // `localhost` would resolve to ::1 only on some systems (then
        // 127.0.0.1 does not answer); other names are looked up by `bind`.
        let host = if opts.host == "localhost" { "127.0.0.1" } else { opts.host.as_str() };
        let listener = tokio::net::TcpListener::bind((host, opts.port))
            .await
            .with_context(|| format!("{}:{}", opts.host, opts.port))?;
        // With the client's address: the login slows down guessing per address.
        axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).await?;
        anyhow::Ok(())
    })
}

/// Stops the boards (or replay) and starts them with the current settings.
/// Talks to the boards, so it takes a moment: not on a tokio thread.
pub fn restart_source(app: &App) -> Result<()> {
    let mut source = app.source.lock().unwrap();
    start_source(app, &mut source)
}

/// `restart_source` with the source lock already held (the flash holds it
/// while it has a board's port).
fn start_source(app: &App, source: &mut Option<Source>) -> Result<()> {
    *source = None; // dropping the old source stops it
    let (settings, origin) = {
        let core = app.core.lock().unwrap();
        (core.settings.clone(), core.origin())
    };
    let replay = app.replay.lock().unwrap().clone();
    let (new, names) = match &replay {
        Some(r) => source::start_replay(&r.path, origin, app.events.clone())?,
        None => {
            let config = ScanConfig {
                channels: settings.channels.iter().map(|c| (c.name.clone(), c.mhz)).collect(),
                gain: settings.gain,
                batch_ms: 10,
            };
            source::start_boards(&config, origin, &app.events)?
        }
    };
    if names.is_empty() {
        eprintln!("no boards found");
    }
    let rewind = new.rewind_flag();
    *source = Some(new);
    let views = names
        .into_iter()
        .map(|n| {
            let mut view = BoardView::new(n.port, n.id, n.info, n.name);
            view.default_role = n.role;
            view
        })
        .collect();
    let replay = replay.map(|r| {
        let channels = crate::debug::file_channels(&r.path).unwrap_or_default();
        json!({ "source": r.source, "label": r.label, "channels": channels })
    });
    let mut core = app.core.lock().unwrap();
    core.replay = replay;
    core.replay_rewind = rewind;
    core.boards_started(views);
    Ok(())
}

/// What plays instead of the boards (`replay`: `{source, label, channels}`
/// or null); the same as `state.replay`.
async fn get_source(State(app): State<Arc<App>>) -> ApiResult {
    Ok(Json(json!({ "replay": app.core.lock().unwrap().replay })))
}

#[derive(Deserialize)]
struct SourceRequest {
    /// "rec:<name>" or "session:<id>"; null: the boards again.
    replay: Option<String>,
}

/// Plays a recording or a session's levels instead of the boards, or goes
/// back to the boards. Not during a session or a flash. A file that does
/// not play leaves the source as it was.
async fn set_source(State(app): State<Arc<App>>, Json(req): Json<SourceRequest>) -> ApiResult {
    if app.core.lock().unwrap().session_running() {
        return Err(anyhow::anyhow!("Erst die laufende Session beenden.").into());
    }
    if app.flashing() {
        return Err(anyhow::anyhow!("Es wird gerade ein Board geflasht.").into());
    }
    let new = match req.replay {
        None => None,
        Some(source) => {
            let (path, _) = crate::debug::source_file(&app, &source)?;
            if !path.is_file() {
                return Err(anyhow::anyhow!("Die Pegel-Datei fehlt: {}", path.display()).into());
            }
            let label = match source.strip_prefix("session:") {
                Some(id) => format!("Session #{id}"),
                None => source.trim_start_matches("rec:").to_string(),
            };
            Some(Replay { source, path, label })
        }
    };
    // Starting the source blocks (boards, or reading the file): off the async threads.
    let app2 = Arc::clone(&app);
    tokio::task::spawn_blocking(move || {
        let old = std::mem::replace(&mut *app2.replay.lock().unwrap(), new);
        let started = restart_source(&app2);
        if started.is_err() {
            // Back to what ran before, so there is still a source.
            *app2.replay.lock().unwrap() = old;
            let _ = restart_source(&app2);
        }
        started
    })
    .await??;
    Ok(Json(json!({ "replay": app.core.lock().unwrap().replay })))
}

// --- errors: any failure becomes a 400 with the message ---

struct ApiError(anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("{:#}", self.0) })),
        )
            .into_response()
    }
}

// `?` turns any error into an ApiError through this.
impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        ApiError(e.into())
    }
}

type ApiResult = Result<Json<Value>, ApiError>;

// --- handlers ---

async fn get_state(State(app): State<Arc<App>>) -> ApiResult {
    Ok(Json(app.core.lock().unwrap().state_json()))
}

#[derive(Deserialize)]
struct StartRequest {
    race: bool,
}

async fn start_session(State(app): State<Arc<App>>, Json(req): Json<StartRequest>) -> ApiResult {
    let id = app.core.lock().unwrap().start_session(req.race)?;
    Ok(Json(json!({ "id": id })))
}

async fn stop_session(State(app): State<Arc<App>>) -> ApiResult {
    app.core.lock().unwrap().stop_session()?;
    Ok(Json(json!({})))
}

/// The last 100 sessions, each with its results.
async fn list_sessions(State(app): State<Arc<App>>) -> ApiResult {
    let core = app.core.lock().unwrap();
    let mut out = Vec::new();
    for s in core.db.sessions(100)? {
        let passes = core.db.passes(s.id)?;
        out.push(json!({ "session": s, "results": results(&s, &passes) }));
    }
    Ok(Json(Value::Array(out)))
}

async fn get_session(State(app): State<Arc<App>>, Path(id): Path<i64>) -> ApiResult {
    let core = app.core.lock().unwrap();
    let s = core.db.session(id)?.context("Diese Session gibt es nicht.")?;
    let passes = core.db.passes(id)?;
    let changes = core.db.pass_changes(id)?;
    Ok(Json(
        json!({ "session": s, "results": results(&s, &passes), "passes": passes, "changes": changes }),
    ))
}

async fn confirm_session(State(app): State<Arc<App>>, Path(id): Path<i64>) -> ApiResult {
    app.core.lock().unwrap().confirm_session(id)?;
    Ok(Json(json!({})))
}

async fn delete_session(State(app): State<Arc<App>>, Path(id): Path<i64>) -> ApiResult {
    app.core.lock().unwrap().delete_session(id)?;
    Ok(Json(json!({})))
}

/// Cuts the levels 10 s after the last pass (`Core::trim_session`).
async fn trim_session(State(app): State<Arc<App>>, Path(id): Path<i64>) -> ApiResult {
    let (cut_us, removed) = app.core.lock().unwrap().trim_session(id)?;
    app.traces.lock().unwrap().retain(|(k, _), _| *k != id);
    Ok(Json(json!({ "cut_us": cut_us, "removed": removed })))
}

/// A session's level graph, from the cache or computed. Reads a file of up
/// to hundreds of MB: call it off the async threads (`spawn_blocking`); it
/// doesn't hold the core lock while it reads.
pub fn session_trace(app: &App, id: i64) -> Result<Arc<SessionTrace>> {
    gate_trace(app, id, false)
}

/// `session_trace` of the finish gate's boards, or (`arming`) of the
/// arming gate's, with that gate's thresholds.
pub fn gate_trace(app: &App, id: i64, arming: bool) -> Result<Arc<SessionTrace>> {
    let (path, config, fixed, min_rise) = {
        let mut core = app.core.lock().unwrap();
        let s = core.db.session(id)?.context("Diese Session gibt es nicht.")?;
        core.flush_levels(id)?;
        let path = s
            .levels_file
            .context("Für diese Session sind keine Pegel aufgezeichnet.")?;
        // Per channel for every possible channel (8); unused ones don't matter.
        // Sessions from before `detection` was saved: the defaults.
        let detection = s.config.detection.clone().unwrap_or_default();
        (
            PathBuf::from(path),
            DetectConfig {
                min_lap_us: (s.config.min_lap_s * 1e6) as i64,
                min_rise_db: detection.min_rise_db,
            },
            detection.fixed(arming, 8),
            detection.min_rise(arming, 8),
        )
    };
    let size = std::fs::metadata(&path)
        .with_context(|| format!("{}", path.display()))?
        .len();
    if let Some((cached_size, t)) = app.traces.lock().unwrap().get(&(id, arming))
        && *cached_size == size
    {
        return Ok(Arc::clone(t));
    }
    let t = Arc::new(trace::load(&path, config, fixed, min_rise, arming)?);
    let mut traces = app.traces.lock().unwrap();
    // A few MB each (finish and arming gate of a session are two): keep the
    // memory small; the next request reads again.
    if traces.len() >= 8 {
        traces.clear();
    }
    traces.insert((id, arming), (size, Arc::clone(&t)));
    Ok(t)
}

/// Works out the level graph of every session that ends (both gates) in
/// the background, so that it is in the cache when the race control opens
/// the editor: on the Pi a long session takes seconds.
async fn warm_traces(app: Arc<App>) {
    let mut events = app.api_tx.subscribe();
    loop {
        let msg = match events.recv().await {
            Ok(msg) => msg,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => return,
        };
        let Ok(v) = serde_json::from_str::<Value>(&msg) else { continue };
        if v["event"] != "session_stopped" || v["data"]["reason"] == "cancelled" {
            continue;
        }
        let Some(id) = v["data"]["session_id"].as_i64() else { continue };
        let app = Arc::clone(&app);
        // Errors (no level file) show up when the page asks.
        tokio::task::spawn_blocking(move || {
            let _ = gate_trace(&app, id, false);
            let _ = gate_trace(&app, id, true);
        });
    }
}

#[derive(Deserialize)]
struct GraphQuery {
    channel: usize,
    /// Wall clock µs; missing: from the start / to the end of the session.
    from_us: Option<i64>,
    to_us: Option<i64>,
    /// At most this many points (about the width in pixels).
    points: Option<usize>,
    /// "arming": the arming gate's boards (default: the finish gate's).
    gate: Option<String>,
}

async fn get_graph(State(app): State<Arc<App>>, Path(id): Path<i64>, Query(q): Query<GraphQuery>) -> ApiResult {
    let arming = q.gate.as_deref() == Some("arming");
    let t = tokio::task::spawn_blocking(move || gate_trace(&app, id, arming)).await??;
    let Some((start, end)) = t.bounds(q.channel) else {
        return Ok(Json(json!({ "start_us": null, "end_us": null, "graph": null })));
    };
    let g = t.graph(
        q.channel,
        q.from_us.unwrap_or(start),
        q.to_us.unwrap_or(end),
        q.points.unwrap_or(800).min(5000),
    );
    Ok(Json(json!({ "start_us": start, "end_us": end, "graph": g })))
}

#[derive(Deserialize)]
struct SnapQuery {
    channel: usize,
    time_us: i64,
}

/// Where a pass added at `time_us` would go (see `add_pass` with `snap`).
async fn get_snap(State(app): State<Arc<App>>, Path(id): Path<i64>, Query(q): Query<SnapQuery>) -> ApiResult {
    let t = tokio::task::spawn_blocking(move || session_trace(&app, id)).await??;
    Ok(Json(match t.snap(q.channel, q.time_us, SNAP_WINDOW_US) {
        Some((time_us, peak_db)) => json!({ "time_us": time_us, "peak_db": peak_db }),
        None => json!({ "time_us": null, "peak_db": null }),
    }))
}

/// A tap this close to a peak snaps to it.
const SNAP_WINDOW_US: i64 = 1_000_000;

#[derive(Deserialize)]
struct AddPass {
    channel: u8,
    time_us: i64,
    /// Move it to the nearest peak of the level graph (within 1 s).
    #[serde(default)]
    snap: bool,
}

/// A pass the detection missed, added by hand.
async fn add_pass(State(app): State<Arc<App>>, Path(id): Path<i64>, Json(req): Json<AddPass>) -> ApiResult {
    let (mut time_us, mut peak_db) = (req.time_us, 0.0);
    if req.snap {
        let app2 = Arc::clone(&app);
        let t = tokio::task::spawn_blocking(move || session_trace(&app2, id)).await??;
        if let Some((t, p)) = t.snap(req.channel as usize, req.time_us, SNAP_WINDOW_US) {
            (time_us, peak_db) = (t, p);
        }
    }
    let mut core = app.core.lock().unwrap();
    check_room(&core, id, req.channel, time_us, None, MANUAL_GAP_US)?;
    let by = By { origin: "seite", at_us: core.now_us() };
    let pass = core.db.add_manual_pass(id, req.channel, time_us, peak_db, by)?;
    core.session_changed(id)?;
    Ok(Json(json!({ "id": pass, "time_us": time_us })))
}

/// Passes set by hand (page) may make laps shorter than the minimum lap:
/// the race director knows better. Only two passes on the same spot (a
/// double tap) are refused.
pub const MANUAL_GAP_US: i64 = 100_000;

/// A pass closer than `min_gap_us` to another (not deleted) pass of the
/// same channel: refuse it. `min_gap_us`: the minimum lap (API, e.g. a
/// button at the gate that may bounce) or `MANUAL_GAP_US` (page).
pub fn check_room(
    core: &Core,
    session: i64,
    channel: u8,
    time_us: i64,
    except: Option<i64>,
    min_gap_us: i64,
) -> Result<()> {
    core.db.session(session)?.context("Diese Session gibt es nicht.")?;
    let clash = core.db.passes(session)?.into_iter().find(|p| {
        p.channel == channel
            && p.gate == 0
            && !p.deleted
            && Some(p.id) != except
            && (p.time_us - time_us).abs() < min_gap_us
    });
    if clash.is_some() {
        anyhow::bail!(
            "Hier ist schon ein Durchflug (näher als {:.1} s). Erst den anderen löschen.",
            min_gap_us as f64 / 1e6
        );
    }
    Ok(())
}

/// The minimum lap of a session in µs.
pub fn min_lap_us(core: &Core, session: i64) -> Result<i64> {
    let s = core.db.session(session)?.context("Diese Session gibt es nicht.")?;
    Ok((s.config.min_lap_s * 1e6) as i64)
}

#[derive(Deserialize)]
struct EditRequest {
    channel: u8,
    #[serde(flatten)]
    edit: PassEdit,
}

/// Corrections of one channel from the pass editor, saved at once. Laps
/// shorter than the minimum lap are allowed (set by hand), only two passes
/// on the same spot are not.
async fn edit_passes(State(app): State<Arc<App>>, Path(id): Path<i64>, Json(req): Json<EditRequest>) -> ApiResult {
    let mut core = app.core.lock().unwrap();
    core.db.session(id)?.context("Diese Session gibt es nicht.")?;
    let e = &req.edit;
    let mut times: Vec<i64> = core
        .db
        .passes(id)?
        .into_iter()
        .filter(|p| p.channel == req.channel && p.gate == 0)
        .filter(|p| (!p.deleted || e.restores.contains(&p.id)) && !e.deletes.contains(&p.id))
        .map(|p| e.moves.iter().find(|(m, _)| *m == p.id).map_or(p.time_us, |(_, t)| *t))
        .chain(e.adds.iter().copied())
        .collect();
    times.sort();
    if let Some(w) = times.windows(2).find(|w| w[1] - w[0] < MANUAL_GAP_US) {
        return Err(anyhow::anyhow!(
            "Zwei Durchflüge liegen nur {:.2} s auseinander – einer davon ist doppelt.",
            (w[1] - w[0]) as f64 / 1e6
        )
        .into());
    }
    let by = By { origin: "editor", at_us: core.now_us() };
    core.db.edit_passes(id, req.channel, e, by)?;
    core.session_changed(id)?;
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct SetDeleted {
    deleted: bool,
}

async fn set_pass_deleted(State(app): State<Arc<App>>, Path(id): Path<i64>, Json(req): Json<SetDeleted>) -> ApiResult {
    let mut core = app.core.lock().unwrap();
    if !req.deleted {
        // Restoring: the same rule as adding.
        let session = core.db.pass_session(id)?.context("Diesen Durchflug gibt es nicht.")?;
        let p = core
            .db
            .passes(session)?
            .into_iter()
            .find(|p| p.id == id)
            .context("Diesen Durchflug gibt es nicht.")?;
        check_room(&core, session, p.channel, p.time_us, Some(id), MANUAL_GAP_US)?;
    }
    let session = core
        .db
        .set_pass_deleted(id, req.deleted, By { origin: "seite", at_us: core.now_us() })?
        .context("Diesen Durchflug gibt es nicht.")?;
    core.session_changed(session)?;
    Ok(Json(json!({})))
}

async fn get_settings(State(app): State<Arc<App>>) -> ApiResult {
    Ok(Json(serde_json::to_value(&app.core.lock().unwrap().settings)?))
}

async fn put_settings(State(app): State<Arc<App>>, Json(s): Json<Settings>) -> ApiResult {
    if s.channels.is_empty() || s.channels.len() > 8 {
        return Err(anyhow::anyhow!("1 bis 8 Kanäle").into());
    }
    let restart = {
        let mut core = app.core.lock().unwrap();
        let old = core.settings.clone();
        core.set_settings(s.clone())?;
        old.channels != s.channels || old.gain != s.gain
    };
    if restart {
        // Talking to the boards blocks: off the async threads.
        let app2 = Arc::clone(&app);
        tokio::task::spawn_blocking(move || restart_source(&app2)).await??;
    }
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct PilotData {
    /// The callsign.
    name: String,
    #[serde(default)]
    full_name: String,
}

impl PilotData {
    fn checked(&self) -> Result<(&str, &str)> {
        let name = self.name.trim();
        if name.is_empty() {
            anyhow::bail!("Der Callsign fehlt.");
        }
        Ok((name, self.full_name.trim()))
    }
}

async fn add_pilot(State(app): State<Arc<App>>, Json(req): Json<PilotData>) -> ApiResult {
    let (name, full_name) = req.checked()?;
    let id = app.core.lock().unwrap().add_pilot(name, full_name)?;
    Ok(Json(json!({ "id": id })))
}

async fn update_pilot(State(app): State<Arc<App>>, Path(id): Path<i64>, Json(req): Json<PilotData>) -> ApiResult {
    let (name, full_name) = req.checked()?;
    app.core.lock().unwrap().update_pilot(id, name, full_name)?;
    Ok(Json(json!({})))
}

async fn delete_pilot(State(app): State<Arc<App>>, Path(id): Path<i64>) -> ApiResult {
    app.core.lock().unwrap().delete_pilot(id)?;
    Ok(Json(json!({})))
}

/// `POST /api/presets`: a new preset from a slot's detection.
#[derive(Deserialize)]
struct NewPreset {
    pilot_id: i64,
    name: String,
    slot: usize,
}

async fn add_preset(State(app): State<Arc<App>>, Json(req): Json<NewPreset>) -> ApiResult {
    let id = app.core.lock().unwrap().add_preset(req.pilot_id, &req.name, req.slot)?;
    Ok(Json(json!({ "id": id })))
}

/// `PUT /api/presets/{id}`: rename, and/or take the values from a slot.
#[derive(Deserialize)]
struct PresetChange {
    name: Option<String>,
    slot: Option<usize>,
}

async fn update_preset(State(app): State<Arc<App>>, Path(id): Path<i64>, Json(req): Json<PresetChange>) -> ApiResult {
    app.core
        .lock()
        .unwrap()
        .update_preset(id, req.name.as_deref(), req.slot)?;
    Ok(Json(json!({})))
}

async fn delete_preset(State(app): State<Arc<App>>, Path(id): Path<i64>) -> ApiResult {
    app.core.lock().unwrap().delete_preset(id)?;
    Ok(Json(json!({})))
}

/// Best laps, races and wins per pilot, all time and today (`stats.rs`).
async fn get_stats(State(app): State<Arc<App>>) -> ApiResult {
    Ok(Json(stats_json(&app.core.lock().unwrap())?))
}

/// The laps of a session's seats (for `api.rs`).
pub fn session_results(s: &crate::db::SessionRow, passes: &[crate::laps::PassRecord]) -> Vec<crate::laps::PilotResult> {
    results(s, passes)
}

pub fn stats_json(core: &Core) -> Result<Value> {
    let mut sessions = Vec::new();
    for s in core.db.sessions(2000)? {
        let passes = core.db.passes(s.id)?;
        let r = results(&s, &passes);
        sessions.push((s, r));
    }
    let today_start = chrono::Local::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|t| t.and_local_timezone(chrono::Local).earliest())
        .map_or(0, |t| t.timestamp_micros());
    let n = core.settings.consecutive as usize;
    Ok(serde_json::to_value(stats::compute(&sessions, today_start, n))?)
}

/// The firmware on offer, the boards' ports (also those without our
/// firmware) and the flash in progress or the last one.
async fn get_firmware(State(app): State<Arc<App>>) -> ApiResult {
    let (firmware, error) = match flash::load_manifest(&app.firmware_dir) {
        Ok(m) => (Some(m), None),
        Err(e) => (None, Some(format!("{e:#}"))),
    };
    let ports = if app.replaying() {
        Vec::new()
    } else {
        find_ports()?
    };
    let core = app.core.lock().unwrap();
    let ports: Vec<Value> = ports
        .into_iter()
        .map(|p| {
            // A board that runs our firmware and was set up has its INFO.
            let board = core.boards.iter().find(|b| b.port == p.name);
            json!({ "port": p.name, "id": p.id, "info": board.map(|b| b.info.clone()), "name": board.and_then(|b| b.name.clone()) })
        })
        .collect();
    Ok(Json(json!({
        "firmware": firmware,
        "error": error,
        "ports": ports,
        "flash": *app.flash.lock().unwrap(),
    })))
}

#[derive(Deserialize)]
struct FlashRequest {
    port: String,
}

/// Writes the firmware to the board at `port`, in a thread of its own:
/// the boards stop for that time and start again afterwards. Progress goes
/// out as `flash` messages over the WebSocket.
/// Looks for boards again (e.g. plugged in after the server started) and
/// starts them all anew. Not during a session or a flash.
async fn rescan_boards(State(app): State<Arc<App>>) -> ApiResult {
    if app.core.lock().unwrap().session_running() {
        return Err(anyhow::anyhow!("Erst die laufende Session beenden.").into());
    }
    if app.flash.lock().unwrap().as_ref().is_some_and(|s| !s.done) {
        return Err(anyhow::anyhow!("Es wird gerade ein Board geflasht.").into());
    }
    // Talking to the boards blocks: off the async threads.
    let app2 = Arc::clone(&app);
    tokio::task::spawn_blocking(move || restart_source(&app2)).await??;
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct NameRequest {
    /// The board's id (USB serial number).
    id: String,
    /// "" clears the name.
    name: String,
}

/// Saves a name on a board. The board's port is busy while it scans, so all
/// boards stop for that moment and start again (as with a rescan).
async fn name_board(State(app): State<Arc<App>>, Json(req): Json<NameRequest>) -> ApiResult {
    if app.replaying() {
        return Err(anyhow::anyhow!("Während der Wiedergabe einer Aufnahme gibt es keine Boards.").into());
    }
    let name = req.name.trim().to_string();
    board::check_name(&name)?;
    let port = {
        let core = app.core.lock().unwrap();
        if core.session_running() {
            return Err(anyhow::anyhow!("Erst die laufende Session beenden.").into());
        }
        match core.boards.iter().find(|b| b.id == req.id) {
            Some(b) => b.port.clone(),
            None => return Err(anyhow::anyhow!("Das Board ist nicht (mehr) angesteckt.").into()),
        }
    };
    if app.flash.lock().unwrap().as_ref().is_some_and(|s| !s.done) {
        return Err(anyhow::anyhow!("Es wird gerade ein Board geflasht.").into());
    }
    // Talking to the boards blocks: off the async threads.
    let app2 = Arc::clone(&app);
    tokio::task::spawn_blocking(move || {
        let mut source = app2.source.lock().unwrap();
        *source = None; // stops all boards: frees the port
        let saved = board::set_name(&port, &name);
        // Start the boards again either way; the first error counts.
        let restarted = start_source(&app2, &mut source);
        saved.and(restarted)
    })
    .await??;
    Ok(Json(json!({})))
}

async fn flash_board(State(app): State<Arc<App>>, Json(req): Json<FlashRequest>) -> ApiResult {
    if app.replaying() {
        return Err(anyhow::anyhow!("Während der Wiedergabe einer Aufnahme gibt es keine Boards.").into());
    }
    if app.core.lock().unwrap().session_running() {
        return Err(anyhow::anyhow!("Erst die laufende Session beenden.").into());
    }
    if !find_ports()?.iter().any(|p| p.name == req.port) {
        return Err(anyhow::anyhow!("{} ist nicht (mehr) angesteckt.", req.port).into());
    }
    flash::load_manifest(&app.firmware_dir)?;
    {
        let mut status = app.flash.lock().unwrap();
        if status.as_ref().is_some_and(|s| !s.done) {
            return Err(anyhow::anyhow!("Es wird schon ein Board geflasht.").into());
        }
        *status = Some(FlashStatus {
            port: req.port.clone(),
            stage: "Boards anhalten …".into(),
            progress: None,
            done: false,
            error: None,
        });
    }
    let app2 = Arc::clone(&app);
    thread::spawn(move || run_flash(&app2, &req.port));
    Ok(Json(json!({})))
}

fn run_flash(app: &App, port: &str) {
    // Holding the source lock keeps a settings change from starting the
    // boards again while the port is in use here.
    let mut source = app.source.lock().unwrap();
    *source = None; // stops all boards: frees the port
    app.core.lock().unwrap().boards_started(Vec::new());

    let mut last_sent = Instant::now() - Duration::from_secs(1);
    let mut report = |stage: &str, progress: Option<f64>| {
        // At most 10 messages a second, but every new stage.
        let same_stage = app.flash.lock().unwrap().as_ref().is_some_and(|s| s.stage == stage);
        if same_stage && progress != Some(1.0) && last_sent.elapsed() < Duration::from_millis(100) {
            return;
        }
        last_sent = Instant::now();
        set_flash(app, stage, progress, false, None);
    };
    let result = flash::flash(port, &app.firmware_dir, &mut report);

    // The board restarts and comes back on USB.
    report("Boards wieder starten …", None);
    thread::sleep(Duration::from_secs(2));
    let restarted = start_source(app, &mut source);
    drop(source);

    let error = match (result, restarted) {
        (Err(e), _) => Some(format!("{e:#}")),
        (Ok(_), Err(e)) => Some(format!("Geflasht, aber die Boards starten nicht: {e:#}")),
        (Ok(version), Ok(())) => {
            let info = app
                .core
                .lock()
                .unwrap()
                .boards
                .iter()
                .find(|b| b.port == port)
                .map(|b| b.info.clone());
            match info {
                Some(i) if i.split('|').nth(3) == Some(version.as_str()) => None,
                Some(i) => Some(format!("Geflasht, aber das Board meldet eine andere Version: {i}")),
                None => Some("Geflasht, aber das Board meldet sich nicht. RESET drücken.".into()),
            }
        }
    };
    let stage = if error.is_none() { "Fertig" } else { "Fehlgeschlagen" };
    set_flash(app, stage, None, true, error);
}

fn set_flash(app: &App, stage: &str, progress: Option<f64>, done: bool, error: Option<String>) {
    let mut status = app.flash.lock().unwrap();
    let Some(s) = status.as_mut() else { return };
    s.stage = stage.to_string();
    s.progress = progress;
    s.done = done;
    s.error = error;
    let mut msg = serde_json::to_value(&*s).unwrap_or_default();
    msg["type"] = json!("flash");
    let _ = app.tx.send(msg.to_string());
}

/// Plays a sample announcement (settings page, "Probe").
async fn announce_test(State(app): State<Arc<App>>) -> Json<Value> {
    app.core.lock().unwrap().announce_test();
    Json(json!({}))
}

/// The WebSocket: first the whole state, then every message the core sends.
async fn websocket(State(app): State<Arc<App>>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| forward(app, socket))
}

async fn forward(app: Arc<App>, mut socket: WebSocket) {
    // Subscribe before reading the state, so nothing falls in between.
    let mut rx = app.tx.subscribe();
    let state = app.core.lock().unwrap().state_json().to_string();
    if socket.send(Message::Text(state.into())).await.is_err() {
        return;
    }
    loop {
        match rx.recv().await {
            Ok(msg) => {
                if socket.send(Message::Text(msg.into())).await.is_err() {
                    return; // the browser went away
                }
            }
            // Too slow to keep up: skip what was missed, send the whole state.
            Err(broadcast::error::RecvError::Lagged(_)) => {
                let state = app.core.lock().unwrap().state_json().to_string();
                if socket.send(Message::Text(state.into())).await.is_err() {
                    return;
                }
            }
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}
