//! The data as a whole: exporting sessions (CSV or JSON), backups of the
//! database and deleting all sessions (reset).
//!
//! - Export: `GET /api/export?sessions=3,5&channels=0,2&content=laps|results&format=csv|json`.
//!   One or several sessions in one file, all channels or only some (by
//!   index). CSV is meant for a spreadsheet: `;` between the fields, decimal
//!   comma, UTF-8 with BOM (so Excel reads the umlauts). JSON has the
//!   sessions with their standings and lap times (`content` doesn't matter).
//! - Debug package: `GET /api/export/debug?session=3&channels=0,2` is a
//!   `.tar.gz` with everything needed to find out why the detection did what
//!   it did (`debug_package`).
//! - Backup: `POST /api/backups` copies the database (`VACUUM INTO`, a
//!   consistent copy while the server runs) and gzips it into
//!   `<data>/backups/laptimer_<date>_<time>.db.gz`. `GET /api/backups`
//!   lists them, `GET /api/backups/{name}` downloads one, `DELETE` removes it.
//!   Restoring: stop the server, `gunzip` the file to `<data>/laptimer.db`.
//! - Reset: `POST /api/reset` deletes every session with its seats, passes
//!   and level files; pilots, settings, API keys and hooks stay. A backup
//!   is made first (`…_vor-reset.db.gz`).

use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use flate2::Compression;
use flate2::write::GzEncoder;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::core::{results, standings};
use crate::db::SessionRow;
use crate::laps::{EndRule, PassRecord, PilotResult};
use crate::server::App;

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/export", get(export))
        .route("/export/debug", get(export_debug))
        .route("/backups", get(list_backups).post(create_backup))
        .route("/backups/{name}", get(download_backup).delete(delete_backup))
        .route("/reset", post(reset))
}

type St = State<Arc<App>>;

fn bad(e: anyhow::Error) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("{e:#}") }))).into_response()
}

/// A file to save (the browser offers "save as").
fn file_response(name: &str, content_type: &str, body: Vec<u8>) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        ],
        body,
    )
        .into_response()
}

// --- export ---

#[derive(Deserialize)]
struct ExportQuery {
    /// Session ids, comma separated.
    sessions: String,
    /// Channel indexes, comma separated; missing or empty: all.
    #[serde(default)]
    channels: String,
    /// "laps" (one row per lap, default) or "results" (one row per pilot).
    #[serde(default)]
    content: String,
    /// "csv" (default) or "json".
    #[serde(default)]
    format: String,
}

/// One session to export: the session, its results (seat order) and which
/// of its seats are wanted.
pub struct Exported {
    pub row: SessionRow,
    pub results: Vec<PilotResult>,
    pub seats: Vec<usize>,
    /// All passes, for how each lap came about (`PassRecord::source`).
    pub passes: Vec<PassRecord>,
}

/// "3,5" → [3, 5].
fn id_list(text: &str, what: &str) -> Result<Vec<i64>> {
    text.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().with_context(|| format!("'{what}': Zahlen mit Komma getrennt, nicht „{s}“.")))
        .collect()
}

async fn export(State(app): St, Query(q): Query<ExportQuery>) -> Response {
    let result = tokio::task::spawn_blocking(move || build_export(&app, &q)).await;
    match result {
        Ok(Ok((name, content_type, body))) => file_response(&name, content_type, body),
        Ok(Err(e)) => bad(e),
        Err(e) => bad(e.into()),
    }
}

/// The file: (name, content type, content).
fn build_export(app: &App, q: &ExportQuery) -> Result<(String, &'static str, Vec<u8>)> {
    let ids = id_list(&q.sessions, "sessions")?;
    if ids.is_empty() {
        bail!("Keine Session gewählt.");
    }
    let channels = id_list(&q.channels, "channels")?;
    let mut list = Vec::new();
    {
        let core = app.core.lock().unwrap();
        for id in &ids {
            let row = core.db.session(*id)?.with_context(|| format!("Session {id} gibt es nicht."))?;
            let passes = core.db.passes(*id)?;
            let results = results(&row, &passes);
            let seats = (0..row.seats.len())
                .filter(|&i| channels.is_empty() || channels.contains(&(row.seats[i].channel as i64)))
                .collect();
            list.push(Exported { row, results, seats, passes });
        }
    }
    // Oldest first, like a logbook.
    list.sort_by_key(|e| e.row.created_us);
    if list.iter().all(|e| e.seats.is_empty()) {
        bail!("Die gewählten Kanäle fliegen in keiner der Sessions mit.");
    }

    let mut name = if ids.len() == 1 {
        format!("laptimer_session_{}", ids[0])
    } else {
        format!("laptimer_{}_sessions", ids.len())
    };
    // A single channel: its name in the file name ("_R3").
    if channels.len() == 1
        && let Some(seat) = list.iter().flat_map(|e| e.seats.iter().map(|&i| &e.row.seats[i])).next()
    {
        name += &format!("_{}", seat.channel_name);
    }
    match q.format.as_str() {
        "" | "csv" => {
            let (table, suffix) = match q.content.as_str() {
                "" | "laps" => (laps_csv(&list), "runden"),
                "results" => (results_csv(&list), "ergebnis"),
                other => bail!("'content': laps oder results, nicht „{other}“."),
            };
            // BOM: Excel reads the file as UTF-8 only with it.
            let body = format!("\u{feff}{table}").into_bytes();
            Ok((format!("{name}_{suffix}.csv"), "text/csv; charset=utf-8", body))
        }
        "json" => {
            let body = serde_json::to_vec_pretty(&export_json(&list))?;
            Ok((format!("{name}.json"), "application/json", body))
        }
        other => bail!("'format': csv oder json, nicht „{other}“."),
    }
}

/// µs → seconds with a decimal comma, 3 places ("13,712"); none: empty.
fn secs(us: Option<i64>) -> String {
    match us {
        Some(us) => format!("{:.3}", us as f64 / 1e6).replace('.', ","),
        None => String::new(),
    }
}

/// Local date and time ("23.10.2026 14:05:10").
fn local_time(us: i64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_micros(us).single() {
        Some(t) => t.format("%d.%m.%Y %H:%M:%S").to_string(),
        None => String::new(),
    }
}

/// A CSV field: quoted if it contains `;`, a quote or a line break.
fn field(text: &str) -> String {
    if text.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

fn csv_line(out: &mut String, fields: &[String]) {
    let line: Vec<String> = fields.iter().map(|f| field(f)).collect();
    out.push_str(&line.join(";"));
    out.push_str("\r\n");
}

/// "Rennen" / "Training".
fn kind(row: &SessionRow) -> &'static str {
    if row.config.race { "Rennen" } else { "Training" }
}

/// "3 Runden", "120 s", "offen".
fn end_text(row: &SessionRow) -> String {
    match row.config.end_rule {
        EndRule::Open => "offen".into(),
        EndRule::Laps(1) => "1 Runde".into(),
        EndRule::Laps(n) => format!("{n} Runden"),
        EndRule::Seconds(n) => format!("{n} s"),
    }
}

/// When a session's times start: a race at its start signal, a training at Start.
fn start_of(row: &SessionRow) -> i64 {
    if row.config.race { row.start_us } else { row.created_us }
}

/// One row per lap.
pub fn laps_csv(list: &[Exported]) -> String {
    let mut out = String::new();
    let head = [
        "Session", "Datum", "Art", "Format", "Kanal", "Frequenz (MHz)", "Pilot", "Runde", "Rundenzeit (s)", "Zeit seit Start (s)", "Uhrzeit",
        "Erfassung", "Verschoben um (s)",
    ];
    csv_line(&mut out, &head.map(String::from));
    for e in list {
        for &i in &e.seats {
            let seat = &e.row.seats[i];
            for lap in &e.results[i].laps {
                // The pass that ended the lap: detected, moved or added by hand.
                let pass = e.passes.iter().find(|p| p.id == lap.pass_id);
                let moved_by = pass.and_then(|p| p.detected_us.filter(|&d| d != p.time_us).map(|d| p.time_us - d));
                csv_line(
                    &mut out,
                    &[
                        e.row.id.to_string(),
                        local_time(e.row.created_us),
                        kind(&e.row).into(),
                        end_text(&e.row),
                        seat.channel_name.clone(),
                        seat.mhz.to_string(),
                        seat.label.clone(),
                        lap.number.to_string(),
                        secs(Some(lap.duration_us)),
                        secs(Some(lap.end_us - start_of(&e.row))),
                        local_time(lap.end_us),
                        pass.map_or("", |p| p.source()).into(),
                        secs(moved_by),
                    ],
                );
            }
        }
    }
    out
}

/// One row per pilot (seat) and session, in the order of the standings.
pub fn results_csv(list: &[Exported]) -> String {
    let mut out = String::new();
    let head = [
        "Session", "Datum", "Art", "Format", "Platz", "Kanal", "Frequenz (MHz)", "Pilot", "Runden", "Bestzeit (s)", "Beste in Folge (s)", "Gesamtzeit (s)",
        "Holeshot (s)", "Im Ziel",
    ];
    csv_line(&mut out, &head.map(String::from));
    for e in list {
        let st = standings(&e.row, &e.results);
        let mut seats = e.seats.clone();
        // By place (no place, e.g. no lap: at the end).
        seats.sort_by_key(|&i| st[i]["position"].as_i64().unwrap_or(i64::MAX));
        for i in seats {
            let seat = &e.row.seats[i];
            let r = &e.results[i];
            csv_line(
                &mut out,
                &[
                    e.row.id.to_string(),
                    local_time(e.row.created_us),
                    kind(&e.row).into(),
                    end_text(&e.row),
                    st[i]["position"].as_i64().map(|p| p.to_string()).unwrap_or_default(),
                    seat.channel_name.clone(),
                    seat.mhz.to_string(),
                    seat.label.clone(),
                    r.laps.len().to_string(),
                    secs(r.best_lap_us),
                    secs(r.best_consecutive.map(|(sum, _)| sum)),
                    secs(r.total_us),
                    secs(r.holeshot_us),
                    if r.finished_us.is_some() { "ja" } else { "nein" }.into(),
                ],
            );
        }
    }
    out
}

/// The sessions with their standings (as in the API, `core::standings`),
/// each with the lap times.
fn export_json(list: &[Exported]) -> Value {
    let sessions: Vec<Value> = list
        .iter()
        .map(|e| {
            let st = standings(&e.row, &e.results);
            let results: Vec<Value> = e.seats.iter().map(|&i| st[i].clone()).collect();
            json!({
                "session_id": e.row.id,
                "race": e.row.config.race,
                "config": e.row.config,
                "created_us": e.row.created_us,
                "start_us": e.row.start_us,
                "ended_us": e.row.ended_us,
                "confirmed_us": e.row.confirmed_us,
                "results": results,
                "passes": passes_json(e),
            })
        })
        .collect();
    json!({ "exported_us": crate::core::unix_now_us(), "sessions": sessions })
}

/// The finish-gate passes of the chosen channels, with how each came about
/// (`source`: erkannt / verschoben / von Hand; `detected_us`: where the detector had it).
fn passes_json(e: &Exported) -> Vec<Value> {
    let channels: Vec<u8> = e.seats.iter().map(|&i| e.row.seats[i].channel).collect();
    e.passes
        .iter()
        .filter(|p| p.gate == 0 && channels.contains(&p.channel))
        .map(|p| {
            json!({
                "id": p.id,
                "channel": p.channel,
                "time_us": p.time_us,
                "deleted": p.deleted,
                "rejected": p.rejected,
                "source": p.source(),
                "detected_us": p.detected_us,
            })
        })
        .collect()
}

// --- backups ---

fn backup_dir(app: &App) -> PathBuf {
    app.core.lock().unwrap().data_dir().join("backups")
}

/// Makes a backup: `<data>/backups/laptimer_<date>_<time>[_<note>].db.gz`.
/// Returns its name.
pub fn make_backup(app: &App, note: &str) -> Result<String> {
    let dir = backup_dir(app);
    fs::create_dir_all(&dir).with_context(|| format!("{}", dir.display()))?;
    let mut name = chrono::Local::now().format("laptimer_%Y-%m-%d_%H-%M-%S").to_string();
    if !note.is_empty() {
        name += &format!("_{note}");
    }
    let tmp = dir.join(format!("{name}.db.tmp"));
    let _ = fs::remove_file(&tmp); // VACUUM INTO wants a new file
    // The copy needs the database (core lock); compressing doesn't.
    app.core.lock().unwrap().db.copy_to(&tmp)?;
    let name = format!("{name}.db.gz");
    let result = gzip(&tmp, &dir.join(&name));
    let _ = fs::remove_file(&tmp);
    result?;
    Ok(name)
}

fn gzip(from: &Path, to: &Path) -> Result<()> {
    let mut input = BufReader::new(File::open(from)?);
    let mut gz = GzEncoder::new(BufWriter::new(File::create(to)?), Compression::default());
    std::io::copy(&mut input, &mut gz)?;
    gz.finish()?;
    Ok(())
}

/// Ok if `name` is a backup file's name (nothing outside the folder).
fn check_name(name: &str) -> Result<()> {
    if !name.ends_with(".db.gz") || name.contains('/') || name.starts_with('.') {
        bail!("„{name}“ ist kein Backup.");
    }
    Ok(())
}

async fn create_backup(State(app): St) -> Response {
    match tokio::task::spawn_blocking(move || make_backup(&app, "")).await {
        Ok(Ok(name)) => Json(json!({ "name": name })).into_response(),
        Ok(Err(e)) => bad(e),
        Err(e) => bad(e.into()),
    }
}

/// The backups, newest first: name, size, time.
async fn list_backups(State(app): St) -> Response {
    let dir = backup_dir(&app);
    let mut list: Vec<(i64, Value)> = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(meta) = entry.metadata() else { continue };
            if check_name(&name).is_err() || !meta.is_file() {
                continue;
            }
            let modified_us = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_micros() as i64);
            list.push((modified_us, json!({ "name": name, "size": meta.len(), "created_us": modified_us })));
        }
    }
    list.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
    let dir = dir.canonicalize().unwrap_or(dir);
    Json(json!({ "dir": dir, "backups": list.into_iter().map(|(_, v)| v).collect::<Vec<_>>() })).into_response()
}

async fn download_backup(State(app): St, UrlPath(name): UrlPath<String>) -> Response {
    if let Err(e) = check_name(&name) {
        return bad(e);
    }
    match fs::read(backup_dir(&app).join(&name)) {
        Ok(body) => file_response(&name, "application/gzip", body),
        Err(_) => bad(anyhow::anyhow!("Backup „{name}“ gibt es nicht.")),
    }
}

async fn delete_backup(State(app): St, UrlPath(name): UrlPath<String>) -> Response {
    if let Err(e) = check_name(&name) {
        return bad(e);
    }
    match fs::remove_file(backup_dir(&app).join(&name)) {
        Ok(()) => Json(json!({})).into_response(),
        Err(_) => bad(anyhow::anyhow!("Backup „{name}“ gibt es nicht.")),
    }
}

// --- reset ---

/// Deletes all sessions (a backup first). Refused while one runs.
async fn reset(State(app): St) -> Response {
    let result = tokio::task::spawn_blocking(move || -> Result<Value> {
        if app.core.lock().unwrap().active_id().is_some() {
            bail!("Es läuft eine Session – erst stoppen.");
        }
        let backup = make_backup(&app, "vor-reset")?;
        // Between the backup and here a session could have started: the
        // reset checks again (under the same lock as the delete).
        let deleted = app.core.lock().unwrap().reset_sessions()?;
        app.traces.lock().unwrap().clear();
        Ok(json!({ "deleted": deleted, "backup": backup }))
    })
    .await;
    match result {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => bad(e),
        Err(e) => bad(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Seat, SessionConfig};
    use crate::laps::StartMode;

    fn session(id: i64, race: bool) -> SessionRow {
        let seat = |channel: u8, name: &str, label: &str| Seat {
            channel,
            channel_name: name.into(),
            mhz: 5658,
            pilot_id: None,
            label: label.into(),
        };
        SessionRow {
            id,
            config: SessionConfig {
                race,
                start_mode: StartMode::FromStart,
                end_rule: if race { EndRule::Laps(2) } else { EndRule::Open },
                countdown_s: 0,
                min_lap_s: 3.0,
                consecutive: 3,
                detection: None,
            },
            created_us: 1_000_000,
            start_us: 1_000_000,
            ended_us: Some(30_000_000),
            confirmed_us: None,
            levels_file: None,
            seats: vec![seat(0, "R1", "NITRO"), seat(1, "R3", "A;B")],
        }
    }

    fn pass(id: i64, channel: u8, time_us: i64) -> PassRecord {
        PassRecord {
            id,
            channel,
            time_us,
            peak_db: 0.0,
            manual: false,
            deleted: false,
            gate: 0,
            rejected: None,
            detected_us: Some(time_us),
        }
    }

    fn exported(row: SessionRow, passes: &[PassRecord], seats: Vec<usize>) -> Exported {
        let results = results(&row, passes);
        Exported { row, results, seats, passes: passes.to_vec() }
    }

    #[test]
    fn csv_tables() {
        // Race, 2 laps: NITRO 5 s + 4,5 s, "A;B" one lap of 6 s.
        let passes = [pass(1, 0, 6_000_000), pass(2, 0, 10_500_000), pass(3, 1, 7_000_000)];
        let e = exported(session(7, true), &passes, vec![0, 1]);
        let laps = laps_csv(std::slice::from_ref(&e));
        let lines: Vec<&str> = laps.lines().collect();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].starts_with("Session;Datum;Art;Format;Kanal"));
        assert!(lines[1].starts_with("7;"));
        assert!(lines[1].contains(";Rennen;2 Runden;R1;5658;NITRO;1;5,000;5,000;"));
        assert!(lines[2].contains(";NITRO;2;4,500;9,500;"));
        assert!(lines[3].contains(";\"A;B\";1;6,000;6,000;"));
        assert!(lines[1].ends_with(";erkannt;"), "{}", lines[1]);

        // A lap ending in a moved pass says so, with how far.
        let mut moved = passes;
        moved[1].time_us = 10_250_000;
        let e2 = exported(session(7, true), &moved, vec![0]);
        let laps2 = laps_csv(std::slice::from_ref(&e2));
        assert!(laps2.lines().nth(2).unwrap().ends_with(";verschoben;-0,250"), "{laps2}");

        let res = results_csv(std::slice::from_ref(&e));
        let lines: Vec<&str> = res.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[1].contains(";1;R1;5658;NITRO;2;4,500;;9,500;;ja"));
        assert!(lines[2].contains(";2;R3;5658;\"A;B\";1;6,000;;6,000;;nein"), "{}", lines[2]);

        // Only channel R3.
        let e = exported(session(7, true), &passes, vec![1]);
        assert_eq!(laps_csv(std::slice::from_ref(&e)).lines().count(), 2);
        let json = export_json(std::slice::from_ref(&e));
        assert_eq!(json["sessions"][0]["results"].as_array().unwrap().len(), 1);
        assert_eq!(json["sessions"][0]["results"][0]["pilot"], json!("A;B"));
    }

    #[test]
    fn names_and_lists() {
        assert_eq!(id_list("3, 5,", "x").unwrap(), vec![3, 5]);
        assert!(id_list("3,a", "x").is_err());
        assert!(check_name("laptimer_2026-10-23_10-00-00.db.gz").is_ok());
        assert!(check_name("../laptimer.db.gz").is_err());
        assert!(check_name("laptimer.db").is_err());
        assert_eq!(secs(Some(13_712_400)), "13,712");
        assert_eq!(field("a\"b"), "\"a\"\"b\"");
    }
}

// --- debug package ---

#[derive(Deserialize)]
struct DebugQuery {
    session: i64,
    /// Channel indexes, comma separated; missing or empty: all.
    #[serde(default)]
    channels: String,
}

async fn export_debug(State(app): St, Query(q): Query<DebugQuery>) -> Response {
    // Reads the level file (and runs the detector over it): not on the async threads.
    let result = tokio::task::spawn_blocking(move || debug_package(&app, &q)).await;
    match result {
        Ok(Ok((name, body))) => file_response(&name, "application/gzip", body),
        Ok(Err(e)) => bad(e),
        Err(e) => bad(e.into()),
    }
}

const DEBUG_README: &str = "\
Debug-Paket des Laptimers

session.json         Session (Config, Sitze, Zeiten), aktuelle Einstellungen, alle
                     gespeicherten Durchflüge (auch gelöschte, von Hand eingefügte und
                     Freigabe-Gate), Ergebnis, Version
levels.csv           die Rohpegel der Session (Format wie `laptimer record`), nur die
                     gewählten Kanäle; mit `laptimer replay levels.csv` neu auswertbar
trace.csv            was der Detektor je 10-ms-Slot sah: Hüllkurve (kombiniert und je Board),
                     Eintritts- und Austrittsschwelle (dBFS, leer = keine)
detector_events.csv  was der Detektor meldete, auch Zurückgenommenes (retract). Neu
                     gerechnet mit den Einstellungen der Session, also nicht unbedingt
                     das, was die Session live gespeichert hat (siehe passes in session.json)

Zeiten: *_us sind Wanduhr-Mikrosekunden (µs seit 1970); in levels.csv ist host_us
die Host-Uhr, Wanduhr = host_us + clock_offset_us (session.json). Pegel in dBFS.
";

/// "-45.25" or "" (no data).
fn db_text(v: Option<f32>) -> String {
    v.map_or(String::new(), |x| format!("{x:.2}"))
}

fn code_db(code: u8) -> Option<f32> {
    (code > 0).then(|| crate::esp::frame::rssi_dbfs(code))
}

/// A `.tar.gz` of one session for debugging, all channels or only some:
/// (file name, content). Everything is built in memory (a level file of an
/// hour is some 100 MB).
fn debug_package(app: &App, q: &DebugQuery) -> Result<(String, Vec<u8>)> {
    let channels: Vec<usize> = id_list(&q.channels, "channels")?.iter().map(|&c| c as usize).collect();
    let wanted = |c: usize| channels.is_empty() || channels.contains(&c);
    let id = q.session;
    let (row, passes, settings) = {
        let mut core = app.core.lock().unwrap();
        let row = core.db.session(id)?.with_context(|| format!("Session {id} gibt es nicht."))?;
        core.flush_levels(id)?;
        let passes = core.db.passes(id)?;
        (row, passes, core.settings.clone())
    };
    let levels_path = row.levels_file.clone().context("Für diese Session sind keine Pegel aufgezeichnet.")?;
    let trace = crate::server::session_trace(app, id)?;
    let offset = trace.offset_us();

    // levels.csv: the comment lines, and the rows of the wanted channels.
    let mut levels = Vec::new();
    for line in BufReader::new(File::open(&levels_path)?).lines() {
        let line = line?;
        let keep = if line.starts_with('#') || line.starts_with("board") {
            true
        } else {
            // board,channel,host_us,...
            line.split(',').nth(1).and_then(|c| c.parse().ok()).is_some_and(&wanted)
        };
        if keep {
            levels.extend_from_slice(line.as_bytes());
            levels.push(b'\n');
        }
    }

    // trace.csv: one row per slot and channel.
    let mut trace_csv = String::from("channel,time_us,level_db");
    for b in 0..crate::detect::TRACE_BOARDS {
        trace_csv += &format!(",board{b}_db");
    }
    trace_csv += ",enter_db,exit_db\n";
    for c in (0..8).filter(|&c| wanted(c)) {
        for p in trace.slots(c) {
            trace_csv += &format!("{c},{},{}", p.time_us + offset, db_text(code_db(p.level)));
            for b in p.boards {
                trace_csv += &format!(",{}", db_text(code_db(b)));
            }
            trace_csv += &format!(",{},{}\n", db_text(code_db(p.enter)), db_text(code_db(p.exit)));
        }
    }

    let mut events_csv = String::from("event,channel,time_us,peak_db\n");
    for e in trace.events() {
        let (name, p) = match e {
            crate::detect::Event::Pass(p) => ("pass", p),
            crate::detect::Event::Retract(p) => ("retract", p),
        };
        if wanted(p.channel as usize) {
            events_csv += &format!("{name},{},{},{:.2}\n", p.channel, p.time_us + offset, p.peak_db);
        }
    }

    let results = results(&row, &passes);
    let session = json!({
        "laptimer_version": crate::VERSION,
        "exported_us": crate::core::unix_now_us(),
        "channels_exported": if channels.is_empty() { Value::Null } else { json!(channels) },
        "clock_offset_us": offset,
        "levels_file": levels_path,
        "session": row,
        "settings_now": settings,
        "standings": standings(&row, &results),
        "passes": passes.iter().filter(|p| wanted(p.channel as usize)).collect::<Vec<_>>(),
    });

    let mut name = format!("laptimer_session_{id}_debug");
    if channels.len() == 1
        && let Some(seat) = row.seats.iter().find(|s| s.channel as usize == channels[0])
    {
        name += &format!("_{}", seat.channel_name);
    }

    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    let now = (crate::core::unix_now_us() / 1_000_000) as u64;
    let files: [(&str, Vec<u8>); 5] = [
        ("README.txt", DEBUG_README.as_bytes().to_vec()),
        ("session.json", serde_json::to_vec_pretty(&session)?),
        ("levels.csv", levels),
        ("trace.csv", trace_csv.into_bytes()),
        ("detector_events.csv", events_csv.into_bytes()),
    ];
    for (file, data) in &files {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(now);
        tar.append_data(&mut header, format!("{name}/{file}"), data.as_slice())?;
    }
    let body = tar.into_inner()?.finish()?;
    Ok((format!("{name}.tar.gz"), body))
}
