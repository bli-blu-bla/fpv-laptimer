//! Updates over the web page (Einstellungen → System), going back to the
//! previous version, restarting the laptimer and shutting the Pi down.
//!
//! Only for an installed release (`<root>/releases/<ver>/bin/laptimer`, made
//! by `laptimer install`, see `install.rs`; `<root>` is `~/.laptimer`). With
//! `cargo run` the page says that updates are not possible.
//!
//! An update comes from an upload (tarball + `.minisig`) or from the releases
//! of a repository (`[update] url` in `config.toml`, default our GitHub
//! repository; GitHub or Forgejo, downloaded with `curl`). Two steps, so the
//! page can show what it got before anything changes:
//! 1. Stage (`POST /api/update/upload` or `/api/update/download`): check the
//!    signature (the built-in `minisign.pub` or `[update] extra_keys`),
//!    unpack to `<root>/staging/`, read `VERSION`, check that `bin/laptimer`
//!    is for this processor.
//! 2. Install (`POST /api/update/install`): no session running, backup
//!    `…_vor-update`, move to `releases/<ver>`, switch `previous` and
//!    `current`, replace the systemd unit if it changed, then the server
//!    exits and the service (systemd or launchd) starts the new `current`;
//!    without a service the server starts the new program itself. An older
//!    (or the same, or an unclear) version only with `allow_older`: the page
//!    asks first.
//!
//! Automatic rollback: before switching, the server writes
//! `<root>/update-pending` (`to=releases/<new>`, `from=releases/<old>`,
//! `attempts=0`). `update-check.sh`, run by the service before each start
//! (`ExecStartPre`), counts the starts; at the 4th start it switches back to
//! `from` and writes `<root>/update-failed`, which the page shows. The new
//! version deletes `update-pending` once it has run for 15 s.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use minisign_verify::{PublicKey, Signature};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::config::UpdateConfig;
use crate::server::App;

/// The key the CI signs releases with (`minisign.pub` in the repository).
const RELEASE_KEY: &str = include_str!("../../minisign.pub");
/// How long a new version must run before it counts as working.
const CONFIRM_AFTER: Duration = Duration::from_secs(15);
/// Releases kept in `releases/` (`current` and `previous` always stay).
const KEEP: usize = 3;
/// Upload limit; a release is ~5 MB.
const MAX_UPLOAD: usize = 64 << 20;
/// Where updates come from without `[update] url` in `config.toml`.
pub const DEFAULT_SOURCE: &str = "https://github.com/bli-blu-bla/fpv-laptimer";

pub struct Updater {
    /// `~/.laptimer` if this program runs from an installed release.
    root: Option<PathBuf>,
    /// This program's release, `releases/<ver>` (like the `current` link).
    own: Option<String>,
    config: UpdateConfig,
    keys: Vec<PublicKey>,
    /// Checked and unpacked, waiting for "install".
    staged: Mutex<Option<Staged>>,
    /// Held while downloading, staging or installing: one at a time.
    busy: Mutex<()>,
}

struct Staged {
    version: String,
    /// `<root>/staging/<the tarball's folder>`
    dir: PathBuf,
    order: Order,
    /// For the page: "Upload <file>" or "GitHub <tag>".
    source: String,
}

/// A version compared with the running one.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Order {
    Newer,
    Same,
    Older,
    /// Same numbers, but suffixes that don't say which is newer.
    Unclear,
}

impl Updater {
    pub fn new(config: UpdateConfig) -> Result<Updater> {
        let mut keys = vec![PublicKey::decode(RELEASE_KEY).map_err(|e| anyhow!("minisign.pub: {e}"))?];
        for k in &config.extra_keys {
            let key = PublicKey::from_base64(k.trim())
                .map_err(|e| anyhow!("config.toml, [update] extra_keys: „{k}“ ist kein minisign-Schlüssel ({e})"))?;
            keys.push(key);
        }
        if config.token.contains(['"', '\\', '\n']) {
            bail!("config.toml, [update] token: enthält \" oder \\ oder einen Zeilenumbruch");
        }
        let (root, own) = match install_location() {
            Some((root, own)) => (Some(root), Some(own)),
            None => (None, None),
        };
        let updater = Updater {
            root,
            own,
            config,
            keys,
            staged: Mutex::new(None),
            busy: Mutex::new(()),
        };
        updater.confirm_later();
        Ok(updater)
    }

    fn root(&self) -> Result<&Path> {
        self.root
            .as_deref()
            .context("Updates gehen nur bei einer installierten Version (install.sh), nicht mit cargo run.")
    }

    /// One update at a time.
    fn lock_busy(&self) -> Result<std::sync::MutexGuard<'_, ()>> {
        self.busy.try_lock().map_err(|_| anyhow!("Es läuft schon ein Update."))
    }

    /// Everything the page shows.
    pub fn status(&self) -> Value {
        let root = self.root.as_deref();
        let previous = root
            .and_then(|r| link(r, "previous").filter(|p| r.join(p).join("bin/laptimer").is_file()))
            .map(|p| version_of(&p));
        let failed = root.and_then(|r| read_kv(&r.join("update-failed"))).map(|kv| {
            json!({
                "to": version_of(kv.get("to").map_or("", String::as_str)),
                "from": version_of(kv.get("from").map_or("", String::as_str)),
                "time_s": kv.get("time").and_then(|t| t.parse::<i64>().ok()),
            })
        });
        json!({
            "version": crate::VERSION,
            "installed": root.is_some(),
            "previous": previous,
            "online": !self.source_url().is_empty(),
            // "GitHub" or "Forgejo", for the page's texts.
            "source": self.source().map(|s| s.name()).ok(),
            "platform": platform(),
            "staged": self.staged_json(),
            "failed": failed,
            "pending": root.is_some_and(|r| r.join("update-pending").exists()),
            // Under a service (systemd, launchd), which starts it again after
            // an exit; otherwise the server starts the new program itself.
            "managed": managed(),
            "power": can_power(),
        })
    }

    fn staged_json(&self) -> Value {
        match &*self.staged.lock().unwrap() {
            Some(s) => json!({ "version": s.version, "order": s.order, "source": s.source }),
            None => Value::Null,
        }
    }

    /// Checks and unpacks a package; it waits for `install`.
    pub fn stage(&self, package: &[u8], signature: &str, source: &str) -> Result<Value> {
        let _busy = self.lock_busy()?;
        self.stage_locked(package, signature, source)
    }

    fn stage_locked(&self, package: &[u8], signature: &str, source: &str) -> Result<Value> {
        let root = self.root()?;
        self.verify(package, signature)?;
        let staging = root.join("staging");
        *self.staged.lock().unwrap() = None;
        let _ = fs::remove_dir_all(&staging);
        fs::create_dir_all(&staging).with_context(|| format!("{}", staging.display()))?;
        let dir = unpack(package, &staging)?;
        let version = fs::read_to_string(dir.join("VERSION"))
            .map_err(|_| anyhow!("Im Paket fehlt die Datei VERSION."))?
            .trim()
            .to_string();
        check_version_name(&version)?;
        check_binary(&dir.join("bin/laptimer"))?;
        let order = compare(&version, crate::VERSION);
        *self.staged.lock().unwrap() = Some(Staged {
            version,
            dir,
            order,
            source: source.to_string(),
        });
        Ok(self.staged_json())
    }

    fn verify(&self, package: &[u8], signature: &str) -> Result<()> {
        let sig = Signature::decode(signature.trim())
            .map_err(|_| anyhow!("Die Signatur-Datei (.minisig) ist unlesbar."))?;
        if self.keys.iter().any(|k| k.verify(package, &sig, true).is_ok()) {
            return Ok(());
        }
        bail!("Die Signatur passt nicht: das Paket ist beschädigt oder nicht mit unserem Schlüssel signiert.")
    }

    /// Forgets the staged package.
    pub fn discard(&self) -> Result<Value> {
        let _busy = self.lock_busy()?;
        *self.staged.lock().unwrap() = None;
        if let Some(root) = &self.root {
            let _ = fs::remove_dir_all(root.join("staging"));
        }
        Ok(self.status())
    }

    /// Installs the staged package and exits (systemd starts the new one).
    pub fn install(&self, app: &App, allow_older: bool) -> Result<Value> {
        let root = self.root()?;
        let _busy = self.lock_busy()?;
        check_idle(app)?;
        let staged = {
            let mut slot = self.staged.lock().unwrap();
            let s = slot
                .as_ref()
                .context("Kein Update bereit: erst ein Paket hochladen oder herunterladen.")?;
            if s.order != Order::Newer && !allow_older {
                let how = match s.order {
                    Order::Same => "dieselbe Version wie",
                    Order::Older => "älter als",
                    _ => "nicht sicher neuer als",
                };
                bail!("Version {} ist {how} die laufende ({}).", s.version, crate::VERSION);
            }
            slot.take().unwrap()
        };

        let backup = crate::data::make_backup(app, "vor-update")?;
        let name = format!("releases/{}", staged.version);
        let dest = root.join(&name);
        if dest.exists() {
            // The same version again (or a leftover). A running program
            // keeps its open files, so the old folder can go.
            let old = root.join(format!("{name}.old"));
            let _ = fs::remove_dir_all(&old);
            fs::rename(&dest, &old).with_context(|| format!("{}", dest.display()))?;
            let _ = fs::remove_dir_all(&old);
        }
        fs::create_dir_all(root.join("releases"))?;
        fs::rename(&staged.dir, &dest).with_context(|| format!("{}", dest.display()))?;
        let _ = fs::remove_dir_all(root.join("staging"));

        let current = link(root, "current");
        if let Some(cur) = current.as_deref().filter(|c| *c != name) {
            // Written before switching: update-check.sh rolls back to `from`
            // (and `previous` back to what it was) if the new version
            // doesn't come up.
            let previous = link(root, "previous").unwrap_or_default();
            let marker = [("to", name.as_str()), ("from", cur), ("previous", &previous), ("attempts", "0")];
            write_kv(&root.join("update-pending"), &marker)?;
            set_link(root, "previous", cur)?;
        }
        set_link(root, "current", &name)?;
        // A version that didn't come up goes first.
        if let Some(failed) = read_kv(&root.join("update-failed")).and_then(|kv| kv.get("to").cloned()) {
            let keep = [Some(name.clone()), link(root, "previous")];
            if failed.starts_with("releases/") && !keep.contains(&Some(failed.clone())) {
                let _ = fs::remove_dir_all(root.join(&failed));
            }
            let _ = fs::remove_file(root.join("update-failed"));
        }
        update_unit(&dest);
        cleanup(root);
        println!("laptimer: Update auf {} installiert, Neustart", staged.version);
        exit_soon(Some(root));
        Ok(json!({ "version": staged.version, "backup": backup }))
    }

    /// Back to `previous` (and `current` becomes `previous`), then exits.
    pub fn rollback(&self, app: &App) -> Result<Value> {
        let root = self.root()?;
        let _busy = self.lock_busy()?;
        check_idle(app)?;
        let prev = link(root, "previous")
            .filter(|p| root.join(p).join("bin/laptimer").is_file())
            .context("Es gibt keine vorige Version.")?;
        let cur = link(root, "current").context("~/.laptimer/current fehlt.")?;
        let _ = fs::remove_file(root.join("update-pending"));
        set_link(root, "current", &prev)?;
        set_link(root, "previous", &cur)?;
        println!("laptimer: zurück auf {}, Neustart", version_of(&prev));
        exit_soon(Some(root));
        Ok(json!({ "version": version_of(&prev) }))
    }

    /// Hides the "update failed" notice.
    pub fn dismiss(&self) -> Result<Value> {
        if let Some(root) = &self.root {
            let _ = fs::remove_file(root.join("update-failed"));
        }
        Ok(self.status())
    }

    /// After an update: once this version has run for a while, it counts as
    /// working (`update-check.sh` stops counting).
    fn confirm_later(&self) {
        let (Some(root), Some(own)) = (&self.root, &self.own) else {
            return;
        };
        let marker = root.join("update-pending");
        let to = |m: &Path| read_kv(m).and_then(|kv| kv.get("to").cloned());
        match to(&marker) {
            None => return,
            Some(t) if t != *own => {
                // Not for us (rolled back, or started without systemd).
                let _ = fs::remove_file(&marker);
                return;
            }
            Some(_) => {}
        }
        let own = own.clone();
        std::thread::spawn(move || {
            std::thread::sleep(CONFIRM_AFTER);
            // Checked again: meanwhile a newer update may have written its own.
            if to(&marker).as_ref() == Some(&own) {
                let _ = fs::remove_file(&marker);
                println!("laptimer: Update läuft, bestätigt");
            }
        });
    }

    // --- GitHub / Forgejo ---

    /// `[update] url`, or our repository if there is none (`url = ""`: off).
    fn source_url(&self) -> &str {
        self.config.url.as_deref().unwrap_or(DEFAULT_SOURCE).trim()
    }

    /// `https://github.com/owner/repo` or `https://host/owner/repo` (Forgejo).
    fn source(&self) -> Result<Source> {
        let url = self.source_url().trim_end_matches('/').trim_end_matches(".git");
        if url.is_empty() {
            bail!("Keine Update-Quelle: in ~/.laptimer/config.toml steht unter [update] url = \"\".");
        }
        let bad = || anyhow!("[update] url: erwartet https://host/Besitzer/Repo, nicht „{url}“");
        let (scheme, rest) = url.split_once("://").ok_or_else(bad)?;
        let (host, repo) = rest.split_once('/').ok_or_else(bad)?;
        Ok(Source {
            github: host == "github.com",
            origin: format!("{scheme}://{host}"),
            repo: repo.to_string(),
        })
    }

    fn online_releases(&self, source: &Source) -> Result<Vec<OnlineRelease>> {
        let url = if source.github {
            format!("https://api.github.com/repos/{}/releases?per_page=20", source.repo)
        } else {
            format!("{}/api/v1/repos/{}/releases?limit=20", source.origin, source.repo)
        };
        let body = self.curl(source, &url, None, false)?;
        serde_json::from_slice(&body).with_context(|| format!("Unerwartete Antwort von {}", source.name()))
    }

    /// The releases online, newest first, compared with this version.
    pub fn list_online(&self) -> Result<Value> {
        let source = self.source()?;
        let list: Vec<Value> = self
            .online_releases(&source)?
            .iter()
            .filter(|r| !r.draft)
            .take(10)
            .map(|r| {
                let version = r.tag_name.trim_start_matches('v');
                let (package, signature) = package_names(version);
                let has = |n: &str| r.assets.iter().any(|a| a.name == n);
                json!({
                    "tag": r.tag_name,
                    "version": version,
                    "prerelease": r.prerelease,
                    "published_at": r.published_at,
                    "order": compare(version, crate::VERSION),
                    "available": has(&package) && has(&signature),
                })
            })
            .collect();
        Ok(json!({ "releases": list }))
    }

    /// Downloads a release and stages it.
    pub fn download(&self, tag: &str) -> Result<Value> {
        let root = self.root()?;
        let _busy = self.lock_busy()?;
        let source = self.source()?;
        let release = self
            .online_releases(&source)?
            .into_iter()
            .find(|r| r.tag_name == tag)
            .with_context(|| format!("Release {tag} gibt es auf {} nicht.", source.name()))?;
        let version = tag.trim_start_matches('v');
        let (package, signature) = package_names(version);
        let asset = |name: &str| {
            release
                .assets
                .iter()
                .find(|a| a.name == name)
                .with_context(|| format!("{name} fehlt im Release {tag}."))
        };
        let (package_asset, signature_asset) = (asset(&package)?, asset(&signature)?);
        let dir = root.join("download");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir)?;
        let result = (|| {
            self.fetch_asset(&source, package_asset, &dir.join(&package))?;
            self.fetch_asset(&source, signature_asset, &dir.join(&signature))?;
            let bytes = fs::read(dir.join(&package))?;
            let sig = fs::read_to_string(dir.join(&signature))?;
            self.stage_locked(&bytes, &sig, &format!("{} {tag}", source.name()))
        })();
        let _ = fs::remove_dir_all(&dir);
        result
    }

    /// One file of a release into `out`. With a token (private repository)
    /// not over the public download link: GitHub wants the asset's API URL,
    /// Forgejo `/attachments/<uuid>`.
    fn fetch_asset(&self, source: &Source, asset: &OnlineAsset, out: &Path) -> Result<()> {
        let token = !self.config.token.is_empty();
        if source.github && token && !asset.url.is_empty() {
            self.curl(source, &asset.url, Some(out), true)?;
        } else if !source.github && !asset.uuid.is_empty() {
            self.curl(source, &format!("{}/attachments/{}", source.origin, asset.uuid), Some(out), false)?;
        } else {
            self.curl(source, &asset.browser_download_url, Some(out), false)?;
        }
        Ok(())
    }

    /// GET with curl (into `out`, or returned). The token goes to curl on
    /// stdin, not on the command line (where `ps` would show it). `binary`:
    /// the file itself, not its description (GitHub's asset API).
    fn curl(&self, source: &Source, url: &str, out: Option<&Path>, binary: bool) -> Result<Vec<u8>> {
        let mut cmd = Command::new("curl");
        cmd.args(["-fsSL", "--max-time", "300", "--max-filesize", "200000000", "-K", "-"]);
        if let Some(p) = out {
            cmd.arg("-o").arg(p);
        }
        cmd.arg(url).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().context("curl fehlt (sudo apt install curl)")?;
        let mut config = String::new();
        if !self.config.token.is_empty() {
            let scheme = if source.github { "Bearer" } else { "token" };
            config += &format!("header = \"Authorization: {scheme} {}\"\n", self.config.token);
        }
        if binary {
            config += "header = \"Accept: application/octet-stream\"\n";
        }
        // Dropping stdin after writing ends curl's config.
        child.stdin.take().unwrap().write_all(config.as_bytes())?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            bail!("{} nicht erreichbar oder kein Zugriff ({url}): {}", source.name(), err.trim());
        }
        Ok(output.stdout)
    }
}

/// Where the releases are: GitHub or a Forgejo.
struct Source {
    github: bool,
    /// `https://host`
    origin: String,
    /// `owner/repo`
    repo: String,
}

impl Source {
    fn name(&self) -> &'static str {
        if self.github { "GitHub" } else { "Forgejo" }
    }
}

/// A release as GitHub and Forgejo describe it (the fields both have, plus
/// each one's own download handle).
#[derive(Deserialize)]
struct OnlineRelease {
    tag_name: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    published_at: String,
    #[serde(default)]
    assets: Vec<OnlineAsset>,
}

#[derive(Deserialize)]
struct OnlineAsset {
    name: String,
    #[serde(default)]
    browser_download_url: String,
    /// GitHub: the asset's API URL.
    #[serde(default)]
    url: String,
    /// Forgejo: for `/attachments/<uuid>`.
    #[serde(default)]
    uuid: String,
}

/// The release's files for this system and processor, e.g.
/// `laptimer-0.2.0-linux-aarch64.tar.gz` (`tools/build_release.sh`).
fn package_names(version: &str) -> (String, String) {
    let package = format!("laptimer-{version}-{}.tar.gz", platform());
    let signature = format!("{package}.minisig");
    (package, signature)
}

/// `linux-aarch64`, `macos-x86_64`, …: the packages' names.
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// `(~/.laptimer, "releases/<ver>")` if this program is
/// `<root>/releases/<ver>/bin/laptimer` and `<root>/current` exists.
fn install_location() -> Option<(PathBuf, String)> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let release = exe.parent()?.parent()?;
    let releases = release.parent()?;
    if releases.file_name()? != "releases" {
        return None;
    }
    let root = releases.parent()?;
    root.join("current").symlink_metadata().ok()?;
    let own = format!("releases/{}", release.file_name()?.to_string_lossy());
    Some((root.to_path_buf(), own))
}

/// Where a link in `<root>` points, e.g. `releases/0.2.0`.
pub fn link(root: &Path, name: &str) -> Option<String> {
    fs::read_link(root.join(name)).ok().map(|p| p.to_string_lossy().into_owned())
}

/// `releases/0.2.0` → `0.2.0`.
pub fn version_of(release: &str) -> String {
    release.rsplit('/').next().unwrap_or(release).to_string()
}

/// Points `<root>/<name>` at `target`, atomically (a new link renamed over the old).
pub fn set_link(root: &Path, name: &str, target: &str) -> Result<()> {
    let tmp = root.join(format!("{name}.new"));
    let _ = fs::remove_file(&tmp);
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, &tmp).with_context(|| format!("{}", tmp.display()))?;
    #[cfg(not(unix))]
    bail!("Updates nur unter Linux ({target})");
    fs::rename(&tmp, root.join(name)).with_context(|| format!("{}", root.join(name).display()))?;
    Ok(())
}

/// The marker files: `key=value` lines (read by `update-check.sh` too).
pub fn read_kv(path: &Path) -> Option<HashMap<String, String>> {
    let text = fs::read_to_string(path).ok()?;
    Some(
        text.lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect(),
    )
}

fn write_kv(path: &Path, pairs: &[(&str, &str)]) -> Result<()> {
    let text: String = pairs.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
    let tmp = path.with_extension("new");
    fs::write(&tmp, text).with_context(|| format!("{}", tmp.display()))?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Neither a session nor a flash may be cut off.
fn check_idle(app: &App) -> Result<()> {
    if app.core.lock().unwrap().session_running() {
        bail!("Es läuft eine Session – erst stoppen.");
    }
    if app.flashing() {
        bail!("Es wird gerade ein Board geflasht.");
    }
    Ok(())
}

/// Unpacks the tarball into `into`; returns its one top folder
/// (`laptimer-<ver>/`). `unpack_in` refuses paths that leave `into`.
fn unpack(package: &[u8], into: &Path) -> Result<PathBuf> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(package));
    // Files get the time of unpacking: cleanup() keeps the newest folders.
    archive.set_preserve_mtime(false);
    let mut top: Option<PathBuf> = None;
    for entry in archive.entries().context("Das Paket ist kein .tar.gz.")? {
        let mut entry = entry.context("Das Paket ist beschädigt.")?;
        let path = entry.path().context("Das Paket ist beschädigt.")?.into_owned();
        let Some(Component::Normal(first)) = path.components().find(|c| *c != Component::CurDir) else {
            continue; // "./"
        };
        let first = PathBuf::from(first);
        match &top {
            None => top = Some(first),
            Some(t) if *t == first => {}
            Some(_) => bail!("Das Paket hat mehr als einen Ordner; erwartet laptimer-<version>/."),
        }
        if !entry.unpack_in(into).context("Das Paket ist beschädigt.")? {
            bail!("Unzulässiger Pfad im Paket: {}", path.display());
        }
    }
    Ok(into.join(top.context("Das Paket ist leer.")?))
}

/// Used as a folder name: letters, digits, `.`, `-`, `_`, `+`.
pub fn check_version_name(v: &str) -> Result<()> {
    let ok = !v.is_empty()
        && v.len() <= 64
        && !v.starts_with('.')
        && v.chars().all(|c| c.is_ascii_alphanumeric() || "._-+".contains(c));
    if !ok {
        bail!("Ungültige Version im Paket: „{v}“");
    }
    Ok(())
}

/// The program must be for this system and processor: a Linux program (ELF,
/// machine type at byte 18) or a Mac program (Mach-O, CPU type at byte 4).
fn check_binary(path: &Path) -> Result<()> {
    let mut head = [0u8; 20];
    fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .map_err(|_| anyhow!("Im Paket fehlt bin/laptimer."))?;
    let found = if &head[..4] == b"\x7fELF" {
        let arch = match u16::from_le_bytes([head[18], head[19]]) {
            183 => "aarch64",
            62 => "x86_64",
            40 => "arm",
            243 => "riscv64",
            _ => "unbekannt",
        };
        format!("linux-{arch}")
    } else if head[..4] == [0xcf, 0xfa, 0xed, 0xfe] {
        let arch = match u32::from_le_bytes([head[4], head[5], head[6], head[7]]) {
            0x0100_000c => "aarch64",
            0x0100_0007 => "x86_64",
            _ => "unbekannt",
        };
        format!("macos-{arch}")
    } else {
        bail!("bin/laptimer im Paket ist kein Programm für Linux oder macOS.");
    };
    let here = platform();
    if found != here {
        bail!("Das Paket ist für {found}, dieser Rechner ist {here}.");
    }
    Ok(())
}

/// Puts the new release's systemd unit in place if it changed, and tells
/// systemd (the restart after the exit then uses it).
fn update_unit(release: &Path) {
    let Some(home) = std::env::home_dir() else { return };
    let installed = home.join(".config/systemd/user/laptimer.service");
    let Ok(new) = fs::read(release.join("laptimer.service")) else { return };
    if !installed.exists() || fs::read(&installed).is_ok_and(|old| old == new) {
        return;
    }
    if let Err(e) = fs::write(&installed, &new) {
        eprintln!("laptimer: {}: {e}", installed.display());
        return;
    }
    let _ = Command::new("systemctl").args(["--user", "daemon-reload"]).status();
}

/// Keeps the newest KEEP releases, and always `current` and `previous`.
pub fn cleanup(root: &Path) {
    let keep: Vec<String> = ["current", "previous"].iter().filter_map(|n| link(root, n)).collect();
    let Ok(entries) = fs::read_dir(root.join("releases")) else { return };
    let mut dirs: Vec<(std::time::SystemTime, PathBuf, String)> = entries
        .flatten()
        .filter_map(|e| {
            let time = e.metadata().ok()?.modified().ok()?;
            let name = format!("releases/{}", e.file_name().to_string_lossy());
            Some((time, e.path(), name))
        })
        .collect();
    dirs.sort_by_key(|(t, _, _)| std::cmp::Reverse(*t));
    for (i, (_, path, name)) in dirs.iter().enumerate() {
        if (i >= KEEP || name.ends_with(".old")) && !keep.contains(name) {
            let _ = fs::remove_dir_all(path);
        }
    }
}

/// Runs under a service that starts it again after an exit: systemd
/// (`INVOCATION_ID`) or the units `laptimer install` writes
/// (`LAPTIMER_SERVICE`, also launchd).
fn managed() -> bool {
    std::env::var_os("LAPTIMER_SERVICE").is_some() || std::env::var_os("INVOCATION_ID").is_some()
}

/// Ends the server shortly, after the answer got out. A service starts
/// `current` again; without one the server replaces itself with
/// `<root>/current/bin/laptimer` (same arguments).
fn exit_soon(root: Option<&Path>) {
    let next = root.map(|r| r.join("current/bin/laptimer")).filter(|_| !managed());
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        if let Some(program) = next {
            restart_as(&program);
        }
        std::process::exit(0);
    });
}

/// Replaces this process with `program` (exec): same process, same
/// arguments. Open files, the port and the serial ports are closed on exec
/// (Rust, SQLite and serialport open them with O_CLOEXEC), so the new
/// program can open them again. Returns only if exec failed.
#[cfg(unix)]
fn restart_as(program: &Path) {
    use std::os::unix::process::CommandExt;
    println!("laptimer: starte {} neu", program.display());
    let err = Command::new(program).args(std::env::args_os().skip(1)).exec();
    eprintln!("laptimer: Neustart fehlgeschlagen: {err}");
}

#[cfg(not(unix))]
fn restart_as(_program: &Path) {}

/// `install.sh` allows `sudo systemctl poweroff/reboot` without a password
/// (Linux with a service).
fn can_power() -> bool {
    Command::new("sudo")
        .args(["-n", "-l", "/usr/bin/systemctl", "poweroff"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// `systemctl poweroff` or `reboot` (the paths must match the sudoers rule).
fn power(action: &str) -> Result<()> {
    let out = Command::new("sudo")
        .args(["-n", "/usr/bin/systemctl", action])
        .output()
        .context("sudo fehlt")?;
    if !out.status.success() {
        bail!(
            "Das darf der Laptimer nicht (install.sh richtet es ein): {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// "0.2.0" against "0.1.3": by the numbers, then the rest. No suffix is
/// newer than a pre-release ("-rc1"); other suffixes (git describe's
/// "-3-gabc123") are unclear.
pub fn compare(new: &str, current: &str) -> Order {
    use std::cmp::Ordering;
    let new = new.trim_start_matches('v');
    let current = current.trim_start_matches('v');
    if new == current {
        return Order::Same;
    }
    let (a, a_rest) = split_version(new);
    let (b, b_rest) = split_version(current);
    for i in 0..a.len().max(b.len()) {
        match a.get(i).unwrap_or(&0).cmp(b.get(i).unwrap_or(&0)) {
            Ordering::Greater => return Order::Newer,
            Ordering::Less => return Order::Older,
            Ordering::Equal => {}
        }
    }
    let pre = |rest: &str| rest.strip_prefix('-').is_some_and(|r| r.starts_with(|c: char| c.is_ascii_alphabetic()));
    match (a_rest.is_empty(), b_rest.is_empty()) {
        (true, false) if pre(b_rest) => Order::Newer,
        (false, true) if pre(a_rest) => Order::Older,
        _ => Order::Unclear,
    }
}

/// "0.2.0-rc1" → ([0, 2, 0], "-rc1").
fn split_version(v: &str) -> (Vec<u64>, &str) {
    let end = v.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(v.len());
    let numbers = v[..end].split('.').filter_map(|n| n.parse().ok()).collect();
    (numbers, &v[end..])
}

// --- routes ---

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/update", get(get_status))
        .route("/update/upload", post(upload).layer(DefaultBodyLimit::max(MAX_UPLOAD)))
        .route("/update/releases", get(releases))
        .route("/update/download", post(download))
        .route("/update/install", post(install))
        .route("/update/staged", delete(discard))
        .route("/update/rollback", post(rollback))
        .route("/update/dismiss", post(dismiss))
        .route("/system/restart", post(restart))
        .route("/system/reboot", post(|s: St| system_power(s, "reboot")))
        .route("/system/poweroff", post(|s: St| system_power(s, "poweroff")))
}

type St = State<Arc<App>>;

fn bad(e: anyhow::Error) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("{e:#}") }))).into_response()
}

/// Runs `f` off the async threads (files, processes, the core lock).
async fn blocking(app: Arc<App>, f: impl FnOnce(&App) -> Result<Value> + Send + 'static) -> Response {
    match tokio::task::spawn_blocking(move || f(&app)).await {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => bad(e),
        Err(e) => bad(e.into()),
    }
}

async fn get_status(State(app): St) -> Response {
    blocking(app, |app| Ok(app.update.status())).await
}

/// Form fields `package` (the .tar.gz) and `signature` (its .minisig).
async fn upload(State(app): St, mut form: Multipart) -> Response {
    let (mut package, mut signature) = (None, None);
    loop {
        let field = match form.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => return bad(anyhow!("Upload abgebrochen: {e}")),
        };
        let name = field.name().unwrap_or("").to_string();
        let file = field.file_name().unwrap_or("").to_string();
        let data = match field.bytes().await {
            Ok(d) => d,
            Err(e) => return bad(anyhow!("Upload abgebrochen: {e}")),
        };
        match name.as_str() {
            "package" => package = Some((file, data)),
            "signature" => signature = Some(String::from_utf8_lossy(&data).into_owned()),
            _ => {}
        }
    }
    let (Some((file, package)), Some(signature)) = (package, signature) else {
        return bad(anyhow!("Bitte beide Dateien wählen: das Paket (.tar.gz) und seine Signatur (.minisig)."));
    };
    blocking(app, move |app| app.update.stage(&package, &signature, &format!("Upload {file}"))).await
}

async fn releases(State(app): St) -> Response {
    blocking(app, |app| app.update.list_online()).await
}

#[derive(Deserialize)]
struct DownloadRequest {
    tag: String,
}

async fn download(State(app): St, Json(req): Json<DownloadRequest>) -> Response {
    blocking(app, move |app| app.update.download(&req.tag)).await
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct InstallRequest {
    /// The page asked: also an older, the same or an unclear version.
    allow_older: bool,
}

async fn install(State(app): St, Json(req): Json<InstallRequest>) -> Response {
    blocking(app, move |app| app.update.install(app, req.allow_older)).await
}

async fn discard(State(app): St) -> Response {
    blocking(app, |app| app.update.discard()).await
}

async fn rollback(State(app): St) -> Response {
    blocking(app, |app| app.update.rollback(app)).await
}

async fn dismiss(State(app): St) -> Response {
    blocking(app, |app| app.update.dismiss()).await
}

/// Restarts the laptimer: exits, the service starts it again (or it starts
/// itself again without one).
async fn restart(State(app): St) -> Response {
    blocking(app, |app| {
        check_idle(app)?;
        let root = app.update.root()?;
        println!("laptimer: Neustart über die Seite");
        exit_soon(Some(root));
        Ok(json!({}))
    })
    .await
}

async fn system_power(State(app): St, action: &'static str) -> Response {
    blocking(app, move |app| {
        check_idle(app)?;
        power(action)?;
        Ok(json!({}))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert_eq!(compare("0.2.0", "0.1.9"), Order::Newer);
        assert_eq!(compare("0.10.0", "0.9.0"), Order::Newer);
        assert_eq!(compare("0.1.0", "0.2.0"), Order::Older);
        assert_eq!(compare("0.2.0", "0.2.0"), Order::Same);
        assert_eq!(compare("v0.2.0", "0.2.0"), Order::Same);
        assert_eq!(compare("0.2", "0.2.0"), Order::Unclear);
        assert_eq!(compare("0.2.1", "0.2"), Order::Newer);
        // Pre-releases come before the release.
        assert_eq!(compare("0.2.0", "0.2.0-rc1"), Order::Newer);
        assert_eq!(compare("0.2.0-rc1", "0.2.0"), Order::Older);
        assert_eq!(compare("0.2.0-rc2", "0.2.0-rc1"), Order::Unclear);
        // A development build (git describe).
        assert_eq!(compare("0.2.0", "0.1.0-12-gabc1234-dirty"), Order::Newer);
        assert_eq!(compare("0.2.0", "0.2.0-3-gabc1234"), Order::Unclear);
    }

    #[test]
    fn checks_version_names() {
        assert!(check_version_name("0.2.0-rc1").is_ok());
        assert!(check_version_name("../x").is_err());
        assert!(check_version_name(".hidden").is_err());
        assert!(check_version_name("").is_err());
    }

    #[test]
    fn built_in_key_reads() {
        assert!(PublicKey::decode(RELEASE_KEY).is_ok());
    }

    #[test]
    fn reads_sources() {
        let updater = |url: Option<&str>| {
            Updater::new(UpdateConfig { url: url.map(str::to_string), ..Default::default() }).unwrap()
        };
        let s = updater(None).source().unwrap();
        assert!(s.github);
        assert_eq!(s.repo, "bli-blu-bla/fpv-laptimer");
        let s = updater(Some("https://git.example.org/me/laptimer.git/")).source().unwrap();
        assert!(!s.github);
        assert_eq!((s.origin.as_str(), s.repo.as_str()), ("https://git.example.org", "me/laptimer"));
        assert!(updater(Some("")).source().is_err());
        assert!(updater(Some("github.com/x")).source().is_err());
    }

    #[test]
    fn checks_binaries() {
        let dir = std::env::temp_dir().join(format!("laptimer_bin_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("laptimer");
        let check = |head: &[u8]| {
            let mut bytes = head.to_vec();
            bytes.resize(64, 0);
            fs::write(&file, bytes).unwrap();
            check_binary(&file)
        };
        let mut elf = b"\x7fELF".to_vec();
        elf.resize(18, 0);
        let mut macho = vec![0xcf, 0xfa, 0xed, 0xfe];
        let (elf_ok, macho_ok) = match (std::env::consts::OS, std::env::consts::ARCH) {
            ("linux", "x86_64") => (62u16, None),
            ("linux", "aarch64") => (183, None),
            ("macos", "aarch64") => (0, Some(0x0100_000cu32)),
            ("macos", "x86_64") => (0, Some(0x0100_0007)),
            _ => return,
        };
        if let Some(cpu) = macho_ok {
            macho.extend(cpu.to_le_bytes());
            assert!(check(&macho).is_ok());
            elf.extend(183u16.to_le_bytes());
            assert!(check(&elf).is_err());
        } else {
            elf.extend(elf_ok.to_le_bytes());
            assert!(check(&elf).is_ok());
            macho.extend(0x0100_000cu32.to_le_bytes());
            assert!(check(&macho).unwrap_err().to_string().contains("macos-aarch64"));
        }
        assert!(check(b"#!/bin/sh").is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
