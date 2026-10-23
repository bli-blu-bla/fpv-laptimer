//! Events for the outside world: what happened at the track (a session
//! started, a pilot passed the gate, a result was confirmed, …), as one JSON
//! message each. They go to the API's WebSocket (`/api/v1/ws`, `api.rs`) and
//! to the hooks (`hooks.rs`, scripts run per event).
//!
//! The message:
//! `{"type": "event", "event": "pilot_pass", "seq": 17, "time_us": …, "data": {…}}`
//! (`seq` counts up from 1 at every server start; `time_us`: wall clock, µs
//! since 1970). The events and their data are listed in `docs/API.md`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;

use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast;

use crate::core::unix_now_us;

/// Every event there is (for the page; see `docs/API.md`).
pub const EVENT_NAMES: &[&str] = &[
    "session_started",
    "countdown",
    "start_signal",
    "pilot_pass",
    "pilot_finished",
    "session_stopped",
    "race_confirmed",
    "session_deleted",
    "sessions_reset",
    "passes_changed",
    "channels_changed",
    "pilot_added",
    "pilot_updated",
    "pilot_deleted",
];

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    /// Always "event" (WebSocket messages are told apart by `type`).
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub event: String,
    pub seq: u64,
    pub time_us: i64,
    pub data: Value,
}

/// Sends events to the API WebSocket and the hooks.
pub struct Events {
    /// For the WebSocket clients (each subscribes); the event as JSON text.
    pub ws: broadcast::Sender<String>,
    /// To the hooks' thread.
    hooks: Sender<Event>,
    seq: AtomicU64,
}

impl Events {
    pub fn new(ws: broadcast::Sender<String>, hooks: Sender<Event>) -> Events {
        Events {
            ws,
            hooks,
            seq: AtomicU64::new(0),
        }
    }

    pub fn emit(&self, event: &str, data: Value) {
        let e = Event {
            kind: "event",
            event: event.to_string(),
            seq: self.seq.fetch_add(1, Ordering::Relaxed) + 1,
            time_us: unix_now_us(),
            data,
        };
        // Nobody listening is fine: both sends may fail.
        if let Ok(text) = serde_json::to_string(&e) {
            let _ = self.ws.send(text);
        }
        let _ = self.hooks.send(e);
    }
}
