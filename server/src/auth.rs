//! The password for the race control (`[auth]` in `config.toml`).
//!
//! Reading stays open (live view, sessions, best times, spectators,
//! export). Everything that changes something needs a login: `POST
//! /api/login {password}` sets the cookie `laptimer_login` (HttpOnly, 30
//! days); only its sha256 is stored (table `logins`), so logins survive a
//! restart or an update. `/api/v1` keeps its API keys (`api.rs`).
//!
//! Guessing is slowed down: every wrong password waits 1 s, after 5 in a row
//! the address is blocked for a minute.

use std::collections::HashMap;
use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::config::AuthConfig;
use crate::core::unix_now_us;
use crate::server::App;

const COOKIE: &str = "laptimer_login";
const LOGIN_DAYS: i64 = 30;
const MAX_FAILURES: u32 = 5;
const BLOCK: Duration = Duration::from_secs(60);

/// Lives in `App`.
pub struct Auth {
    enabled: bool,
    /// sha256 of the password: compared without depending on its length.
    password_hash: [u8; 32],
    /// Wrong passwords per address: (count in a row, blocked until).
    failures: Mutex<HashMap<IpAddr, (u32, Option<Instant>)>>,
}

impl Auth {
    pub fn new(config: &AuthConfig) -> Auth {
        Auth {
            enabled: config.enabled,
            password_hash: Sha256::digest(config.password.as_bytes()).into(),
            failures: Mutex::new(HashMap::new()),
        }
    }
}

/// `/api/auth`, `/api/login`, `/api/logout`, `/api/logout/all`.
pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/auth", get(status))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/logout/all", post(logout_all))
}

/// Middleware for `/api`: refuses what needs a login (`needs_login`) without one.
pub async fn require_login(State(app): State<Arc<App>>, req: Request, next: Next) -> Response {
    if !app.auth.enabled || !needs_login(req.method(), req.uri().path()) {
        return next.run(req).await;
    }
    if logged_in(&app, req.headers()).await {
        return next.run(req).await;
    }
    let error = "Bitte anmelden: Ändern braucht das Passwort der Rennleitung.";
    (StatusCode::UNAUTHORIZED, Json(json!({ "error": error, "login": true }))).into_response()
}

/// Reading is open, except API keys, hooks, backups (a backup holds
/// everything) and updates. Changing needs a login; `/api/v1` has its own keys.
fn needs_login(method: &Method, path: &str) -> bool {
    // Nested routers see the path without "/api"; strip it either way.
    let path = path.strip_prefix("/api").unwrap_or(path);
    let under = |prefix: &str| path == prefix || path.starts_with(&format!("{prefix}/"));
    if under("/v1") || under("/auth") || path == "/login" || path == "/logout" {
        return false;
    }
    if method == Method::GET || method == Method::HEAD {
        return under("/keys") || under("/hooks") || under("/backups") || under("/update") || under("/system");
    }
    true
}

async fn logged_in(app: &Arc<App>, headers: &HeaderMap) -> bool {
    let Some(token) = cookie(headers) else {
        return false;
    };
    let app = Arc::clone(app);
    tokio::task::spawn_blocking(move || {
        let since = unix_now_us() - LOGIN_DAYS * 86_400_000_000;
        let core = app.core.lock().unwrap();
        core.db.login_valid(&hash(&token), since).unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

/// The login cookie's value, if the browser sent one.
fn cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|kv| kv.trim().strip_prefix(&format!("{COOKIE}=")).map(str::to_string))
}

/// `{enabled, logged_in}`; without a password everyone counts as logged in.
async fn status(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    let logged_in = !app.auth.enabled || logged_in(&app, &headers).await;
    Json(json!({ "enabled": app.auth.enabled, "logged_in": logged_in })).into_response()
}

#[derive(Deserialize)]
struct LoginRequest {
    password: String,
}

async fn login(
    State(app): State<Arc<App>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<LoginRequest>,
) -> Response {
    let ip = addr.ip();
    if let Some(wait) = blocked(&app.auth, ip) {
        let error = format!("Zu viele Fehlversuche. Bitte in {} s noch einmal.", wait.as_secs() + 1);
        return (StatusCode::TOO_MANY_REQUESTS, Json(json!({ "error": error }))).into_response();
    }
    let given: [u8; 32] = Sha256::digest(req.password.as_bytes()).into();
    if !same(&given, &app.auth.password_hash) {
        failed(&app.auth, ip);
        tokio::time::sleep(Duration::from_secs(1)).await;
        let error = "Falsches Passwort.";
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": error }))).into_response();
    }
    app.auth.failures.lock().unwrap().remove(&ip);

    let app2 = Arc::clone(&app);
    let made = tokio::task::spawn_blocking(move || -> Result<String> {
        let token = hex(&random_bytes::<32>()?);
        let now = unix_now_us();
        let core = app2.core.lock().unwrap();
        // Expired logins go when a new one comes.
        core.db.delete_logins(Some(now - LOGIN_DAYS * 86_400_000_000))?;
        core.db.add_login(&hash(&token), now)?;
        Ok(token)
    })
    .await;
    match made {
        Ok(Ok(token)) => {
            let max_age = LOGIN_DAYS * 86_400;
            let cookie = format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}");
            ([(header::SET_COOKIE, cookie)], Json(json!({ "logged_in": true }))).into_response()
        }
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{e:#}") })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// How long `ip` still has to wait, if it is blocked.
fn blocked(auth: &Auth, ip: IpAddr) -> Option<Duration> {
    let mut failures = auth.failures.lock().unwrap();
    let (_, until) = failures.get(&ip)?;
    let until = (*until)?;
    let now = Instant::now();
    if now < until {
        return Some(until - now);
    }
    failures.remove(&ip); // served its time: start counting again
    None
}

fn failed(auth: &Auth, ip: IpAddr) {
    let mut failures = auth.failures.lock().unwrap();
    let entry = failures.entry(ip).or_insert((0, None));
    entry.0 += 1;
    if entry.0 >= MAX_FAILURES {
        entry.1 = Some(Instant::now() + BLOCK);
    }
}

/// Ends this browser's login.
async fn logout(State(app): State<Arc<App>>, headers: HeaderMap) -> Response {
    if let Some(token) = cookie(&headers) {
        let _ = tokio::task::spawn_blocking(move || app.core.lock().unwrap().db.delete_login(&hash(&token))).await;
    }
    let cookie = format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0");
    ([(header::SET_COOKIE, cookie)], Json(json!({ "logged_in": false }))).into_response()
}

/// Ends every login, this one too (needs a login: see `needs_login`).
async fn logout_all(State(app): State<Arc<App>>) -> Response {
    let done = tokio::task::spawn_blocking(move || app.core.lock().unwrap().db.delete_logins(None)).await;
    match done {
        Ok(Ok(())) => Json(json!({ "logged_in": false })).into_response(),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{e:#}") })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// Equal without stopping at the first difference (no timing hints).
fn same(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `N` random bytes from the system.
pub fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0u8; N];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .context("Zufallszahlen (/dev/urandom)")?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_needs_a_login() {
        let get = &Method::GET;
        let post = &Method::POST;
        assert!(!needs_login(get, "/state"));
        assert!(!needs_login(get, "/api/sessions/3/graph"));
        assert!(!needs_login(get, "/export"));
        assert!(needs_login(get, "/keys"));
        assert!(needs_login(get, "/api/hooks/2"));
        assert!(needs_login(get, "/backups"));
        assert!(!needs_login(get, "/keysomething"));
        assert!(needs_login(post, "/session/start"));
        assert!(needs_login(&Method::PUT, "/settings"));
        assert!(needs_login(&Method::DELETE, "/sessions/1"));
        assert!(needs_login(post, "/logout/all"));
        assert!(!needs_login(post, "/login"));
        assert!(!needs_login(post, "/logout"));
        assert!(!needs_login(post, "/v1/session/start"));
    }

    #[test]
    fn reads_the_cookie() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, "a=1; laptimer_login=abc; b=2".parse().unwrap());
        assert_eq!(cookie(&h).as_deref(), Some("abc"));
    }
}
