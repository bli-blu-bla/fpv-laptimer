//! The public API (`/api/v1`) for other programs: channels and pilots,
//! starting and stopping sessions, confirming results, and every event
//! (`events.rs`) over a WebSocket. Documented in `docs/API.md`.
//!
//! Every action is a command with a name and JSON arguments (`command`).
//! The same command can be sent two ways:
//! - HTTP: `POST /api/v1/session/start {"race": true}` (the REST routes
//!   below), or `POST /api/v1/cmd/session.start` with the arguments as body;
//! - WebSocket `/api/v1/ws`: `{"id": 1, "cmd": "session.start", "args": {"race": true}}`,
//!   answered by `{"type": "reply", "id": 1, "ok": true, "data": …}`.
//!
//! Everything under `/api/v1` needs an API key: header `Authorization:
//! Bearer <key>` or `X-API-Key: <key>`, or `?key=<key>` (for WebSockets
//! from a browser, which can't set headers). Keys are made on the API page;
//! only their sha256 is stored. (The web page itself uses `/api` without a
//! key, with the login of `auth.rs` instead.)
//!
//! Commands run in `spawn_blocking`: they lock `Core` (a std mutex) and may
//! talk to the boards (a new channel restarts them).

use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

use crate::auth::{hex, random_bytes};
use crate::core::{ChannelSetting, Core, standings, unix_now_us};
use crate::db::SessionRow;
use crate::laps::{EndRule, StartMode};
use crate::server::{App, check_room, restart_source, session_results, stats_json};

/// The version in the path and in the WebSocket's `hello`.
const API_VERSION: u32 = 1;

/// Raceband, for channels given by name ("R3").
const RACEBAND: [u32; 8] = [5658, 5695, 5732, 5769, 5806, 5843, 5880, 5917];

// --- routes ---

/// `/api/v1`: the REST routes, the generic `/cmd/{name}` and the WebSocket,
/// all behind the key check.
pub fn router(app: Arc<App>) -> Router<Arc<App>> {
    // `a` = the arguments from the JSON body (missing: none).
    Router::new()
        .route("/state", get(|s: St| call(s, "state", json!({}))))
        .route("/channels", get(|s: St| call(s, "channels.list", json!({}))))
        .route(
            "/channels/assign",
            post(|s: St, b: Body| call(s, "channels.assign", args(b))),
        )
        .route(
            "/channels/{index}",
            patch(|s: St, Path(i): Path<i64>, b: Body| call(s, "channels.set", with(args(b), "index", i))),
        )
        .route(
            "/pilots",
            get(|s: St| call(s, "pilots.list", json!({}))).post(|s: St, b: Body| call(s, "pilots.create", args(b))),
        )
        .route(
            "/pilots/{id}",
            get(|s: St, Path(id): Path<i64>| call(s, "pilots.get", json!({ "id": id })))
                .patch(|s: St, Path(id): Path<i64>, b: Body| call(s, "pilots.update", with(args(b), "id", id)))
                .delete(|s: St, Path(id): Path<i64>| call(s, "pilots.delete", json!({ "id": id }))),
        )
        .route(
            "/session/start",
            post(|s: St, b: Body| call(s, "session.start", args(b))),
        )
        .route("/session/stop", post(|s: St| call(s, "session.stop", json!({}))))
        .route("/session/cancel", post(|s: St| call(s, "session.cancel", json!({}))))
        .route(
            "/session/confirm",
            post(|s: St, b: Body| call(s, "session.confirm", args(b))),
        )
        .route(
            "/session/discard",
            post(|s: St, b: Body| call(s, "session.discard", args(b))),
        )
        .route(
            "/sessions",
            get(|s: St, Query(q): Query<LimitQuery>| call(s, "sessions.list", json!({ "limit": q.limit }))),
        )
        .route(
            "/sessions/{id}",
            get(|s: St, Path(id): Path<i64>| call(s, "sessions.get", json!({ "id": id }))),
        )
        .route("/passes", post(|s: St, b: Body| call(s, "passes.add", args(b))))
        .route(
            "/passes/{id}",
            delete(|s: St, Path(id): Path<i64>| call(s, "passes.delete", json!({ "id": id }))),
        )
        .route(
            "/passes/{id}/restore",
            post(|s: St, Path(id): Path<i64>| call(s, "passes.restore", json!({ "id": id }))),
        )
        .route("/stats", get(|s: St| call(s, "stats", json!({}))))
        .route(
            "/cmd/{name}",
            post(|s: St, Path(name): Path<String>, b: Body| call_named(s, name, args(b))),
        )
        .route("/ws", get(websocket))
        .layer(middleware::from_fn_with_state(app, check_key))
}

/// For the web page (no key, but a login: `auth.rs`): API keys and hooks.
pub fn page_routes() -> Router<Arc<App>> {
    Router::new()
        .route("/keys", get(list_keys).post(create_key))
        .route("/keys/{id}", delete(delete_key))
        .route("/hooks", get(get_hooks).post(create_hook))
        .route("/hooks/{id}", put(update_hook).delete(delete_hook))
        .route("/hooks/{id}/test", post(test_hook))
}

type St = State<Arc<App>>;

#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<i64>,
}
type Body = Option<Json<Value>>;

fn args(body: Body) -> Value {
    body.map_or(json!({}), |Json(v)| v)
}

/// The arguments with one more (a value from the path).
fn with(mut args: Value, key: &str, value: i64) -> Value {
    if let Some(obj) = args.as_object_mut() {
        obj.insert(key.to_string(), json!(value));
    }
    args
}

async fn call(State(app): St, cmd: &'static str, args: Value) -> Response {
    call_named(State(app), cmd.to_string(), args).await
}

/// Runs a command; its result is the response (200), an error becomes
/// 400 `{"error": "…"}` (404 for an unknown command).
async fn call_named(State(app): St, cmd: String, args: Value) -> Response {
    let known = COMMANDS.contains(&cmd.as_str());
    match tokio::task::spawn_blocking(move || command(&app, &cmd, args)).await {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => {
            let status = if known {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::NOT_FOUND
            };
            (status, Json(json!({ "error": format!("{e:#}") }))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// --- keys ---

/// The key from the header or the query; checked against the database.
async fn check_key(State(app): St, req: Request, next: Next) -> Response {
    let key = key_of(req.headers(), req.uri().query());
    let ok = match key {
        Some(k) => {
            let app = Arc::clone(&app);
            tokio::task::spawn_blocking(move || key_valid(&app, &k))
                .await
                .unwrap_or(false)
        }
        None => false,
    };
    if !ok {
        let error = "API-Schlüssel fehlt oder ist ungültig (Header 'Authorization: Bearer <key>' oder ?key=<key>).";
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": error }))).into_response();
    }
    next.run(req).await
}

fn key_of(headers: &HeaderMap, query: Option<&str>) -> Option<String> {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::trim);
    if let Some(v) = header("authorization")
        && let Some(k) = v.strip_prefix("Bearer ")
    {
        return Some(k.trim().to_string());
    }
    if let Some(k) = header("x-api-key") {
        return Some(k.to_string());
    }
    // Keys are plain letters and digits: no URL decoding needed.
    query?
        .split('&')
        .find_map(|kv| kv.strip_prefix("key="))
        .map(str::to_string)
}

fn key_valid(app: &App, key: &str) -> bool {
    let core = app.core.lock().unwrap();
    match core.db.api_key_by_hash(&hash_key(key)) {
        Ok(Some((id, last))) => {
            // "Last used", but not a write for every single request.
            let now = unix_now_us();
            if last.is_none_or(|t| now - t > 60_000_000) {
                let _ = core.db.touch_api_key(id, now);
            }
            true
        }
        _ => false,
    }
}

fn hash_key(key: &str) -> String {
    hex(&Sha256::digest(key.as_bytes()))
}

/// A new random key: "lt_" and 40 hex digits (160 bits).
fn new_key() -> Result<String> {
    Ok(format!("lt_{}", hex(&random_bytes::<20>()?)))
}

async fn list_keys(State(app): St) -> Response {
    match app.core.lock().unwrap().db.api_keys() {
        Ok(keys) => Json(json!(keys)).into_response(),
        Err(e) => bad(e),
    }
}

#[derive(Deserialize)]
struct NewKey {
    name: String,
}

/// Makes a key; the answer is the only time the key itself is shown.
async fn create_key(State(app): St, Json(req): Json<NewKey>) -> Response {
    let result = (|| -> Result<Value> {
        let name = req.name.trim();
        if name.is_empty() {
            bail!("Der Name fehlt (wofür ist der Schlüssel?).");
        }
        let key = new_key()?;
        let prefix = &key[..10];
        let id = app
            .core
            .lock()
            .unwrap()
            .db
            .add_api_key(name, prefix, &hash_key(&key), unix_now_us())?;
        Ok(json!({ "id": id, "key": key, "prefix": prefix }))
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => bad(e),
    }
}

async fn delete_key(State(app): St, Path(id): Path<i64>) -> Response {
    match app.core.lock().unwrap().db.delete_api_key(id) {
        Ok(()) => Json(json!({})).into_response(),
        Err(e) => bad(e),
    }
}

fn bad(e: anyhow::Error) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("{e:#}") }))).into_response()
}

// --- hooks (for the page) ---

async fn get_hooks(State(app): St) -> Response {
    let hooks = app.core.lock().unwrap().db.hooks();
    match hooks {
        Ok(list) => Json(json!({
            "dir": app.hooks.dir.canonicalize().unwrap_or(app.hooks.dir.clone()),
            "scripts": app.hooks.scripts(),
            "hooks": list,
            "runs": app.hooks.runs(),
            "events": crate::events::EVENT_NAMES,
        }))
        .into_response(),
        Err(e) => bad(e),
    }
}

/// Checks a hook from the page; Ok: it can be stored.
fn check_hook(app: &App, hook: &mut crate::db::Hook) -> Result<()> {
    hook.args = hook.args.trim().to_string();
    if hook.event != "*" && !crate::events::EVENT_NAMES.contains(&hook.event.as_str()) {
        bail!("Event „{}“ gibt es nicht.", hook.event);
    }
    app.hooks.check_script(&hook.script)
}

/// Stores a change (`change` gets the database) and hands the hooks' new
/// list to the hooks' thread.
fn change_hooks(app: &App, change: impl FnOnce(&crate::db::Db) -> Result<Value>) -> Response {
    let result = (|| {
        let core = app.core.lock().unwrap();
        let value = change(&core.db)?;
        app.hooks.set_list(core.db.hooks()?);
        Ok(value)
    })();
    match result {
        Ok(v) => Json(v).into_response(),
        Err(e) => bad(e),
    }
}

async fn create_hook(State(app): St, Json(mut hook): Json<crate::db::Hook>) -> Response {
    if let Err(e) = check_hook(&app, &mut hook) {
        return bad(e);
    }
    change_hooks(&app, |db| Ok(json!({ "id": db.add_hook(&hook)? })))
}

async fn update_hook(State(app): St, Path(id): Path<i64>, Json(mut hook): Json<crate::db::Hook>) -> Response {
    hook.id = id;
    if let Err(e) = check_hook(&app, &mut hook) {
        return bad(e);
    }
    change_hooks(&app, |db| {
        db.update_hook(&hook)?;
        Ok(json!({}))
    })
}

async fn delete_hook(State(app): St, Path(id): Path<i64>) -> Response {
    change_hooks(&app, |db| {
        db.delete_hook(id)?;
        Ok(json!({}))
    })
}

/// Runs one hook now, with `LAPTIMER_TEST=1`.
async fn test_hook(State(app): St, Path(id): Path<i64>) -> Response {
    let hooks = app.core.lock().unwrap().db.hooks();
    match hooks.map(|list| list.into_iter().find(|h| h.id == id)) {
        Ok(Some(hook)) => {
            app.hooks.test(hook);
            Json(json!({})).into_response()
        }
        Ok(None) => bad(anyhow!("Hook {id} gibt es nicht.")),
        Err(e) => bad(e),
    }
}

// --- WebSocket ---

async fn websocket(State(app): St, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| ws_session(app, socket))
}

/// One client: first `hello` with the state, then every event; commands
/// coming in are answered in between (one after the other).
async fn ws_session(app: Arc<App>, mut socket: WebSocket) {
    // Subscribe before reading the state, so no event falls in between.
    let mut events = app.api_tx.subscribe();
    let app2 = Arc::clone(&app);
    let state = tokio::task::spawn_blocking(move || command(&app2, "state", json!({})))
        .await
        .ok()
        .and_then(|r| r.ok());
    let hello =
        json!({ "type": "hello", "api": API_VERSION, "version": crate::VERSION, "state": state, "commands": COMMANDS });
    if socket.send(Message::Text(hello.to_string().into())).await.is_err() {
        return;
    }
    loop {
        // Whichever comes first: a message from the client or an event.
        tokio::select! {
            msg = socket.recv() => {
                let text = match msg {
                    Some(Ok(Message::Text(t))) => t.to_string(),
                    // The library has queued the answer to the client's Close;
                    // it goes out on the next read, which then ends with None.
                    // (Returning at once: the client sees 1006, "abnormal".)
                    Some(Ok(Message::Close(_))) => continue,
                    Some(Err(_)) | None => return,
                    Some(Ok(_)) => continue, // ping/pong/binary
                };
                let reply = ws_command(&app, &text).await;
                if socket.send(Message::Text(reply.to_string().into())).await.is_err() {
                    return;
                }
            }
            ev = events.recv() => {
                let text = match ev {
                    Ok(t) => t,
                    // Too slow: say how many events were lost.
                    Err(broadcast::error::RecvError::Lagged(n)) => json!({ "type": "lagged", "missed": n }).to_string(),
                    Err(broadcast::error::RecvError::Closed) => return,
                };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    return;
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct WsCommand {
    #[serde(default)]
    id: Value,
    cmd: String,
    #[serde(default)]
    args: Value,
}

async fn ws_command(app: &Arc<App>, text: &str) -> Value {
    let req: WsCommand = match serde_json::from_str(text) {
        Ok(r) => r,
        Err(e) => {
            let error = format!("Erwartet {{\"id\": …, \"cmd\": \"…\", \"args\": {{…}}}}: {e}");
            return json!({ "type": "reply", "id": null, "ok": false, "status": 400, "error": error });
        }
    };
    let app = Arc::clone(app);
    let args = if req.args.is_null() { json!({}) } else { req.args };
    let cmd = req.cmd;
    // The same status numbers as over HTTP: 404 unknown command, 400 refused.
    let status = if COMMANDS.contains(&cmd.as_str()) { 400 } else { 404 };
    match tokio::task::spawn_blocking(move || command(&app, &cmd, args)).await {
        Ok(Ok(data)) => json!({ "type": "reply", "id": req.id, "ok": true, "data": data }),
        Ok(Err(e)) => json!({ "type": "reply", "id": req.id, "ok": false, "status": status, "error": format!("{e:#}") }),
        Err(e) => json!({ "type": "reply", "id": req.id, "ok": false, "status": 500, "error": e.to_string() }),
    }
}

// --- the commands ---

/// All command names (the WebSocket's `hello` lists them).
pub const COMMANDS: &[&str] = &[
    "ping",
    "state",
    "channels.list",
    "channels.set",
    "channels.assign",
    "pilots.list",
    "pilots.get",
    "pilots.create",
    "pilots.update",
    "pilots.delete",
    "session.start",
    "session.stop",
    "session.cancel",
    "session.confirm",
    "session.discard",
    "sessions.list",
    "sessions.get",
    "passes.add",
    "passes.delete",
    "passes.restore",
    "stats",
];

/// Runs one command. Blocks (core lock, maybe the boards): not on an async thread.
pub fn command(app: &App, cmd: &str, args: Value) -> Result<Value> {
    if !args.is_object() {
        bail!("Die Argumente müssen ein JSON-Objekt sein.");
    }
    match cmd {
        "ping" => Ok(json!({ "pong": true, "now_us": app.core.lock().unwrap().now_us() })),
        "state" => state(&app.core.lock().unwrap()),
        "channels.list" => Ok(json!(app.core.lock().unwrap().channel_list())),
        "channels.set" => set_channel(app, &args),
        "channels.assign" => assign_channels(app, &args),
        "pilots.list" => Ok(json!(app.core.lock().unwrap().db.pilots()?)),
        "pilots.get" => Ok(json!(app.core.lock().unwrap().pilot(int(&args, "id")?)?)),
        "pilots.create" => {
            let name = text(&args, "name")?.context("'name' (der Callsign) fehlt.")?;
            let full_name = text(&args, "full_name")?.unwrap_or_default();
            let mut core = app.core.lock().unwrap();
            let id = core.add_pilot(&name, &full_name)?;
            Ok(json!(core.pilot(id)?))
        }
        "pilots.update" => {
            let id = int(&args, "id")?;
            let mut core = app.core.lock().unwrap();
            let p = core.pilot(id)?;
            let name = text(&args, "name")?.unwrap_or(p.name);
            let full_name = text(&args, "full_name")?.unwrap_or(p.full_name);
            core.update_pilot(id, &name, &full_name)?;
            Ok(json!(core.pilot(id)?))
        }
        "pilots.delete" => {
            app.core.lock().unwrap().delete_pilot(int(&args, "id")?)?;
            Ok(json!({}))
        }
        "session.start" => start(app, &args),
        "session.stop" => {
            let mut core = app.core.lock().unwrap();
            let id = core.active_id().context("Es läuft keine Session.")?;
            core.stop_session()?;
            Ok(json!({ "session_id": id }))
        }
        "session.cancel" => {
            let mut core = app.core.lock().unwrap();
            let id = core.active_id().context("Es läuft keine Session.")?;
            core.end_session("cancelled")?;
            core.delete_session(id)?;
            Ok(json!({ "session_id": id }))
        }
        "session.confirm" => {
            let mut core = app.core.lock().unwrap();
            let id = ended_session(&core, &args)?;
            core.confirm_session(id)?;
            Ok(json!({ "session_id": id }))
        }
        "session.discard" => {
            let mut core = app.core.lock().unwrap();
            let id = ended_session(&core, &args)?;
            core.delete_session(id)?;
            Ok(json!({ "session_id": id }))
        }
        "sessions.list" => {
            let limit = opt_int(&args, "limit")?.unwrap_or(20).clamp(1, 500) as u32;
            let core = app.core.lock().unwrap();
            let mut out = Vec::new();
            for s in core.db.sessions(limit)? {
                out.push(session_json(&core, &s, false)?);
            }
            Ok(json!(out))
        }
        "sessions.get" => {
            let core = app.core.lock().unwrap();
            let s = core
                .db
                .session(int(&args, "id")?)?
                .context("Diese Session gibt es nicht.")?;
            session_json(&core, &s, true)
        }
        "passes.add" => add_pass(app, &args),
        "passes.delete" | "passes.restore" => {
            let id = int(&args, "id")?;
            let mut core = app.core.lock().unwrap();
            let session = core.db.pass_session(id)?.context("Diesen Durchflug gibt es nicht.")?;
            let restore = cmd == "passes.restore";
            if restore {
                let p = core
                    .db
                    .passes(session)?
                    .into_iter()
                    .find(|p| p.id == id)
                    .context("Diesen Durchflug gibt es nicht.")?;
                check_room(&core, session, p.channel, p.time_us, Some(id))?;
            }
            core.db.set_pass_deleted(id, !restore)?;
            core.session_changed(session)?;
            Ok(json!({ "session_id": session }))
        }
        "stats" => stats_json(&app.core.lock().unwrap()),
        _ => bail!("Unbekannter Befehl '{cmd}'. Bekannt: {}", COMMANDS.join(", ")),
    }
}

// --- reading the arguments ---

fn int(args: &Value, key: &str) -> Result<i64> {
    opt_int(args, key)?.with_context(|| format!("'{key}' fehlt."))
}

fn opt_int(args: &Value, key: &str) -> Result<Option<i64>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_i64()
            .or_else(|| v.as_f64().map(|f| f.round() as i64))
            .map(Some)
            .with_context(|| format!("'{key}' muss eine Zahl sein.")),
    }
}

fn text(args: &Value, key: &str) -> Result<Option<String>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.trim().to_string())),
        Some(_) => bail!("'{key}' muss ein Text sein."),
    }
}

fn flag(args: &Value, key: &str) -> Result<Option<bool>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => bail!("'{key}' muss true oder false sein."),
    }
}

// --- the commands' parts ---

/// The whole picture: running session, the race waiting for confirmation,
/// channels, pilots, boards.
fn state(core: &Core) -> Result<Value> {
    let session = match core.active_id().map(|id| core.db.session(id)).transpose()?.flatten() {
        Some(s) => Some(session_json(core, &s, false)?),
        None => None,
    };
    let unconfirmed = match core
        .db
        .unconfirmed_race()?
        .map(|id| core.db.session(id))
        .transpose()?
        .flatten()
    {
        Some(s) => Some(session_json(core, &s, false)?),
        None => None,
    };
    let boards: Vec<Value> = core
        .boards
        .iter()
        .map(|b| {
            json!({
                "id": b.id,
                "port": b.port,
                "role": b.role,
                "ok": b.failed.is_none(),
                "error": b.failed,
                "levels_per_s": b.levels_per_s,
            })
        })
        .collect();
    Ok(json!({
        "now_us": core.now_us(),
        "session": session,
        "unconfirmed": unconfirmed,
        "channels": core.channel_list(),
        "pilots": core.db.pilots()?,
        "boards": boards,
    }))
}

/// A session for the API. `phase`: "countdown", "running" or "ended";
/// `passes`: also every pass (sessions.get).
fn session_json(core: &Core, s: &SessionRow, with_passes: bool) -> Result<Value> {
    let passes = core.db.passes(s.id)?;
    let results = session_results(s, &passes);
    let now = core.now_us();
    let phase = match s.ended_us {
        Some(_) => "ended",
        None if now < s.start_us => "countdown",
        None => "running",
    };
    let (end, end_value) = match s.config.end_rule {
        EndRule::Open => ("open", None),
        EndRule::Laps(n) => ("laps", Some(n)),
        EndRule::Seconds(n) => ("seconds", Some(n)),
    };
    let mut v = json!({
        "id": s.id,
        "race": s.config.race,
        "phase": phase,
        "start_mode": s.config.start_mode,
        "end": end,
        "end_value": end_value,
        "countdown_s": s.config.countdown_s,
        "min_lap_s": s.config.min_lap_s,
        "created_us": s.created_us,
        "start_us": s.start_us,
        "ended_us": s.ended_us,
        "confirmed_us": s.confirmed_us,
        "needs_confirmation": s.config.race && s.ended_us.is_some() && s.confirmed_us.is_none(),
        "results": standings(s, &results),
    });
    if with_passes {
        v["passes"] = json!(passes);
    }
    Ok(v)
}

/// The ended session the arguments mean: `id`, or else the race waiting
/// for confirmation.
fn ended_session(core: &Core, args: &Value) -> Result<i64> {
    match opt_int(args, "id")? {
        Some(id) => Ok(id),
        None => core
            .db
            .unconfirmed_race()?
            .context("Kein Rennen wartet auf Bestätigung (oder 'id' angeben)."),
    }
}

/// session.start: `race` (default false), for a race optionally `laps` or
/// `seconds` (else open), `countdown_s`, `start_mode`; they are saved as the
/// race format (the page shows them too).
fn start(app: &App, args: &Value) -> Result<Value> {
    let race = flag(args, "race")?.unwrap_or(false);
    let mut core = app.core.lock().unwrap();
    if race {
        let mut s = core.settings.clone();
        let laps = opt_int(args, "laps")?;
        let secs = opt_int(args, "seconds")?;
        match (laps, secs) {
            (Some(_), Some(_)) => bail!("Entweder 'laps' oder 'seconds', nicht beides."),
            (Some(n), None) if (1..=100).contains(&n) => s.race.end_rule = EndRule::Laps(n as u32),
            (None, Some(n)) if (10..=3600).contains(&n) => s.race.end_rule = EndRule::Seconds(n as u32),
            (Some(_), None) => bail!("'laps': 1 bis 100."),
            (None, Some(_)) => bail!("'seconds': 10 bis 3600."),
            (None, None) => {}
        }
        if let Some(c) = opt_int(args, "countdown_s")? {
            if !(0..=60).contains(&c) {
                bail!("'countdown_s': 0 bis 60.");
            }
            s.race.countdown_s = c as u32;
        }
        if let Some(m) = args.get("start_mode").filter(|v| !v.is_null()) {
            s.race.start_mode = serde_json::from_value::<StartMode>(m.clone())
                .context("'start_mode': holeshot, from_start oder flying.")?;
        }
        if s != core.settings {
            core.set_settings(s)?;
        }
    }
    let id = core.start_session(race)?;
    let row = core.db.session(id)?.context("Session fehlt")?;
    session_json(&core, &row, false)
}

/// channels.set: one channel's pilot (`pilot_id` or `pilot` = callsign,
/// null = nobody; `create`: make an unknown callsign a pilot), `label` (free
/// text instead of a pilot), `enabled`, `frequency` ("R3" or MHz). A pilot
/// or frequency used by another channel is swapped with it (as on the page).
fn set_channel(app: &App, args: &Value) -> Result<Value> {
    let index = int(args, "index")? as usize;
    let restart = {
        let mut core = app.core.lock().unwrap();
        let mut s = core.settings.clone();
        if index >= s.channels.len() {
            bail!("Kanal {index} gibt es nicht (0 bis {}).", s.channels.len() - 1);
        }
        s.seats.resize(s.channels.len(), Default::default());
        if let Some(pilot) = pilot_arg(&mut core, args)? {
            set_pilot(&mut s.seats, index, pilot);
        }
        if let Some(label) = text(args, "label")? {
            s.seats[index].label = label;
            if !s.seats[index].label.is_empty() {
                s.seats[index].pilot_id = None;
            }
        }
        if let Some(enabled) = flag(args, "enabled")? {
            s.seats[index].enabled = enabled;
        }
        if let Some(f) = args.get("frequency").filter(|v| !v.is_null()) {
            let ch = frequency(f)?;
            if let Some(other) = s.channels.iter().position(|c| c.mhz == ch.mhz)
                && other != index
            {
                s.channels[other] = s.channels[index].clone();
            }
            s.channels[index] = ch;
        }
        let restart = s.channels != core.settings.channels;
        core.set_settings(s)?;
        restart
    };
    if restart {
        restart_source(app)?;
    }
    Ok(json!(app.core.lock().unwrap().channel_list()[index]))
}

/// channels.assign: `pilots` = one entry per channel (callsign, pilot id or
/// null); `create`: unknown callsigns become pilots. Channels beyond the list
/// stay as they are. A channel with a pilot is switched on.
fn assign_channels(app: &App, args: &Value) -> Result<Value> {
    let list = args
        .get("pilots")
        .and_then(Value::as_array)
        .context("'pilots' fehlt: eine Liste, ein Eintrag pro Kanal (Callsign, Pilot-ID oder null).")?;
    let create = flag(args, "create")?.unwrap_or(false);
    let mut core = app.core.lock().unwrap();
    let mut s = core.settings.clone();
    if list.len() > s.channels.len() {
        bail!("{} Einträge, aber nur {} Kanäle.", list.len(), s.channels.len());
    }
    s.seats.resize(s.channels.len(), Default::default());
    for (i, entry) in list.iter().enumerate() {
        let pilot = match entry {
            Value::Null => None,
            Value::Number(n) => Some(core.pilot(n.as_i64().context("Pilot-ID")?)?.id),
            Value::String(name) => Some(find_pilot(&mut core, name, create)?),
            _ => bail!("Eintrag {i}: Callsign, Pilot-ID oder null."),
        };
        s.seats[i].pilot_id = pilot;
        s.seats[i].label = String::new();
        if pilot.is_some() {
            s.seats[i].enabled = true;
        }
    }
    let ids: Vec<i64> = s.seats.iter().filter_map(|x| x.pilot_id).collect();
    if (1..ids.len()).any(|i| ids[..i].contains(&ids[i])) {
        bail!("Ein Pilot kann nur auf einem Kanal fliegen.");
    }
    core.set_settings(s)?;
    Ok(json!(core.channel_list()))
}

/// The pilot the arguments name: Some(Some(id)), Some(None) for "nobody"
/// (`pilot_id` or `pilot` given as null), None if neither is given.
fn pilot_arg(core: &mut Core, args: &Value) -> Result<Option<Option<i64>>> {
    let create = flag(args, "create")?.unwrap_or(false);
    if let Some(v) = args.get("pilot_id") {
        return match v {
            Value::Null => Ok(Some(None)),
            _ => Ok(Some(Some(core.pilot(int(args, "pilot_id")?)?.id))),
        };
    }
    match args.get("pilot") {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(name)) => Ok(Some(Some(find_pilot(core, name, create)?))),
        Some(_) => bail!("'pilot' muss ein Callsign sein (oder null)."),
    }
}

/// A pilot by callsign (any case); `create`: made if missing.
fn find_pilot(core: &mut Core, name: &str, create: bool) -> Result<i64> {
    let name = name.trim();
    if let Some(p) = core.db.pilots()?.iter().find(|p| p.name.eq_ignore_ascii_case(name)) {
        return Ok(p.id);
    }
    if !create || name.is_empty() {
        bail!("Pilot '{name}' gibt es nicht (mit \"create\": true wird er angelegt).");
    }
    core.add_pilot(name, "")
}

/// Puts a pilot on a seat; the seat that had them gets this seat's pilot.
fn set_pilot(seats: &mut [crate::core::SeatSetting], index: usize, pilot: Option<i64>) {
    if let Some(id) = pilot
        && let Some(other) = seats.iter().position(|x| x.pilot_id == Some(id))
        && other != index
    {
        seats[other].pilot_id = seats[index].pilot_id;
    }
    seats[index].pilot_id = pilot;
    seats[index].label = String::new();
    if pilot.is_some() {
        seats[index].enabled = true;
    }
}

/// "R3" or 5732 → the channel.
fn frequency(v: &Value) -> Result<ChannelSetting> {
    let mhz = match v {
        Value::Number(n) => n.as_u64().context("Frequenz")? as u32,
        Value::String(s) => match s.trim().to_uppercase().strip_prefix('R').map(str::parse::<usize>) {
            Some(Ok(n)) if (1..=8).contains(&n) => RACEBAND[n - 1],
            _ => s.trim().parse().context("'frequency': R1 bis R8 oder MHz.")?,
        },
        _ => bail!("'frequency': R1 bis R8 oder MHz."),
    };
    if !(5300..=6000).contains(&mhz) {
        bail!("'frequency': 5300 bis 6000 MHz.");
    }
    let name = match RACEBAND.iter().position(|&f| f == mhz) {
        Some(i) => format!("R{}", i + 1),
        None => mhz.to_string(),
    };
    Ok(ChannelSetting { name, mhz })
}

/// passes.add: a pass by hand, e.g. from a button. `channel` (index);
/// `time_us` (default: now); `session_id` (default: the running session).
fn add_pass(app: &App, args: &Value) -> Result<Value> {
    let channel = int(args, "channel")?;
    let channel = u8::try_from(channel).context("'channel': die Nummer des Kanals (0, 1, …).")?;
    let time_us = opt_int(args, "time_us")?.unwrap_or_else(unix_now_us);
    let mut core = app.core.lock().unwrap();
    let session = match opt_int(args, "session_id")? {
        Some(id) => id,
        None => core
            .active_id()
            .context("Es läuft keine Session ('session_id' angeben).")?,
    };
    let s = core.db.session(session)?.context("Diese Session gibt es nicht.")?;
    if !s.seats.iter().any(|x| x.channel == channel) {
        bail!("Kanal {channel} fliegt in dieser Session nicht mit.");
    }
    // Between the start button and the end (running: now, with 1 s for
    // a client clock that is a little ahead).
    let latest = s.ended_us.unwrap_or_else(|| core.now_us() + 1_000_000);
    if time_us < s.created_us || time_us > latest {
        bail!("'time_us' liegt außerhalb der Session (vor dem Start oder nach dem Ende bzw. in der Zukunft).");
    }
    check_room(&core, session, channel, time_us, None)?;
    let id = core.db.add_pass(session, channel, time_us, 0.0, true)?;
    core.pass_added(session)?;
    Ok(json!({ "pass_id": id, "session_id": session, "time_us": time_us }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        let k = new_key().unwrap();
        assert!(k.starts_with("lt_") && k.len() == 43);
        assert_ne!(k, new_key().unwrap());
        assert_eq!(hash_key("x").len(), 64);
        let mut h = HeaderMap::new();
        assert_eq!(key_of(&h, Some("a=1&key=lt_abc")), Some("lt_abc".into()));
        h.insert("authorization", "Bearer lt_x ".parse().unwrap());
        assert_eq!(key_of(&h, None), Some("lt_x".into()));
    }

    #[test]
    fn frequencies() {
        assert_eq!(frequency(&json!("r3")).unwrap().mhz, 5732);
        assert_eq!(frequency(&json!(5917)).unwrap().name, "R8");
        assert_eq!(frequency(&json!("5800")).unwrap().name, "5800");
        assert!(frequency(&json!("R9")).is_err());
    }
}
