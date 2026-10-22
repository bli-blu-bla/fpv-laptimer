//! Hooks: scripts run on events (`events.rs`), e.g. to switch a LED on a
//! GPIO pin of the Pi at the start signal.
//!
//! The scripts lie in one folder (`--hooks`, default `<data>/hooks`); only
//! executable files there can be chosen. Which script runs on which event
//! is stored in the database (table `hooks`, set on the page *API & Hooks*)
//! and kept here in memory; the page hands over the new list after every
//! change (`set_list`).
//!
//! A hook is: event (or "*" = every event), script, arguments (split at
//! spaces, no shell: `27 on`), in the background or not. The script runs in
//! the scripts folder. What the event says comes in environment variables:
//! `LAPTIMER_EVENT`, `LAPTIMER_SEQ`, `LAPTIMER_TIME_US`, `LAPTIMER_JSON` (the
//! whole event) and one `LAPTIMER_<FIELD>` for every plain value in its data
//! (`pilot_pass`: `LAPTIMER_PILOT`, `LAPTIMER_LAP`, `LAPTIMER_LAP_TIME_S`, …;
//! true/false as 1/0, null as empty). Output goes to the server's own output.
//!
//! One thread runs the hooks one after the other, in the order of the events
//! (LED on before LED off), each at most `TIMEOUT`. Something long (a LED
//! blinking for 3 s) is marked "background": started, not waited for.

use std::collections::{HashMap, VecDeque};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Value, json};

use crate::core::unix_now_us;
use crate::db::Hook;
use crate::events::Event;

/// How many finished runs the page shows.
const KEEP_RUNS: usize = 30;
/// A hook not in the background running longer is killed.
const TIMEOUT: Duration = Duration::from_secs(10);

/// One hook that ran (for the page).
#[derive(Debug, Clone, Serialize)]
pub struct HookRun {
    pub time_us: i64,
    pub event: String,
    pub hook_id: i64,
    /// Script and arguments.
    pub command: String,
    pub background: bool,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    /// Could not start, was killed, …
    pub error: Option<String>,
}

/// The finished runs, oldest first. In an `Arc`, because a hook in the
/// background records its run from its own thread.
type Runs = Arc<Mutex<VecDeque<HookRun>>>;

pub struct Hooks {
    /// The scripts folder.
    pub dir: PathBuf,
    list: Mutex<Vec<Hook>>,
    runs: Runs,
    /// The latest data of each event, for test runs.
    last: Mutex<HashMap<String, Value>>,
}

impl Hooks {
    /// Creates the scripts folder (if missing) and starts the hooks'
    /// thread. Events sent into the returned sender run their hooks.
    pub fn start(dir: PathBuf, list: Vec<Hook>) -> Result<(Arc<Hooks>, Sender<Event>)> {
        std::fs::create_dir_all(&dir).with_context(|| format!("{}", dir.display()))?;
        let (tx, rx) = mpsc::channel::<Event>();
        let hooks = Arc::new(Hooks {
            dir,
            list: Mutex::new(list),
            runs: Arc::new(Mutex::new(VecDeque::new())),
            last: Mutex::new(HashMap::new()),
        });
        let h = Arc::clone(&hooks);
        thread::spawn(move || {
            for event in rx {
                h.run_event(&event);
            }
        });
        Ok((hooks, tx))
    }

    /// The hooks as they are in the database now.
    pub fn set_list(&self, list: Vec<Hook>) {
        *self.list.lock().unwrap() = list;
    }

    /// The executable files in the scripts folder, sorted.
    pub fn scripts(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| self.check_script(name).is_ok())
            .collect();
        names.sort();
        names
    }

    /// Ok if `name` is an executable file directly in the scripts folder.
    pub fn check_script(&self, name: &str) -> Result<()> {
        if name.is_empty() || name.starts_with('.') || name.contains('/') {
            bail!("„{name}“ ist kein Script-Name.");
        }
        let meta = match std::fs::metadata(self.dir.join(name)) {
            Ok(m) => m,
            Err(_) => bail!("Script „{name}“ fehlt im Ordner {}.", self.dir.display()),
        };
        if !meta.is_file() {
            bail!("„{name}“ ist keine Datei.");
        }
        if meta.permissions().mode() & 0o111 == 0 {
            bail!("Script „{name}“ ist nicht ausführbar (chmod +x).");
        }
        Ok(())
    }

    /// The finished runs, newest first.
    pub fn runs(&self) -> Vec<HookRun> {
        self.runs.lock().unwrap().iter().rev().cloned().collect()
    }

    /// Runs one hook now (in its own thread), with the data of the last
    /// real occurrence of its event (or none) and `test: true`
    /// (→ `LAPTIMER_TEST=1`).
    pub fn test(&self, hook: Hook) {
        let mut data = self
            .last
            .lock()
            .unwrap()
            .get(&hook.event)
            .cloned()
            .unwrap_or_else(|| json!({}));
        if let Some(obj) = data.as_object_mut() {
            obj.insert("test".into(), json!(true));
        }
        let event = Event {
            kind: "event",
            // "*": test with any event's name.
            event: hook.event.replace('*', "test"),
            seq: 0,
            time_us: unix_now_us(),
            data,
        };
        let dir = self.dir.clone();
        let runs = Arc::clone(&self.runs);
        thread::spawn(move || run_hook(&hook, &event, &dir, &runs));
    }

    fn run_event(&self, event: &Event) {
        self.last
            .lock()
            .unwrap()
            .insert(event.event.clone(), event.data.clone());
        // Copy the matching hooks out, so the list isn't locked while they run.
        let hooks: Vec<Hook> = self
            .list
            .lock()
            .unwrap()
            .iter()
            .filter(|h| h.enabled && (h.event == event.event || h.event == "*"))
            .cloned()
            .collect();
        for hook in &hooks {
            run_hook(hook, event, &self.dir, &self.runs);
        }
    }
}

/// Runs one hook and records how it went. In the background: returns at
/// once, a thread waits for the script and records it then.
fn run_hook(hook: &Hook, event: &Event, dir: &Path, runs: &Runs) {
    let started = Instant::now();
    let child = match spawn(hook, event, dir) {
        Ok(c) => c,
        Err(e) => return record(runs, finished(hook, event, started, Err(e))),
    };
    if hook.background {
        // The thread needs its own copies: `hook`, `event` and `runs` are
        // only borrowed here and may be gone before the script ends.
        let (hook, event, runs) = (hook.clone(), event.clone(), Arc::clone(runs));
        thread::spawn(move || {
            let mut child = child;
            let result = child.wait().map_err(anyhow::Error::from).and_then(exit_code);
            record(&runs, finished(&hook, &event, started, result));
        });
    } else {
        record(runs, finished(hook, event, started, wait(child, TIMEOUT)));
    }
}

/// The record of a run that ended with `result` (exit code or error).
fn finished(hook: &Hook, event: &Event, started: Instant, result: Result<Option<i32>>) -> HookRun {
    let command = format!("{} {}", hook.script, hook.args).trim().to_string();
    let (exit_code, error) = match result {
        Ok(code) => (code, None),
        Err(e) => (None, Some(format!("{e:#}"))),
    };
    if let Some(e) = &error {
        eprintln!("hook {} ({command}): {e}", event.event);
    }
    HookRun {
        time_us: event.time_us,
        event: event.event.clone(),
        hook_id: hook.id,
        command,
        background: hook.background,
        exit_code,
        duration_ms: started.elapsed().as_millis() as u64,
        error,
    }
}

/// Starts the script of a hook with the event's environment.
fn spawn(hook: &Hook, event: &Event, dir: &Path) -> Result<Child> {
    let path = dir.join(&hook.script);
    if !path.is_file() {
        bail!("Script fehlt ({})", path.display());
    }
    Command::new(&path)
        .args(hook.args.split_whitespace())
        .current_dir(dir)
        .envs(environment(event))
        .stdin(Stdio::null())
        .spawn()
        .context("lässt sich nicht starten")
}

/// Waits for a script, at most `timeout`. Returns its exit code.
fn wait(mut child: Child, timeout: Duration) -> Result<Option<i32>> {
    let started = Instant::now();
    // std has no "wait with timeout": ask every 10 ms.
    loop {
        if let Some(status) = child.try_wait()? {
            return exit_code(status);
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("nach {} s abgebrochen (Hintergrund wählen)", timeout.as_secs());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn exit_code(status: std::process::ExitStatus) -> Result<Option<i32>> {
    match status.code() {
        Some(code) => Ok(Some(code)),
        None => bail!("beendet durch ein Signal"),
    }
}

fn record(runs: &Runs, run: HookRun) {
    let mut runs = runs.lock().unwrap();
    runs.push_back(run);
    while runs.len() > KEEP_RUNS {
        runs.pop_front();
    }
}

/// The environment variables of an event (see the top of this file).
pub fn environment(event: &Event) -> Vec<(String, String)> {
    let mut env = vec![
        ("LAPTIMER_EVENT".to_string(), event.event.clone()),
        ("LAPTIMER_SEQ".to_string(), event.seq.to_string()),
        ("LAPTIMER_TIME_US".to_string(), event.time_us.to_string()),
        (
            "LAPTIMER_JSON".to_string(),
            serde_json::to_string(event).unwrap_or_default(),
        ),
    ];
    if let Some(data) = event.data.as_object() {
        for (key, value) in data {
            let text = match value {
                Value::Null => String::new(),
                Value::Bool(b) => if *b { "1" } else { "0" }.to_string(),
                Value::Number(n) => n.to_string(),
                Value::String(s) => s.clone(),
                // Lists and objects: only in LAPTIMER_JSON.
                Value::Array(_) | Value::Object(_) => continue,
            };
            env.push((format!("LAPTIMER_{}", key.to_uppercase()), text));
        }
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_from_event() {
        let e = Event {
            kind: "event",
            event: "pilot_pass".into(),
            seq: 3,
            time_us: 42,
            data: json!({ "pilot": "NITRO", "lap": 2, "lap_time_s": 13.712, "finished": false, "position": null, "laps": [1] }),
        };
        let env: HashMap<String, String> = environment(&e).into_iter().collect();
        assert_eq!(env["LAPTIMER_EVENT"], "pilot_pass");
        assert_eq!(env["LAPTIMER_PILOT"], "NITRO");
        assert_eq!(env["LAPTIMER_LAP"], "2");
        assert_eq!(env["LAPTIMER_LAP_TIME_S"], "13.712");
        assert_eq!(env["LAPTIMER_FINISHED"], "0");
        assert_eq!(env["LAPTIMER_POSITION"], "");
        assert!(!env.contains_key("LAPTIMER_LAPS"));
    }

    fn script(dir: &Path, name: &str, text: &str, executable: bool) {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        let mode = if executable { 0o755 } else { 0o644 };
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    fn hook(id: i64, event: &str, script: &str, args: &str) -> Hook {
        Hook {
            id,
            event: event.into(),
            script: script.into(),
            args: args.into(),
            background: false,
            enabled: true,
        }
    }

    #[test]
    fn scripts_and_run() {
        let dir = std::env::temp_dir().join(format!("laptimer_hooks_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (hooks, _tx) = Hooks::start(dir.clone(), Vec::new()).unwrap();
        script(&dir, "out.sh", "#!/bin/sh\necho \"$LAPTIMER_PILOT $LAPTIMER_LAP $1 $2\" > out.txt\n", true);
        script(&dir, "fail.sh", "#!/bin/sh\nexit 3\n", true);
        script(&dir, "notes.txt", "nicht ausführbar", false);
        assert_eq!(hooks.scripts(), vec!["fail.sh", "out.sh"]);
        assert!(hooks.check_script("notes.txt").is_err());
        assert!(hooks.check_script("../out.sh").is_err());

        let mut off = hook(4, "pilot_pass", "fail.sh", "");
        off.enabled = false;
        hooks.set_list(vec![
            hook(1, "pilot_pass", "out.sh", "a  b"),
            hook(2, "*", "fail.sh", ""),
            hook(3, "pilot_pass", "missing.sh", ""),
            off,
            hook(5, "start_signal", "fail.sh", ""),
        ]);
        let e = Event {
            kind: "event",
            event: "pilot_pass".into(),
            seq: 1,
            time_us: 1,
            data: json!({ "pilot": "NITRO", "lap": 2 }),
        };
        hooks.run_event(&e);
        assert_eq!(std::fs::read_to_string(dir.join("out.txt")).unwrap().trim(), "NITRO 2 a b");
        let runs = hooks.runs();
        assert_eq!(runs.iter().map(|r| r.hook_id).collect::<Vec<_>>(), vec![3, 2, 1]);
        assert!(runs[0].error.as_deref().unwrap().contains("fehlt"));
        assert_eq!(runs[1].exit_code, Some(3));
        assert_eq!(runs[2].exit_code, Some(0));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
