//! `laptimer serve`: the web server. A REST API for actions, a WebSocket for
//! everything that changes, and the web page (`frontend/dist`).
//!
//! Threads:
//! - the board threads (`esp/board.rs`) send levels into a std channel;
//! - the "core" thread takes them, feeds `Core` and calls `Core::tick`;
//! - tokio runs the web server; each request locks `Core` briefly.
//!
//! `Core` sits in a `std::sync::Mutex`: never hold it across an `.await`.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tower_http::services::{ServeDir, ServeFile};

use crate::core::{BoardView, Core, Settings, results};
use crate::db::Db;
use crate::esp::board::{Event, ScanConfig};
use crate::sound::Sounds;
use crate::source::{self, Source};

pub struct ServeOptions {
    pub port: u16,
    pub data_dir: PathBuf,
    pub web_dir: PathBuf,
    pub sounds_dir: PathBuf,
    /// Play this recording instead of reading boards.
    pub replay: Option<PathBuf>,
}

/// Shared by all request handlers (axum's "state").
struct App {
    core: Mutex<Core>,
    tx: broadcast::Sender<String>,
    /// The boards (or the replay); replaced when channels or gain change.
    source: Mutex<Option<Source>>,
    events: Sender<Event>,
    replay: Option<PathBuf>,
}

pub fn run(opts: ServeOptions) -> Result<()> {
    std::fs::create_dir_all(&opts.data_dir)?;
    let db = Db::open(&opts.data_dir.join("laptimer.db").to_string_lossy())?;
    let (tx, _) = broadcast::channel(256);
    let (events, events_rx) = mpsc::channel();
    let origin = Instant::now();
    let core = Core::new(
        db,
        opts.data_dir.clone(),
        origin,
        Sounds::new(opts.sounds_dir.clone()),
        tx.clone(),
    );
    let app = Arc::new(App {
        core: Mutex::new(core),
        tx,
        source: Mutex::new(None),
        events,
        replay: opts.replay.clone(),
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
        .route("/sessions/{id}/passes", post(add_pass))
        .route("/passes/{id}", post(set_pass_deleted))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/pilots", post(add_pilot))
        .route("/pilots/{id}", axum::routing::put(rename_pilot).delete(delete_pilot));
    let web = ServeDir::new(&opts.web_dir).fallback(ServeFile::new(opts.web_dir.join("index.html")));
    let router = Router::new()
        .nest("/api", api)
        .route("/ws", get(websocket))
        .fallback_service(web)
        .with_state(app);

    let addr = SocketAddr::from(([0, 0, 0, 0], opts.port));
    println!(
        "laptimer: http://localhost:{} (and this machine's address in the network)",
        opts.port
    );
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .with_context(|| format!("port {}", opts.port))?;
        axum::serve(listener, router).await?;
        anyhow::Ok(())
    })
}

/// Stops the boards (or replay) and starts them with the current settings.
/// Talks to the boards, so it takes a moment: not on a tokio thread.
fn restart_source(app: &App) -> Result<()> {
    let mut source = app.source.lock().unwrap();
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
        .map(|(port, info)| BoardView::new(port, info))
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
    let s = core.db.session(id)?.context("no such session")?;
    let passes = core.db.passes(id)?;
    Ok(Json(
        json!({ "session": s, "results": results(&s, &passes), "passes": passes }),
    ))
}

async fn delete_session(State(app): State<Arc<App>>, Path(id): Path<i64>) -> ApiResult {
    let core = app.core.lock().unwrap();
    let s = core.db.session(id)?.context("no such session")?;
    if s.ended_us.is_none() {
        return Err(anyhow::anyhow!("the session is still running").into());
    }
    if let Some(f) = &s.levels_file {
        let _ = std::fs::remove_file(f);
    }
    core.db.delete_session(id)?;
    let _ = app.tx.send(json!({ "type": "session_changed", "id": id }).to_string());
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
struct AddPass {
    channel: u8,
    time_us: i64,
}

/// A pass the detection missed, added by hand.
async fn add_pass(State(app): State<Arc<App>>, Path(id): Path<i64>, Json(req): Json<AddPass>) -> ApiResult {
    let mut core = app.core.lock().unwrap();
    let pass = core.db.add_pass(id, req.channel, req.time_us, 0.0, true)?;
    core.session_changed(id)?;
    Ok(Json(json!({ "id": pass })))
}

#[derive(Deserialize)]
struct SetDeleted {
    deleted: bool,
}

async fn set_pass_deleted(State(app): State<Arc<App>>, Path(id): Path<i64>, Json(req): Json<SetDeleted>) -> ApiResult {
    let mut core = app.core.lock().unwrap();
    let session = core.db.set_pass_deleted(id, req.deleted)?.context("no such pass")?;
    core.session_changed(session)?;
    Ok(Json(json!({})))
}

async fn get_settings(State(app): State<Arc<App>>) -> ApiResult {
    Ok(Json(serde_json::to_value(&app.core.lock().unwrap().settings)?))
}

async fn put_settings(State(app): State<Arc<App>>, Json(s): Json<Settings>) -> ApiResult {
    if s.channels.is_empty() || s.channels.len() > 8 {
        return Err(anyhow::anyhow!("1 to 8 channels").into());
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
struct PilotName {
    name: String,
}

async fn add_pilot(State(app): State<Arc<App>>, Json(req): Json<PilotName>) -> ApiResult {
    let core = app.core.lock().unwrap();
    let id = core.db.add_pilot(req.name.trim())?;
    core.broadcast_state();
    Ok(Json(json!({ "id": id })))
}

async fn rename_pilot(State(app): State<Arc<App>>, Path(id): Path<i64>, Json(req): Json<PilotName>) -> ApiResult {
    let core = app.core.lock().unwrap();
    core.db.rename_pilot(id, req.name.trim())?;
    core.broadcast_state();
    Ok(Json(json!({})))
}

async fn delete_pilot(State(app): State<Arc<App>>, Path(id): Path<i64>) -> ApiResult {
    let core = app.core.lock().unwrap();
    core.db.delete_pilot(id)?;
    core.broadcast_state();
    Ok(Json(json!({})))
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
