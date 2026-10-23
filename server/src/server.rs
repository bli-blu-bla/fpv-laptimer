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
use tower_http::services::{ServeDir, ServeFile};

use crate::api;
use crate::auth::{self, Auth};
use crate::config::AuthConfig;
use crate::core::{BoardView, Core, Settings, results};
use crate::db::{Db, PassEdit};
use crate::detect::DetectConfig;
use crate::esp::board::{Event, ScanConfig, find_ports};
use crate::esp::flash;
use crate::events::Events;
use crate::hooks::Hooks;
use crate::sound::Sounds;
use crate::source::{self, Source};
use crate::stats;
use crate::trace::{self, SessionTrace};

pub struct ServeOptions {
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
    replay: Option<PathBuf>,
    /// Level graphs of sessions: (size of the level file, graph). A file
    /// that grew (the running session) is read again.
    pub traces: Mutex<HashMap<i64, (u64, Arc<SessionTrace>)>>,
    /// The firmware the page offers (`<web>/firmware`, `tools/stage_firmware.py`).
    firmware_dir: PathBuf,
    /// The flash in progress or the last one (`/api/firmware`).
    flash: Mutex<Option<FlashStatus>>,
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

pub fn run(opts: ServeOptions) -> Result<()> {
    std::fs::create_dir_all(&opts.data_dir)?;
    let db = Db::open(&opts.data_dir.join("laptimer.db").to_string_lossy())?;
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
        replay: opts.replay.clone(),
        traces: Mutex::new(HashMap::new()),
        firmware_dir: opts.web_dir.join("firmware"),
        flash: Mutex::new(None),
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
        .route("/passes/{id}", post(set_pass_deleted))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/pilots", post(add_pilot))
        .route("/pilots/{id}", axum::routing::put(update_pilot).delete(delete_pilot))
        .route("/stats", get(get_stats))
        .route("/boards/rescan", post(rescan_boards))
        .route("/firmware", get(get_firmware))
        .route("/firmware/flash", post(flash_board))
        .route("/announce/test", post(announce_test))
        .merge(auth::routes())
        .merge(api::page_routes())
        .merge(crate::data::routes())
        .nest("/v1", api::router(Arc::clone(&app)))
        // Announcements played by the browsers: the clips and the spoken texts.
        .nest_service("/audio/clips", ServeDir::new(&opts.sounds_dir))
        .nest_service("/audio/tts", ServeDir::new(opts.data_dir.join("tts")))
        // Changes need the login (reading stays open).
        .layer(middleware::from_fn_with_state(Arc::clone(&app), auth::require_login));
    let web = ServeDir::new(&opts.web_dir).fallback(ServeFile::new(opts.web_dir.join("index.html")));
    let router = Router::new()
        .nest("/api", api)
        .route("/ws", get(websocket))
        .fallback_service(web)
        .with_state(app);

    let addr = SocketAddr::from(([0, 0, 0, 0], opts.port));
    println!(
        "laptimer {}: http://localhost:{} (and this machine's address in the network)",
        crate::VERSION,
        opts.port
    );
    if !opts.auth.enabled {
        println!("laptimer: ohne Passwort ([auth] enabled = false): jeder im Netz kann alles ändern");
    }
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .with_context(|| format!("port {}", opts.port))?;
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
    let (new, names) = match &app.replay {
        Some(path) => source::start_replay(path, origin, app.events.clone())?,
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
    *source = Some(new);
    let views = names
        .into_iter()
        .map(|n| BoardView::new(n.port, n.id, n.info))
        .collect();
    app.core.lock().unwrap().boards_started(views);
    Ok(())
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
    Ok(Json(
        json!({ "session": s, "results": results(&s, &passes), "passes": passes }),
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

/// A session's level graph, from the cache or computed. Reads a file of up
/// to hundreds of MB: call it off the async threads (`spawn_blocking`); it
/// doesn't hold the core lock while it reads.
pub fn session_trace(app: &App, id: i64) -> Result<Arc<SessionTrace>> {
    let (path, config, fixed) = {
        let mut core = app.core.lock().unwrap();
        let s = core.db.session(id)?.context("Diese Session gibt es nicht.")?;
        core.flush_levels(id)?;
        let path = s
            .levels_file
            .context("Für diese Session sind keine Pegel aufgezeichnet.")?;
        // Fixed thresholds for every possible channel (8); unused ones don't matter.
        let fixed = s
            .config
            .detection
            .as_ref()
            .map(|d| d.fixed(false, 8))
            .unwrap_or_default();
        (
            PathBuf::from(path),
            DetectConfig {
                min_lap_us: (s.config.min_lap_s * 1e6) as i64,
                min_rise_db: s
                    .config
                    .detection
                    .as_ref()
                    .map_or(crate::detect::MIN_RISE_DB, |d| d.min_rise_db),
            },
            fixed,
        )
    };
    let size = std::fs::metadata(&path)
        .with_context(|| format!("{}", path.display()))?
        .len();
    if let Some((cached_size, t)) = app.traces.lock().unwrap().get(&id)
        && *cached_size == size
    {
        return Ok(Arc::clone(t));
    }
    let t = Arc::new(trace::load(&path, config, fixed)?);
    let mut traces = app.traces.lock().unwrap();
    if traces.len() >= 4 {
        traces.clear(); // keep the memory small; the next request reads again
    }
    traces.insert(id, (size, Arc::clone(&t)));
    Ok(t)
}

#[derive(Deserialize)]
struct GraphQuery {
    channel: usize,
    /// Wall clock µs; missing: from the start / to the end of the session.
    from_us: Option<i64>,
    to_us: Option<i64>,
    /// At most this many points (about the width in pixels).
    points: Option<usize>,
}

async fn get_graph(State(app): State<Arc<App>>, Path(id): Path<i64>, Query(q): Query<GraphQuery>) -> ApiResult {
    let t = tokio::task::spawn_blocking(move || session_trace(&app, id)).await??;
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
    check_room(&core, id, req.channel, time_us, None)?;
    let pass = core.db.add_pass(id, req.channel, time_us, peak_db, true)?;
    core.session_changed(id)?;
    Ok(Json(json!({ "id": pass, "time_us": time_us })))
}

/// A pass closer than the minimum lap to another (not deleted) pass of the
/// same channel would make a lap that can't be: refuse it.
pub fn check_room(core: &Core, session: i64, channel: u8, time_us: i64, except: Option<i64>) -> Result<()> {
    let s = core.db.session(session)?.context("Diese Session gibt es nicht.")?;
    let min_lap_us = (s.config.min_lap_s * 1e6) as i64;
    let clash = core.db.passes(session)?.into_iter().find(|p| {
        p.channel == channel
            && p.gate == 0
            && !p.deleted
            && Some(p.id) != except
            && (p.time_us - time_us).abs() < min_lap_us
    });
    if clash.is_some() {
        anyhow::bail!(
            "Hier ist schon ein Durchflug (näher als die Mindest-Rundenzeit von {} s). Erst den anderen löschen.",
            s.config.min_lap_s
        );
    }
    Ok(())
}

#[derive(Deserialize)]
struct EditRequest {
    channel: u8,
    #[serde(flatten)]
    edit: PassEdit,
}

/// Corrections of one channel from the pass editor, saved at once. The
/// result must keep the minimum lap between any two passes.
async fn edit_passes(State(app): State<Arc<App>>, Path(id): Path<i64>, Json(req): Json<EditRequest>) -> ApiResult {
    let mut core = app.core.lock().unwrap();
    let s = core.db.session(id)?.context("Diese Session gibt es nicht.")?;
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
    let min_lap_us = (s.config.min_lap_s * 1e6) as i64;
    if let Some(w) = times.windows(2).find(|w| w[1] - w[0] < min_lap_us) {
        return Err(anyhow::anyhow!(
            "Zwei Durchflüge liegen nur {:.2} s auseinander – die Mindest-Rundenzeit ist {} s.",
            (w[1] - w[0]) as f64 / 1e6,
            s.config.min_lap_s
        )
        .into());
    }
    core.db.edit_passes(id, req.channel, e)?;
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
        check_room(&core, session, p.channel, p.time_us, Some(id))?;
    }
    let session = core
        .db
        .set_pass_deleted(id, req.deleted)?
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
    let ports = if app.replay.is_some() {
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
            json!({ "port": p.name, "id": p.id, "info": board.map(|b| b.info.clone()) })
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

async fn flash_board(State(app): State<Arc<App>>, Json(req): Json<FlashRequest>) -> ApiResult {
    if app.replay.is_some() {
        return Err(anyhow::anyhow!("Im Wiedergabe-Modus gibt es keine Boards.").into());
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
