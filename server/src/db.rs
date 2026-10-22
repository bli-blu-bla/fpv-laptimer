//! The SQLite database: pilots, sessions, their passes, and the settings.
//!
//! Times are wall-clock µs since 1970. Laps are not stored: they are computed
//! from the passes (`laps.rs`), so a corrected pass corrects every lap.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::laps::{EndRule, PassRecord, StartMode};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS pilots (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,     -- the callsign
    full_name TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY,
    config TEXT NOT NULL,          -- JSON, SessionConfig
    created_us INTEGER NOT NULL,   -- Start pressed
    start_us INTEGER NOT NULL,     -- start signal (after the countdown)
    ended_us INTEGER,              -- NULL while running
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
    deleted INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS passes_by_session ON passes(session_id);
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL            -- JSON
);
";

/// How a session runs. Stored with the session, so changing the settings
/// later doesn't change old results.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SessionConfig {
    pub race: bool,
    pub start_mode: StartMode,
    pub end_rule: EndRule,
    /// Seconds from Start to the start signal (races).
    pub countdown_s: u32,
    pub min_lap_s: f64,
    /// Laps in a row for "best consecutive".
    pub consecutive: u32,
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionRow {
    pub id: i64,
    pub config: SessionConfig,
    pub created_us: i64,
    pub start_us: i64,
    pub ended_us: Option<i64>,
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

    // --- sessions ---

    pub fn create_session(
        &self,
        config: &SessionConfig,
        created_us: i64,
        start_us: i64,
        seats: &[Seat],
        levels_file: Option<&str>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO sessions (config, created_us, start_us, levels_file) VALUES (?1, ?2, ?3, ?4)",
            params![serde_json::to_string(config)?, created_us, start_us, levels_file],
        )?;
        let id = self.conn.last_insert_rowid();
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
                "SELECT config, created_us, start_us, ended_us, levels_file FROM sessions WHERE id = ?1",
                [id],
                |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;
        let Some((config, created_us, start_us, ended_us, levels_file)) = row else {
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
            levels_file,
            seats,
        }))
    }

    // --- passes ---

    /// Several corrections of one channel at once, all or none: passes moved
    /// (they count as edited by hand from then on), deleted, restored, added.
    pub fn edit_passes(&self, session_id: i64, channel: u8, e: &PassEdit) -> Result<()> {
        self.conn.execute_batch("BEGIN")?;
        // The closure runs the changes; `?` inside it ends it early with the
        // error, and then everything is rolled back.
        let result = (|| -> Result<()> {
            let ids = "WHERE id = ?1 AND session_id = ?2 AND channel = ?3";
            for &(id, time_us) in &e.moves {
                self.conn.execute(
                    &format!("UPDATE passes SET time_us = ?4, manual = 1 {ids}"),
                    params![id, session_id, channel, time_us],
                )?;
            }
            for &id in &e.restores {
                self.conn.execute(
                    &format!("UPDATE passes SET deleted = 0 {ids}"),
                    params![id, session_id, channel],
                )?;
            }
            for &id in &e.deletes {
                self.conn.execute(
                    &format!("UPDATE passes SET deleted = 1 {ids}"),
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

    pub fn add_pass(&self, session_id: i64, channel: u8, time_us: i64, peak_db: f32, manual: bool) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO passes (session_id, channel, time_us, peak_db, manual) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, channel, time_us, peak_db, manual],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Deleting a detected pass only marks it, so it can be restored. A pass
    /// added by hand is removed (to undo, add it again). Returns its session.
    pub fn set_pass_deleted(&self, id: i64, deleted: bool) -> Result<Option<i64>> {
        let session = self.pass_session(id)?;
        self.conn
            .execute("UPDATE passes SET deleted = ?2 WHERE id = ?1", params![id, deleted])?;
        if deleted {
            self.conn
                .execute("DELETE FROM passes WHERE id = ?1 AND manual = 1", [id])?;
        }
        Ok(session)
    }

    /// The session a pass belongs to.
    pub fn pass_session(&self, id: i64) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT session_id FROM passes WHERE id = ?1", [id], |r| r.get(0))
            .optional()?)
    }

    /// For the detector taking a pass back: removed for good (it was never a
    /// decision of the user).
    pub fn remove_auto_pass(&self, session_id: i64, channel: u8, time_us: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM passes WHERE session_id = ?1 AND channel = ?2 AND time_us = ?3 AND manual = 0",
            params![session_id, channel, time_us],
        )?;
        Ok(())
    }

    pub fn passes(&self, session_id: i64) -> Result<Vec<PassRecord>> {
        let mut st = self.conn.prepare(
            "SELECT id, channel, time_us, peak_db, manual, deleted FROM passes WHERE session_id = ?1 ORDER BY time_us",
        )?;
        let rows = st.query_map([session_id], |r| {
            Ok(PassRecord {
                id: r.get(0)?,
                channel: r.get(1)?,
                time_us: r.get(2)?,
                peak_db: r.get(3)?,
                manual: r.get(4)?,
                deleted: r.get(5)?,
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
        db.delete_pilot(pilot).unwrap();
        assert_eq!(db.session(id).unwrap().unwrap().seats[0].pilot_id, None);
        db.set_setting("x", &vec![1, 2]).unwrap();
        assert_eq!(db.get_setting::<Vec<i32>>("x").unwrap(), Some(vec![1, 2]));
        db.delete_session(id).unwrap();
        assert!(db.passes(id).unwrap().is_empty());
    }
}
