//! The connection to a server of the user's (Einstellungen → System →
//! Server-Verbindung): the laptimer connects itself, as a client, to a
//! WebSocket URL and speaks the protocol of the API's WebSocket on it
//! (`api.rs`, `docs/api/server-verbindung.md`): first `hello` with the state, then every event;
//! commands coming in are answered with `reply`, unless switched off. For a
//! server that is always on, while the timer only runs at the track.
//!
//! Two optional ways to show who is calling, alone or together: an
//! `Authorization` header in the handshake (as typed, e.g. `Bearer …` or
//! `Basic …`), and a token sent as the very first message, before `hello`:
//! `{"type": "auth", "token": "…"}` (for servers that can't read headers).
//!
//! A lost connection is tried again (2 s, doubling up to 30 s). Events
//! meanwhile are kept in memory for a short while (at most BUFFER_AGE and
//! BUFFER_LEN) and sent after the next `hello`, which says how many follow
//! (`buffered`). Older ones are dropped: this is for live data, not a log.
//!
//! The settings are in the database (key `remote`), apart from the page's
//! settings: those (`/api/settings`, `state`) can be read without a login,
//! the secrets must not. `/api/remote` needs the login for reading too
//! (`auth.rs`) and never returns them, only whether they are set.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{Notify, broadcast};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::{AUTHORIZATION, HeaderValue, USER_AGENT};

use crate::core::unix_now_us;
use crate::server::App;

/// Events kept while there is no connection: at most this old and this many.
const BUFFER_AGE: Duration = Duration::from_secs(5 * 60);
const BUFFER_LEN: usize = 2000;
/// Waiting between attempts: from MIN_BACKOFF, doubling up to MAX_BACKOFF.
const MIN_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// A ping this often; nothing heard for SILENCE: the connection is dead.
const PING_EVERY: Duration = Duration::from_secs(30);
const SILENCE: Duration = Duration::from_secs(75);

/// What the page sets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteConfig {
    pub enabled: bool,
    /// `ws://…` or `wss://…`
    pub url: String,
    /// The `Authorization` header of the handshake, as it is (`Bearer …`,
    /// `Basic …`); empty: none.
    pub authorization: String,
    /// Sent first, as `{"type": "auth", "token": …}`; empty: no such message.
    pub auth_token: String,
    /// The server may send commands (as over the API).
    pub commands: bool,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        RemoteConfig {
            enabled: false,
            url: String::new(),
            authorization: String::new(),
            auth_token: String::new(),
            commands: true,
        }
    }
}

/// What the page shows about the connection.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RemoteStatus {
    /// "off", "connecting", "connected" or "waiting" (for the next attempt).
    pub state: &'static str,
    /// Since when (µs since 1970).
    pub since_us: Option<i64>,
    /// Why the last attempt or connection failed.
    pub error: Option<String>,
    /// Messages sent since the server started (events and replies).
    pub sent: u64,
    /// Events waiting for the connection.
    pub buffered: usize,
    /// Events dropped (too old, too many, or the channel overran).
    pub dropped: u64,
}

pub struct Remote {
    config: Mutex<RemoteConfig>,
    status: Mutex<RemoteStatus>,
    /// Woken when the settings change: the task reconnects with the new ones.
    changed: Notify,
}

impl Remote {
    pub fn new(config: RemoteConfig) -> Remote {
        Remote {
            config: Mutex::new(config),
            status: Mutex::new(RemoteStatus { state: "off", ..Default::default() }),
            changed: Notify::new(),
        }
    }

    /// Changes the status (under its lock, which is never held across an await).
    fn status(&self, f: impl FnOnce(&mut RemoteStatus)) {
        f(&mut self.status.lock().unwrap());
    }

    fn set_state(&self, state: &'static str, error: Option<String>) {
        self.status(|s| {
            if s.state != state {
                s.since_us = Some(unix_now_us());
            }
            s.state = state;
            if error.is_some() || state == "connected" || state == "off" {
                s.error = error;
            }
        });
    }
}

/// Events waiting for the connection, with when they came.
type Buffer = VecDeque<(Instant, String)>;

/// Runs as long as the server: connects whenever the settings say so.
pub async fn run(app: Arc<App>) {
    // Subscribed for good: events also come in while there is no connection.
    let mut events = app.api_tx.subscribe();
    let mut buffer = Buffer::new();
    let mut backoff = MIN_BACKOFF;
    loop {
        let config = app.remote.config.lock().unwrap().clone();
        if !config.enabled || config.url.trim().is_empty() {
            app.remote.set_state("off", None);
            buffer.clear();
            app.remote.status(|s| s.buffered = 0);
            // Off: let events pass (nobody wants them) until the settings change.
            loop {
                tokio::select! {
                    _ = app.remote.changed.notified() => break,
                    ev = events.recv() => if let Err(broadcast::error::RecvError::Closed) = ev { return },
                }
            }
            backoff = MIN_BACKOFF;
            continue;
        }

        app.remote.set_state("connecting", None);
        let ended = match connect(&config).await {
            Ok(socket) => {
                backoff = MIN_BACKOFF;
                session(&app, socket, &config, &mut events, &mut buffer).await
            }
            Err(e) => Ended::Error(format!("{e:#}")),
        };
        match ended {
            Ended::Closed => return,
            Ended::Changed => continue,
            Ended::Error(e) => {
                eprintln!("laptimer: Server-Verbindung: {e}");
                app.remote.set_state("waiting", Some(e));
            }
        }

        // Wait before the next attempt, keeping the events; new settings: at once.
        let wait = tokio::time::sleep(backoff);
        tokio::pin!(wait);
        loop {
            tokio::select! {
                _ = &mut wait => break,
                _ = app.remote.changed.notified() => { backoff = MIN_BACKOFF; break }
                ev = events.recv() => if !keep(&app, &mut buffer, ev) { return },
            }
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Why a connection (or attempt) ended.
enum Ended {
    /// The server is shutting down.
    Closed,
    /// The settings changed: connect again at once.
    Changed,
    Error(String),
}

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Opens the connection, with the `Authorization` header if there is one.
async fn connect(config: &RemoteConfig) -> Result<Socket> {
    let url = config.url.trim();
    let mut req = url
        .into_client_request()
        .map_err(|e| anyhow!("Ungültige Adresse „{url}“ (erwartet ws://… oder wss://…): {e}"))?;
    let headers = req.headers_mut();
    headers.insert(USER_AGENT, HeaderValue::from_str(&format!("laptimer/{}", crate::VERSION))?);
    if !config.authorization.is_empty() {
        let value = HeaderValue::from_str(&config.authorization).context("Der Authorization-Header enthält unzulässige Zeichen")?;
        headers.insert(AUTHORIZATION, value);
    }
    let attempt = tokio_tungstenite::connect_async(req);
    let (socket, _response) = match tokio::time::timeout(CONNECT_TIMEOUT, attempt).await {
        Err(_) => bail!("Keine Antwort von {url} (nach {} s)", CONNECT_TIMEOUT.as_secs()),
        Ok(Err(tokio_tungstenite::tungstenite::Error::Http(resp))) => {
            bail!("{url} lehnt ab: HTTP {}", resp.status())
        }
        Ok(Err(e)) => bail!("{url} nicht erreichbar: {e}"),
        Ok(Ok(s)) => s,
    };
    Ok(socket)
}

/// One connection: the auth message (if set), `hello`, the kept events,
/// then live until it ends.
async fn session(
    app: &Arc<App>,
    socket: Socket,
    config: &RemoteConfig,
    events: &mut broadcast::Receiver<String>,
    buffer: &mut Buffer,
) -> Ended {
    // Two halves, so reading and writing can wait in the same select.
    let (mut tx, mut rx) = socket.split();
    if !config.auth_token.is_empty() {
        let auth = json!({ "type": "auth", "token": config.auth_token });
        if let Err(e) = tx.send(Message::Text(auth.to_string().into())).await {
            return Ended::Error(format!("Senden fehlgeschlagen: {e}"));
        }
        app.remote.status(|s| s.sent += 1);
    }

    // The state after everything kept: the kept events come after it all
    // the same (with their own seq and time), as a record of what happened.
    let app2 = Arc::clone(app);
    let state = tokio::task::spawn_blocking(move || crate::api::command(&app2, "state", json!({})))
        .await
        .ok()
        .and_then(|r| r.ok());
    prune(app, buffer);
    let commands: &[&str] = if config.commands { crate::api::COMMANDS } else { &[] };
    let hello = json!({
        "type": "hello",
        "api": crate::api::API_VERSION,
        "version": crate::VERSION,
        "state": state,
        "commands": commands,
        "buffered": buffer.len(),
    });
    if let Err(e) = tx.send(Message::Text(hello.to_string().into())).await {
        return Ended::Error(format!("Senden fehlgeschlagen: {e}"));
    }
    let kept = buffer.len() as u64;
    while let Some((_, text)) = buffer.pop_front() {
        if let Err(e) = tx.send(Message::Text(text.into())).await {
            return Ended::Error(format!("Senden fehlgeschlagen: {e}"));
        }
    }
    app.remote.status(|s| {
        s.sent += 1 + kept;
        s.buffered = 0;
    });
    app.remote.set_state("connected", None);

    let mut ping = tokio::time::interval(PING_EVERY);
    ping.tick().await; // the first tick comes at once
    let mut heard = Instant::now();
    loop {
        // Whatever comes first: a message from the server, an event, the
        // ping, new settings.
        let out = tokio::select! {
            msg = rx.next() => {
                heard = Instant::now();
                match msg {
                    // Only commands get an answer; anything else from the
                    // server (e.g. an "auth ok") is not ours to reply to.
                    Some(Ok(Message::Text(t))) if !is_command(&t) => None,
                    Some(Ok(Message::Text(t))) => Some(if config.commands {
                        crate::api::ws_command(app, &t).await
                    } else {
                        refused(&t)
                    }),
                    // With the server's code and reason, e.g. a refused token.
                    Some(Ok(Message::Close(Some(frame)))) => {
                        let reason = if frame.reason.is_empty() { String::new() } else { format!(", {}", frame.reason) };
                        return Ended::Error(format!("Der Server hat die Verbindung beendet ({}{reason})", u16::from(frame.code)));
                    }
                    Some(Ok(Message::Close(None))) | None => return Ended::Error("Der Server hat die Verbindung beendet".into()),
                    Some(Err(e)) => return Ended::Error(format!("Verbindung abgebrochen: {e}")),
                    Some(Ok(_)) => None, // ping (answered by the library), pong, binary
                }
            }
            ev = events.recv() => match ev {
                Ok(text) => Some(Value::String(text)),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    app.remote.status(|s| s.dropped += n);
                    Some(json!({ "type": "lagged", "missed": n }))
                }
                Err(broadcast::error::RecvError::Closed) => return Ended::Closed,
            },
            _ = ping.tick() => {
                if heard.elapsed() > SILENCE {
                    return Ended::Error(format!("Keine Antwort vom Server seit {} s", SILENCE.as_secs()));
                }
                if let Err(e) = tx.send(Message::Ping(Vec::new().into())).await {
                    return Ended::Error(format!("Verbindung abgebrochen: {e}"));
                }
                None
            }
            _ = app.remote.changed.notified() => {
                let _ = tx.send(Message::Close(None)).await;
                return Ended::Changed;
            }
        };
        if let Some(v) = out {
            // An event is JSON text already; a reply is a value.
            let text = match v {
                Value::String(s) => s,
                other => other.to_string(),
            };
            if let Err(e) = tx.send(Message::Text(text.into())).await {
                return Ended::Error(format!("Senden fehlgeschlagen: {e}"));
            }
            app.remote.status(|s| s.sent += 1);
        }
    }
}

/// Keeps an event for the next connection; false: the server shuts down.
fn keep(app: &App, buffer: &mut Buffer, ev: Result<String, broadcast::error::RecvError>) -> bool {
    match ev {
        Ok(text) => buffer.push_back((Instant::now(), text)),
        Err(broadcast::error::RecvError::Lagged(n)) => app.remote.status(|s| s.dropped += n),
        Err(broadcast::error::RecvError::Closed) => return false,
    }
    prune(app, buffer);
    true
}

/// Drops what is too old or too much.
fn prune(app: &App, buffer: &mut Buffer) {
    let mut dropped = 0;
    while buffer.len() > BUFFER_LEN || buffer.front().is_some_and(|(t, _)| t.elapsed() > BUFFER_AGE) {
        buffer.pop_front();
        dropped += 1;
    }
    let len = buffer.len();
    app.remote.status(|s| {
        s.dropped += dropped;
        s.buffered = len;
    });
}

/// A JSON object with a `cmd`: a command (as on the API's WebSocket).
fn is_command(text: &str) -> bool {
    serde_json::from_str::<Value>(text).is_ok_and(|v| v.get("cmd").is_some())
}

/// The answer to a command when commands are switched off.
fn refused(text: &str) -> Value {
    let id = serde_json::from_str::<Value>(text).ok().and_then(|v| v.get("id").cloned()).unwrap_or(Value::Null);
    json!({
        "type": "reply", "id": id, "ok": false, "status": 403,
        "error": "Befehle über die Server-Verbindung sind abgeschaltet (Einstellungen → System).",
    })
}

// --- routes (login needed, also for reading: auth.rs) ---

pub fn routes() -> Router<Arc<App>> {
    Router::new().route("/remote", get(get_remote).put(put_remote))
}

type St = State<Arc<App>>;

/// The settings without the secrets (only whether they are set), and the status.
fn view(app: &App) -> Value {
    let c = app.remote.config.lock().unwrap().clone();
    let status = app.remote.status.lock().unwrap().clone();
    json!({
        "enabled": c.enabled,
        "url": c.url,
        "authorization_set": !c.authorization.is_empty(),
        "auth_token_set": !c.auth_token.is_empty(),
        "commands": c.commands,
        "status": status,
    })
}

async fn get_remote(State(app): St) -> Response {
    Json(view(&app)).into_response()
}

#[derive(Deserialize)]
struct RemoteUpdate {
    enabled: bool,
    url: String,
    /// Missing: keep it; "": none.
    authorization: Option<String>,
    /// Missing: keep it; "": none.
    auth_token: Option<String>,
    commands: bool,
}

async fn put_remote(State(app): St, Json(req): Json<RemoteUpdate>) -> Response {
    let result = tokio::task::spawn_blocking(move || -> Result<Value> {
        let url = req.url.trim().to_string();
        if req.enabled {
            if !(url.starts_with("ws://") || url.starts_with("wss://")) {
                bail!("Die Adresse muss mit ws:// oder wss:// beginnen.");
            }
            url.as_str().into_client_request().map_err(|e| anyhow!("Ungültige Adresse: {e}"))?;
        }
        let mut config = app.remote.config.lock().unwrap().clone();
        config.enabled = req.enabled;
        config.url = url;
        config.commands = req.commands;
        if let Some(a) = req.authorization {
            let a = a.trim().to_string();
            HeaderValue::from_str(&a).map_err(|_| anyhow!("Der Authorization-Header enthält unzulässige Zeichen."))?;
            config.authorization = a;
        }
        if let Some(t) = req.auth_token {
            config.auth_token = t.trim().to_string();
        }
        app.core.lock().unwrap().db.set_setting("remote", &config)?;
        *app.remote.config.lock().unwrap() = config;
        app.remote.changed.notify_one();
        Ok(view(&app))
    })
    .await;
    match result {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("{e:#}") }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e.to_string() }))).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_with_the_id() {
        let r = refused(r#"{"id": 7, "cmd": "session.start"}"#);
        assert_eq!(r["id"], 7);
        assert_eq!(r["status"], 403);
        assert_eq!(refused("kein json")["id"], Value::Null);
    }

    #[test]
    fn answers_only_commands() {
        assert!(is_command(r#"{"id": 1, "cmd": "ping"}"#));
        assert!(!is_command(r#"{"type": "auth_ok"}"#));
        assert!(!is_command("hallo"));
    }

    #[test]
    fn old_settings_read() {
        // A saved value without `commands` keeps the default (allowed).
        let c: RemoteConfig = serde_json::from_str(r#"{"enabled": true, "url": "ws://x"}"#).unwrap();
        assert!(c.enabled && c.commands);
        assert!(c.authorization.is_empty() && c.auth_token.is_empty());
    }
}
