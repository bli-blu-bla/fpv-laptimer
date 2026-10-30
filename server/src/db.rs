//! The SQLite database: pilots, sessions, their passes, and the settings.
//!
//! Times are wall-clock µs since 1970. Laps are not stored: they are computed
//! from the passes (`laps.rs`), so a corrected pass corrects every lap.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::laps::{EndRule, PassRecord, Rejected, StartMode};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS pilots (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,     -- the callsign
    full_name TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS presets (
    id INTEGER PRIMARY KEY,
    pilot_id INTEGER NOT NULL REFERENCES pilots(id) ON DELETE CASCADE,
    name TEXT NOT NULL,            -- e.g. the quad: Whoop, 5 Zoll O3
    data TEXT NOT NULL,            -- JSON, PresetValues
    UNIQUE (pilot_id, name)
);
CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY,
    config TEXT NOT NULL,          -- JSON, SessionConfig
    created_us INTEGER NOT NULL,   -- Start pressed
    start_us INTEGER NOT NULL,     -- start signal (after the countdown)
    ended_us INTEGER,              -- NULL while running
    confirmed_us INTEGER,          -- result confirmed (races; NULL: not yet)
    levels_file TEXT               -- the raw levels (CSV, as `laptimer record`)
);
CREATE TABLE IF NOT EXISTS seats (
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    channel INTEGER NOT NULL,      -- index in the channel plan
    channel_name TEXT NOT NULL,
    mhz INTEGER NOT NULL,
    pilot_id INTEGER REFERENCES pilots(id),
    label TEXT NOT NULL,           -- what to show: pilot name, free text or channel name
    PRIMARY KEY (session_id, channel)
);
CREATE TABLE IF NOT EXISTS passes (
    id INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    channel INTEGER NOT NULL,
    time_us INTEGER NOT NULL,
    peak_db REAL NOT NULL,
    manual INTEGER NOT NULL DEFAULT 0,
    deleted INTEGER NOT NULL DEFAULT 0,
    gate INTEGER NOT NULL DEFAULT 0,  -- 0: the finish gate (laps), 1: the arming gate
    rejected TEXT,                    -- deleted by the server: why ('no_arming')
    detected_us INTEGER               -- where the detector put it (NULL: added by hand); differs from time_us: moved
);
CREATE INDEX IF NOT EXISTS passes_by_session ON passes(session_id);
CREATE TABLE IF NOT EXISTS api_keys (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    prefix TEXT NOT NULL,          -- the first characters, to recognize the key
    hash TEXT NOT NULL UNIQUE,     -- sha256 of the key (the key itself is shown once)
    created_us INTEGER NOT NULL,
    last_used_us INTEGER
);
CREATE TABLE IF NOT EXISTS hooks (
    id INTEGER PRIMARY KEY,
    event TEXT NOT NULL,           -- event name, or '*' = every event
    script TEXT NOT NULL,          -- file name in the scripts folder (hooks.rs)
    args TEXT NOT NULL DEFAULT '', -- split at spaces
    background INTEGER NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS logins (
    hash TEXT PRIMARY KEY,         -- sha256 of the login cookie (auth.rs)
    created_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL            -- JSON
);
";

/// How a session runs. Stored with the session, so changing the settings
/// later doesn't change old results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionConfig {
    pub race: bool,
    pub start_mode: StartMode,
    pub end_rule: EndRule,
    /// Seconds from Start to the start signal (races).
    pub countdown_s: u32,
    pub min_lap_s: f64,
    /// Laps in a row for "best consecutive".
    pub consecutive: u32,
    /// The detection settings when the session started (to draw its level
    /// graph with the same thresholds). Missing in older sessions.
    #[serde(default)]
    pub detection: Option<crate::detect::DetectionSettings>,
}

/// Who flies on a channel in a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Seat {
    pub channel: u8,
    pub channel_name: String,
    pub mhz: u32,
    pub pilot_id: Option<i64>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionRow {
    pub id: i64,
    pub config: SessionConfig,
    pub created_us: i64,
    pub start_us: i64,
    pub ended_us: Option<i64>,
    /// When the result was confirmed (races; until then no new session starts).
    pub confirmed_us: Option<i64>,
    pub levels_file: Option<String>,
    pub seats: Vec<Seat>,
}

/// Corrections of one channel (`Db::edit_passes`).
#[derive(Debug, Default, Deserialize)]
pub struct PassEdit {
    /// (pass id, new time)
    #[serde(default)]
    pub moves: Vec<(i64, i64)>,
    #[serde(default)]
    pub deletes: Vec<i64>,
    #[serde(default)]
    pub restores: Vec<i64>,
    /// Times of new passes.
    #[serde(default)]
    pub adds: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pilot {
    pub id: i64,
    /// The callsign.
    pub name: String,
    /// The real name (may be empty).
    pub full_name: String,
}

/// A pilot's detection settings for one quad, copied into the channel the
/// pilot flies on (`Core::apply_presets`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub id: i64,
    pub pilot_id: i64,
    pub name: String,
    /// `flatten`: the values sit next to id and name in the JSON.
    #[serde(flatten)]
    pub values: PresetValues,
}

/// What a preset holds: the finish gate's settings of one channel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PresetValues {
    pub mode: crate::detect::DetectMode,
    /// Fixed thresholds (manual mode), dBFS.
    pub enter_db: f32,
    pub exit_db: f32,
    /// Minimum swing (automatic mode), dB.
    pub min_rise_db: f32,
    /// The boards' gain when it was saved: fixed thresholds only fit that
    /// gain (and that gate setup); the page points out a different one.
    pub gain: u8,
}

/// An API key as listed (without the key itself).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ApiKey {
    pub id: i64,
    pub name: String,
    pub prefix: String,
    pub created_us: i64,
    pub last_used_us: Option<i64>,
}

/// A script run on an event (`hooks.rs`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hook {
    #[serde(default)]
    pub id: i64,
    /// Event name, or "*" = every event.
    pub event: String,
    /// File name in the scripts folder.
    pub script: String,
    #[serde(default)]
    pub args: String,
    /// Started without waiting for it (and without time limit).
    #[serde(default)]
    pub background: bool,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &str) -> Result<Db> {
        let conn = Connection::open(path)?;
        // WAL: reading while writing; foreign keys: cascade deletes.
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        // Databases from before full_name existed get the column.
        let has_full_name = conn
            .prepare("SELECT 1 FROM pragma_table_info('pilots') WHERE name = 'full_name'")?
            .exists([])?;
        let has_gate = conn
            .prepare("SELECT 1 FROM pragma_table_info('passes') WHERE name = 'gate'")?
            .exists([])?;
        if !has_gate {
            conn.execute_batch("ALTER TABLE passes ADD COLUMN gate INTEGER NOT NULL DEFAULT 0")?;
        }
        let has_rejected = conn
            .prepare("SELECT 1 FROM pragma_table_info('passes') WHERE name = 'rejected'")?
            .exists([])?;
        if !has_rejected {
            conn.execute_batch("ALTER TABLE passes ADD COLUMN rejected TEXT")?;
        }
        // Databases from before detected_us: what the detector found keeps its time.
        // Passes moved back then count as added by hand (moving set `manual`).
        let has_detected = conn
            .prepare("SELECT 1 FROM pragma_table_info('passes') WHERE name = 'detected_us'")?
            .exists([])?;
        if !has_detected {
            conn.execute_batch(
                "ALTER TABLE passes ADD COLUMN detected_us INTEGER;
                 UPDATE passes SET detected_us = time_us WHERE manual = 0;",
            )?;
        }
        // Databases from before confirmed_us: their sessions count as confirmed.
        let has_confirmed = conn
            .prepare("SELECT 1 FROM pragma_table_info('sessions') WHERE name = 'confirmed_us'")?
            .exists([])?;
        if !has_confirmed {
            conn.execute_batch(
                "ALTER TABLE sessions ADD COLUMN confirmed_us INTEGER;
                 UPDATE sessions SET confirmed_us = ended_us;",
            )?;
        }
        if !has_full_name {
            conn.execute_batch("ALTER TABLE pilots ADD COLUMN full_name TEXT NOT NULL DEFAULT ''")?;
        }
        Ok(Db { conn })
    }

    // --- settings: one JSON value per key ---

    pub fn get_setting<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let json: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
            .optional()?;
        // A value that doesn't parse (an older format) counts as missing.
        Ok(json.and_then(|j| serde_json::from_str(&j).ok()))
    }

    pub fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, serde_json::to_string(value)?],
        )?;
        Ok(())
    }

    // --- pilots ---

    pub fn pilots(&self) -> Result<Vec<Pilot>> {
        let mut st = self
            .conn
            .prepare("SELECT id, name, full_name FROM pilots ORDER BY name COLLATE NOCASE")?;
        let rows = st.query_map([], |r| {
            Ok(Pilot {
                id: r.get(0)?,
                name: r.get(1)?,
                full_name: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn add_pilot(&self, name: &str, full_name: &str) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO pilots (name, full_name) VALUES (?1, ?2)",
                [name, full_name],
            )
            .map_err(name_taken)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_pilot(&self, id: i64, name: &str, full_name: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE pilots SET name = ?2, full_name = ?3 WHERE id = ?1",
                params![id, name, full_name],
            )
            .map_err(name_taken)?;
        Ok(())
    }

    /// Old sessions keep their labels; only the link to the pilot goes.
    pub fn delete_pilot(&self, id: i64) -> Result<()> {
        self.conn
            .execute("UPDATE seats SET pilot_id = NULL WHERE pilot_id = ?1", [id])?;
        self.conn.execute("DELETE FROM pilots WHERE id = ?1", [id])?;
        Ok(())
    }

    // --- presets ---

    pub fn presets(&self) -> Result<Vec<Preset>> {
        let mut st = self
            .conn
            .prepare("SELECT id, pilot_id, name, data FROM presets ORDER BY pilot_id, name COLLATE NOCASE")?;
        let rows = st.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
        })?;
        let mut list = Vec::new();
        for row in rows {
            let (id, pilot_id, name, data) = row?;
            list.push(Preset {
                id,
                pilot_id,
                name,
                values: serde_json::from_str(&data)?,
            });
        }
        Ok(list)
    }

    pub fn add_preset(&self, pilot_id: i64, name: &str, values: &PresetValues) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO presets (pilot_id, name, data) VALUES (?1, ?2, ?3)",
                params![pilot_id, name, serde_json::to_string(values)?],
            )
            .map_err(preset_name_taken)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_preset(&self, id: i64, name: &str, values: &PresetValues) -> Result<()> {
        self.conn
            .execute(
                "UPDATE presets SET name = ?2, data = ?3 WHERE id = ?1",
                params![id, name, serde_json::to_string(values)?],
            )
            .map_err(preset_name_taken)?;
        Ok(())
    }

    pub fn delete_preset(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM presets WHERE id = ?1", [id])?;
        Ok(())
    }

    // --- API keys ---

    pub fn api_keys(&self) -> Result<Vec<ApiKey>> {
        let mut st = self
            .conn
            .prepare("SELECT id, name, prefix, created_us, last_used_us FROM api_keys ORDER BY created_us")?;
        let rows = st.query_map([], |r| {
            Ok(ApiKey {
                id: r.get(0)?,
                name: r.get(1)?,
                prefix: r.get(2)?,
                created_us: r.get(3)?,
                last_used_us: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn add_api_key(&self, name: &str, prefix: &str, hash: &str, created_us: i64) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO api_keys (name, prefix, hash, created_us) VALUES (?1, ?2, ?3, ?4)",
            params![name, prefix, hash, created_us],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn delete_api_key(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM api_keys WHERE id = ?1", [id])?;
        Ok(())
    }

    /// The key with this hash, if there is one: (id, last used).
    pub fn api_key_by_hash(&self, hash: &str) -> Result<Option<(i64, Option<i64>)>> {
        Ok(self
            .conn
            .query_row("SELECT id, last_used_us FROM api_keys WHERE hash = ?1", [hash], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?)
    }

    pub fn touch_api_key(&self, id: i64, used_us: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE api_keys SET last_used_us = ?2 WHERE id = ?1",
            params![id, used_us],
        )?;
        Ok(())
    }

    // --- logins (auth.rs) ---

    pub fn add_login(&self, hash: &str, created_us: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO logins (hash, created_us) VALUES (?1, ?2)",
            params![hash, created_us],
        )?;
        Ok(())
    }

    /// Whether a login with this hash exists that was made after `since_us`.
    pub fn login_valid(&self, hash: &str, since_us: i64) -> Result<bool> {
        Ok(self
            .conn
            .prepare("SELECT 1 FROM logins WHERE hash = ?1 AND created_us > ?2")?
            .exists(params![hash, since_us])?)
    }

    pub fn delete_login(&self, hash: &str) -> Result<()> {
        self.conn.execute("DELETE FROM logins WHERE hash = ?1", [hash])?;
        Ok(())
    }

    /// All logins, or those made before `before_us` (expired).
    pub fn delete_logins(&self, before_us: Option<i64>) -> Result<()> {
        self.conn.execute(
            "DELETE FROM logins WHERE created_us < ?1",
            [before_us.unwrap_or(i64::MAX)],
        )?;
        Ok(())
    }

    // --- hooks ---

    /// All hooks, in the order they run (for one event: by id).
    pub fn hooks(&self) -> Result<Vec<Hook>> {
        let mut st = self
            .conn
            .prepare("SELECT id, event, script, args, background, enabled FROM hooks ORDER BY id")?;
        let rows = st.query_map([], |r| {
            Ok(Hook {
                id: r.get(0)?,
                event: r.get(1)?,
                script: r.get(2)?,
                args: r.get(3)?,
                background: r.get(4)?,
                enabled: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn add_hook(&self, h: &Hook) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO hooks (event, script, args, background, enabled) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![h.event, h.script, h.args, h.background, h.enabled],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn update_hook(&self, h: &Hook) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE hooks SET event = ?2, script = ?3, args = ?4, background = ?5, enabled = ?6 WHERE id = ?1",
            params![h.id, h.event, h.script, h.args, h.background, h.enabled],
        )?;
        if n == 0 {
            anyhow::bail!("Hook {} gibt es nicht.", h.id);
        }
        Ok(())
    }

    pub fn delete_hook(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM hooks WHERE id = ?1", [id])?;
        Ok(())
    }

    // --- sessions ---

    pub fn create_session(
        &self,
        config: &SessionConfig,
        created_us: i64,
        start_us: i64,
        seats: &[Seat],
        levels_file: Option<&str>,
    ) -> Result<i64> {
        // A new id is never one a deleted session had (SQLite would reuse
        // the highest after a delete): drafts in the browser and exported
        // files are kept by session id. The highest id so far is a setting.
        let max: i64 = self
            .conn
            .query_row("SELECT COALESCE(MAX(id), 0) FROM sessions", [], |r| r.get(0))?;
        let id = max.max(self.get_setting::<i64>("last_session_id")?.unwrap_or(0)) + 1;
        self.conn.execute(
            "INSERT INTO sessions (id, config, created_us, start_us, levels_file) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, serde_json::to_string(config)?, created_us, start_us, levels_file],
        )?;
        self.set_setting("last_session_id", &id)?;
        for s in seats {
            self.conn.execute(
                "INSERT INTO seats (session_id, channel, channel_name, mhz, pilot_id, label)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, s.channel, s.channel_name, s.mhz, s.pilot_id, s.label],
            )?;
        }
        Ok(id)
    }

    pub fn set_levels_file(&self, id: i64, path: &str) -> Result<()> {
        self.conn
            .execute("UPDATE sessions SET levels_file = ?2 WHERE id = ?1", params![id, path])?;
        Ok(())
    }

    pub fn end_session(&self, id: i64, ended_us: i64) -> Result<()> {
        self.conn
            .execute("UPDATE sessions SET ended_us = ?2 WHERE id = ?1", params![id, ended_us])?;
        Ok(())
    }

    /// Sessions still open from a server that stopped while they ran: they
    /// end at their last pass (or where they began).
    pub fn close_open_sessions(&self) -> Result<usize> {
        Ok(self.conn.execute(
            "UPDATE sessions SET ended_us = COALESCE(
                 (SELECT MAX(time_us) FROM passes WHERE passes.session_id = sessions.id), created_us)
             WHERE ended_us IS NULL",
            [],
        )?)
    }

    /// A session cut after `cut_us`: the passes after it go (there are no
    /// levels left for them), the end moves there if it was later.
    pub fn trim_session(&self, id: i64, cut_us: i64) -> Result<usize> {
        let removed = self.conn.execute(
            "DELETE FROM passes WHERE session_id = ?1 AND time_us > ?2",
            params![id, cut_us],
        )?;
        self.conn.execute(
            "UPDATE sessions SET ended_us = ?2 WHERE id = ?1 AND ended_us > ?2",
            params![id, cut_us],
        )?;
        Ok(removed)
    }

    pub fn confirm_session(&self, id: i64, confirmed_us: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET confirmed_us = ?2 WHERE id = ?1",
            params![id, confirmed_us],
        )?;
        Ok(())
    }

    /// The newest ended race whose result isn't confirmed yet.
    pub fn unconfirmed_race(&self) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM sessions
                 WHERE ended_us IS NOT NULL AND confirmed_us IS NULL AND json_extract(config, '$.race') = 1
                 ORDER BY created_us DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Deletes every session (seats and passes go with them: cascade).
    /// Returns how many there were.
    pub fn delete_all_sessions(&self) -> Result<usize> {
        Ok(self.conn.execute("DELETE FROM sessions", [])?)
    }

    /// A consistent copy of the whole database into a new file (also while
    /// the server writes; `VACUUM INTO` also leaves out free pages).
    pub fn copy_to(&self, path: &std::path::Path) -> Result<()> {
        self.conn
            .execute("VACUUM INTO ?1", [path.to_string_lossy().to_string()])?;
        Ok(())
    }

    /// A session from another laptimer (`transfer.rs`), with a new id and
    /// all its passes, in one transaction. `row.seats` already point at the
    /// pilots here. The pass ids are new; nothing else refers to them.
    pub fn import_session(&self, row: &SessionRow, passes: &[PassRecord], levels_file: Option<&str>) -> Result<i64> {
        self.conn.execute_batch("BEGIN")?;
        // As in `edit_passes`: `?` in the closure ends it, then all is rolled back.
        let result = (|| -> Result<i64> {
            let id = self.create_session(&row.config, row.created_us, row.start_us, &row.seats, levels_file)?;
            self.conn.execute(
                "UPDATE sessions SET ended_us = ?2, confirmed_us = ?3 WHERE id = ?1",
                params![id, row.ended_us, row.confirmed_us],
            )?;
            for p in passes {
                let rejected = p.rejected.map(|r| match r {
                    Rejected::NoArming => "no_arming",
                });
                // Packages from before `detected_us`: detected passes as they are.
                let detected_us = p.detected_us.or((!p.manual).then_some(p.time_us));
                self.conn.execute(
                    "INSERT INTO passes (session_id, channel, time_us, peak_db, manual, deleted, gate, rejected, detected_us)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![id, p.channel, p.time_us, p.peak_db, p.manual, p.deleted, p.gate, rejected, detected_us],
                )?;
            }
            Ok(id)
        })();
        match &result {
            Ok(_) => self.conn.execute_batch("COMMIT")?,
            Err(_) => self.conn.execute_batch("ROLLBACK")?,
        }
        result
    }

    /// The session that began at exactly this time, if any (an import seen
    /// before: same creation and start, to the µs).
    pub fn session_at(&self, created_us: i64, start_us: i64) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM sessions WHERE created_us = ?1 AND start_us = ?2",
                params![created_us, start_us],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn delete_session(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM sessions WHERE id = ?1", [id])?;
        Ok(())
    }

    /// The newest first.
    pub fn sessions(&self, limit: u32) -> Result<Vec<SessionRow>> {
        let ids: Vec<i64> = {
            let mut st = self
                .conn
                .prepare("SELECT id FROM sessions ORDER BY created_us DESC LIMIT ?1")?;
            st.query_map([limit], |r| r.get(0))?.collect::<Result<_, _>>()?
        };
        ids.into_iter().filter_map(|id| self.session(id).transpose()).collect()
    }

    pub fn session(&self, id: i64) -> Result<Option<SessionRow>> {
        let row = self
            .conn
            .query_row(
                "SELECT config, created_us, start_us, ended_us, levels_file, confirmed_us FROM sessions WHERE id = ?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((config, created_us, start_us, ended_us, levels_file, confirmed_us)) = row else {
            return Ok(None);
        };
        let mut st = self.conn.prepare(
            "SELECT channel, channel_name, mhz, pilot_id, label FROM seats WHERE session_id = ?1 ORDER BY channel",
        )?;
        let seats = st
            .query_map([id], |r| {
                Ok(Seat {
                    channel: r.get(0)?,
                    channel_name: r.get(1)?,
                    mhz: r.get(2)?,
                    pilot_id: r.get(3)?,
                    label: r.get(4)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(Some(SessionRow {
            id,
            config: serde_json::from_str(&config)?,
            created_us,
            start_us,
            ended_us,
            confirmed_us,
            levels_file,
            seats,
        }))
    }

    // --- passes ---

    /// Several corrections of one channel at once, all or none: passes moved
    /// (`detected_us` keeps where the detector had them), deleted, restored, added.
    pub fn edit_passes(&self, session_id: i64, channel: u8, e: &PassEdit) -> Result<()> {
        self.conn.execute_batch("BEGIN")?;
        // The closure runs the changes; `?` inside it ends it early with the
        // error, and then everything is rolled back.
        let result = (|| -> Result<()> {
            // Only finish-gate passes are edited.
            let ids = "WHERE id = ?1 AND session_id = ?2 AND channel = ?3 AND gate = 0";
            for &(id, time_us) in &e.moves {
                self.conn.execute(
                    &format!("UPDATE passes SET time_us = ?4 {ids}"),
                    params![id, session_id, channel, time_us],
                )?;
            }
            for &id in &e.restores {
                self.conn.execute(
                    &format!("UPDATE passes SET deleted = 0, rejected = NULL {ids}"),
                    params![id, session_id, channel],
                )?;
            }
            for &id in &e.deletes {
                self.conn.execute(
                    &format!("UPDATE passes SET deleted = 1, rejected = NULL {ids}"),
                    params![id, session_id, channel],
                )?;
                self.conn.execute(
                    &format!("DELETE FROM passes {ids} AND manual = 1"),
                    params![id, session_id, channel],
                )?;
            }
            for &time_us in &e.adds {
                self.add_pass(session_id, channel, time_us, 0.0, true)?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => self.conn.execute_batch("COMMIT")?,
            Err(_) => self.conn.execute_batch("ROLLBACK")?,
        }
        result
    }

    /// A pass of the arming gate (it doesn't make laps; see `core.rs`).
    pub fn add_arming_pass(&self, session_id: i64, channel: u8, time_us: i64, peak_db: f32) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO passes (session_id, channel, time_us, peak_db, gate, detected_us) VALUES (?1, ?2, ?3, ?4, 1, ?3)",
            params![session_id, channel, time_us, peak_db],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// `manual`: added by hand; else found by the detector (`detected_us` = `time_us`).
    pub fn add_pass(&self, session_id: i64, channel: u8, time_us: i64, peak_db: f32, manual: bool) -> Result<i64> {
        let detected_us = (!manual).then_some(time_us);
        self.conn.execute(
            "INSERT INTO passes (session_id, channel, time_us, peak_db, manual, detected_us) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![session_id, channel, time_us, peak_db, manual, detected_us],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Deleting a detected pass only marks it, so it can be restored. A pass
    /// added by hand is removed (to undo, add it again). Returns its session.
    pub fn set_pass_deleted(&self, id: i64, deleted: bool) -> Result<Option<i64>> {
        let session = self.pass_session(id)?;
        self.conn
            .execute("UPDATE passes SET deleted = ?2, rejected = NULL WHERE id = ?1", params![id, deleted])?;
        if deleted {
            self.conn
                .execute("DELETE FROM passes WHERE id = ?1 AND manual = 1", [id])?;
        }
        Ok(session)
    }

    /// A pass the server doesn't count (see `Rejected`): marked deleted with
    /// the reason, so the page can say why and the user can restore it.
    pub fn reject_pass(&self, id: i64, why: Rejected) -> Result<()> {
        let why = match why {
            Rejected::NoArming => "no_arming",
        };
        self.conn.execute(
            "UPDATE passes SET deleted = 1, rejected = ?2 WHERE id = ?1",
            params![id, why],
        )?;
        Ok(())
    }

    /// The session a pass belongs to.
    pub fn pass_session(&self, id: i64) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT session_id FROM passes WHERE id = ?1", [id], |r| r.get(0))
            .optional()?)
    }

    /// For the detector taking a pass back: removed for good (it was never a
    /// decision of the user). Not if the user moved it meanwhile.
    pub fn remove_auto_pass(&self, session_id: i64, channel: u8, time_us: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM passes WHERE session_id = ?1 AND channel = ?2 AND time_us = ?3 AND manual = 0
             AND detected_us = time_us",
            params![session_id, channel, time_us],
        )?;
        Ok(())
    }

    pub fn passes(&self, session_id: i64) -> Result<Vec<PassRecord>> {
        let mut st = self.conn.prepare(
            "SELECT id, channel, time_us, peak_db, manual, deleted, gate, rejected, detected_us FROM passes WHERE session_id = ?1 ORDER BY time_us",
        )?;
        let rows = st.query_map([session_id], |r| {
            Ok(PassRecord {
                id: r.get(0)?,
                channel: r.get(1)?,
                time_us: r.get(2)?,
                peak_db: r.get(3)?,
                manual: r.get(4)?,
                deleted: r.get(5)?,
                gate: r.get(6)?,
                rejected: match r.get::<_, Option<String>>(7)?.as_deref() {
                    Some("no_arming") => Some(Rejected::NoArming),
                    _ => None,
                },
                detected_us: r.get(8)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

/// A second pilot with the same callsign: say so in words the page can show.
fn name_taken(e: rusqlite::Error) -> anyhow::Error {
    if let rusqlite::Error::SqliteFailure(f, _) = &e
        && f.code == rusqlite::ErrorCode::ConstraintViolation
    {
        return anyhow::anyhow!("Diesen Callsign gibt es schon.");
    }
    e.into()
}

fn preset_name_taken(e: rusqlite::Error) -> anyhow::Error {
    if let rusqlite::Error::SqliteFailure(f, _) = &e
        && f.code == rusqlite::ErrorCode::ConstraintViolation
    {
        return anyhow::anyhow!("Ein Preset mit diesem Namen hat der Pilot schon.");
    }
    e.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_with_passes() {
        let db = Db::open(":memory:").unwrap();
        let config = SessionConfig {
            race: true,
            start_mode: StartMode::Holeshot,
            end_rule: EndRule::Laps(3),
            countdown_s: 5,
            min_lap_s: 3.0,
            consecutive: 3,
            detection: None,
        };
        let pilot = db.add_pilot("Anna", "").unwrap();
        assert!(db.add_pilot("Anna", "x").is_err()); // callsigns are unique
        let seats = vec![Seat {
            channel: 1,
            channel_name: "R3".into(),
            mhz: 5732,
            pilot_id: Some(pilot),
            label: "Anna".into(),
        }];
        let id = db.create_session(&config, 100, 5_000_100, &seats, None).unwrap();
        let p1 = db.add_pass(id, 1, 6_000_000, -18.0, false).unwrap();
        db.add_pass(id, 1, 12_000_000, -19.0, false).unwrap();
        let by_hand = db.add_pass(id, 1, 9_000_000, 0.0, true).unwrap();
        db.set_pass_deleted(p1, true).unwrap();
        assert_eq!(db.set_pass_deleted(by_hand, true).unwrap(), Some(id)); // removed
        db.remove_auto_pass(id, 1, 12_000_000).unwrap();
        let passes = db.passes(id).unwrap();
        assert_eq!(passes.len(), 1);
        assert!(passes[0].deleted);
        assert_eq!(passes[0].rejected, None);
        // Rejected by the server: the reason is kept until the user decides.
        db.reject_pass(p1, Rejected::NoArming).unwrap();
        assert_eq!(db.passes(id).unwrap()[0].rejected, Some(Rejected::NoArming));
        db.set_pass_deleted(p1, false).unwrap();
        let p = db.passes(id).unwrap()[0];
        assert!(!p.deleted && p.rejected.is_none());
        db.set_pass_deleted(p1, true).unwrap();
        let s = db.session(id).unwrap().unwrap();
        assert_eq!(s.config, config);
        assert_eq!(s.seats, seats);
        // An edit: move the restored pass, add one; all in one go.
        let p2 = db.add_pass(id, 1, 20_000_000, -19.0, false).unwrap();
        let e = PassEdit {
            moves: vec![(p2, 21_000_000)],
            deletes: vec![],
            restores: vec![p1],
            adds: vec![30_000_000],
        };
        db.edit_passes(id, 1, &e).unwrap();
        let times: Vec<(i64, bool)> = db.passes(id).unwrap().iter().map(|p| (p.time_us, p.deleted)).collect();
        assert_eq!(
            times,
            vec![(6_000_000, false), (21_000_000, false), (30_000_000, false)]
        );
        // How each came about: detected, moved (detected time kept), by hand.
        let sources: Vec<(&str, Option<i64>)> = db.passes(id).unwrap().iter().map(|p| (p.source(), p.detected_us)).collect();
        assert_eq!(
            sources,
            vec![("erkannt", Some(6_000_000)), ("verschoben", Some(20_000_000)), ("von Hand", None)]
        );
        // The detector can't take back a moved pass; deleting it only marks it.
        db.remove_auto_pass(id, 1, 20_000_000).unwrap();
        db.set_pass_deleted(p2, true).unwrap();
        let p = db.passes(id).unwrap().into_iter().find(|p| p.id == p2).unwrap();
        assert!(p.deleted && p.source() == "verschoben");
        db.set_pass_deleted(p2, false).unwrap();
        db.delete_pilot(pilot).unwrap();
        assert_eq!(db.session(id).unwrap().unwrap().seats[0].pilot_id, None);
        db.set_setting("x", &vec![1, 2]).unwrap();
        assert_eq!(db.get_setting::<Vec<i32>>("x").unwrap(), Some(vec![1, 2]));
        db.delete_session(id).unwrap();
        assert!(db.passes(id).unwrap().is_empty());
        // Ids are not used again, also not after deleting all.
        let seats = vec![Seat { pilot_id: None, ..seats[0].clone() }];
        let id2 = db.create_session(&config, 100, 100, &seats, None).unwrap();
        assert_eq!(id2, id + 1);
        db.create_session(&config, 200, 200, &seats, None).unwrap();
        assert_eq!(db.delete_all_sessions().unwrap(), 2);
        assert_eq!(db.create_session(&config, 300, 300, &seats, None).unwrap(), id + 3);
    }

    #[test]
    fn detected_time_added_to_old_databases() {
        // A database from before `detected_us`: one detected pass, one moved
        // back then (`manual` set), one added by hand.
        let path = std::env::temp_dir().join(format!("laptimer_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE passes (id INTEGER PRIMARY KEY, session_id INTEGER NOT NULL, channel INTEGER NOT NULL,
                   time_us INTEGER NOT NULL, peak_db REAL NOT NULL, manual INTEGER NOT NULL DEFAULT 0,
                   deleted INTEGER NOT NULL DEFAULT 0, gate INTEGER NOT NULL DEFAULT 0, rejected TEXT);
                 INSERT INTO passes (session_id, channel, time_us, peak_db, manual) VALUES
                   (1, 0, 1000, -20, 0), (1, 0, 2000, -21, 1), (1, 0, 3000, 0, 1);",
            )
            .unwrap();
        }
        let db = Db::open(path.to_str().unwrap()).unwrap();
        let sources: Vec<&str> = db.passes(1).unwrap().iter().map(|p| p.source()).collect();
        assert_eq!(sources, vec!["erkannt", "von Hand", "von Hand"]);
        drop(db);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn races_wait_for_confirmation() {
        let db = Db::open(":memory:").unwrap();
        let mut config = SessionConfig {
            race: false,
            start_mode: StartMode::Flying,
            end_rule: EndRule::Open,
            countdown_s: 0,
            min_lap_s: 3.0,
            consecutive: 3,
            detection: None,
        };
        let training = db.create_session(&config, 100, 100, &[], None).unwrap();
        db.end_session(training, 200).unwrap();
        assert_eq!(db.unconfirmed_race().unwrap(), None); // trainings need no confirmation
        config.race = true;
        let race = db.create_session(&config, 300, 300, &[], None).unwrap();
        assert_eq!(db.unconfirmed_race().unwrap(), None); // still running
        db.end_session(race, 400).unwrap();
        assert_eq!(db.unconfirmed_race().unwrap(), Some(race));
        db.confirm_session(race, 500).unwrap();
        assert_eq!(db.unconfirmed_race().unwrap(), None);
        assert_eq!(db.session(race).unwrap().unwrap().confirmed_us, Some(500));
    }
}
