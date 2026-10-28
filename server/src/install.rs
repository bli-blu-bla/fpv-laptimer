//! `laptimer install`, `rollback`, `uninstall`: the installation in
//! `~/.laptimer`. `install.sh` downloads and unpacks a package, does what
//! needs sudo (Linux: packages, groups, linger, the rule for shutting down)
//! and then runs `bin/laptimer install` from the unpacked package. That
//! works by hand too, e.g. from a package copied over without internet.
//!
//! ```text
//! ~/.laptimer/
//!   config.toml          host, port, password (made on the first install)
//!   current  -> releases/<ver>   the version that runs
//!   previous -> releases/<ver>   the one before (rollback)
//!   releases/<ver>/      the package: bin/, web/, sounds/, … (3 are kept)
//!   data/                database, levels, backups, hooks/
//! ```
//!
//! An installed version is updated: the package becomes `current`, the old
//! one `previous`; config and data stay.
//!
//! Optionally a service, which starts the laptimer at boot and again after
//! it exits (an update from the page ends the server): a systemd user unit
//! on Linux (`~/.config/systemd/user/laptimer.service`, the package's
//! `laptimer.service`), a launchd agent on the Mac
//! (`~/Library/LaunchAgents/local.laptimer.plist`, written here). Without a
//! service the server starts the new version itself after an update
//! (`update.rs`), but nothing starts it at boot.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::config;
use crate::update::{check_version_name, cleanup, link, set_link, version_of};

pub struct InstallOptions {
    /// `Some(true)`: set up the service, `Some(false)`: remove it, `None`:
    /// keep what is there (none on the first install).
    pub service: Option<bool>,
    /// For a new `config.toml`; `None`: a random one (printed).
    pub password: Option<String>,
}

/// Installs the package this program belongs to (`<package>/bin/laptimer`).
pub fn install(opts: InstallOptions) -> Result<()> {
    let root = config::home_dir()?;
    let package = package_dir()?;
    let version = fs::read_to_string(package.join("VERSION"))
        .map(|v| v.trim().to_string())
        .with_context(|| format!("{}/VERSION fehlt: `laptimer install` aus einem entpackten Paket starten", package.display()))?;
    check_version_name(&version)?;
    let name = format!("releases/{version}");
    let old = link(&root, "current");
    match old.as_deref() {
        None => say(&format!("Laptimer {version} wird installiert ({})", root.display())),
        Some(o) if o == name => say(&format!("Laptimer {version} ist schon installiert, wird neu eingerichtet")),
        Some(o) => say(&format!("Update von {} auf {version}", version_of(o))),
    }

    // The package to releases/<ver> (unless it runs from there already).
    fs::create_dir_all(root.join("releases"))?;
    let dest = root.join(&name);
    if dest.canonicalize().ok() != Some(package.clone()) {
        let tmp = root.join(format!("{name}.new"));
        let _ = fs::remove_dir_all(&tmp);
        copy_dir(&package, &tmp)?;
        if dest.exists() {
            // The same version again. A running program keeps its open
            // files, so the old folder can go.
            let gone = root.join(format!("{name}.old"));
            let _ = fs::remove_dir_all(&gone);
            fs::rename(&dest, &gone).with_context(|| format!("{}", dest.display()))?;
            let _ = fs::remove_dir_all(&gone);
        }
        fs::rename(&tmp, &dest).with_context(|| format!("{}", dest.display()))?;
    }
    if let Some(o) = old.as_deref().filter(|o| *o != name) {
        set_link(&root, "previous", o)?;
    }
    set_link(&root, "current", &name)?;
    // Installed by hand: a pending update from the page is over.
    let _ = fs::remove_file(root.join("update-pending"));
    let _ = fs::remove_file(root.join("update-failed"));

    let config_path = root.join("config.toml");
    if !config_path.exists() {
        config::create(&config_path, opts.password.as_deref())?;
    }
    fs::create_dir_all(root.join("data"))?;
    cleanup(&root);

    let had = service_installed();
    let service = opts.service.unwrap_or(had);
    if service {
        install_service(&dest)?;
    } else if had {
        remove_service()?;
        say("Dienst entfernt");
    }

    // Where to find it.
    let cfg = config::load(&config_path, true)?;
    let port = cfg.port.unwrap_or(5800);
    let host = cfg.host.unwrap_or_else(|| "localhost".to_string());
    if host != "0.0.0.0" && host != "::" {
        say(&format!(
            "Achtung: host = \"{host}\" in {}, Handys erreichen den Server nicht (host = \"0.0.0.0\")",
            config_path.display()
        ));
    }
    if service {
        say(&format!("Läuft: http://{}.local:{port}", hostname()));
        if let Some(ip) = local_ip() {
            println!("  oder http://{ip}:{port}");
        }
        if cfg!(target_os = "macos") {
            println!("  Log: {}/laptimer.log", root.display());
        } else {
            println!("  Status: systemctl --user status laptimer   Log: journalctl --user -u laptimer -f");
        }
    } else {
        say(&format!("Starten: {}/current/bin/laptimer serve", root.display()));
        println!("  dann http://localhost:{port} (läuft er schon, einmal neu starten)");
    }
    Ok(())
}

/// Back to `previous` (`current` becomes `previous`), restarts the service.
pub fn rollback() -> Result<()> {
    let root = config::home_dir()?;
    let prev = link(&root, "previous")
        .filter(|p| root.join(p).join("bin/laptimer").is_file())
        .context("Es gibt keine vorige Version.")?;
    let cur = link(&root, "current").context("~/.laptimer/current fehlt.")?;
    let _ = fs::remove_file(root.join("update-pending"));
    set_link(&root, "current", &prev)?;
    set_link(&root, "previous", &cur)?;
    if service_installed() {
        restart_service()?;
    }
    say(&format!("Zurück auf {} (vorher {})", version_of(&prev), version_of(&cur)));
    Ok(())
}

/// Stops and removes the service; releases, config and data stay.
pub fn uninstall() -> Result<()> {
    let root = config::home_dir()?;
    if service_installed() {
        remove_service()?;
        say("Dienst entfernt.");
    } else {
        say("Kein Dienst eingerichtet.");
    }
    println!(
        "Versionen, Einstellungen und Daten liegen weiter in {} (von Hand löschen, wenn gewünscht).",
        root.display()
    );
    Ok(())
}

/// `<package>` of `<package>/bin/laptimer` (this program).
fn package_dir() -> Result<PathBuf> {
    let exe = std::env::current_exe()?.canonicalize()?;
    let package = exe.parent().and_then(Path::parent).context("Programmordner unbekannt")?;
    Ok(package.to_path_buf())
}

/// Copies a folder with everything in it (files keep their permissions).
fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to).with_context(|| format!("{}", to.display()))?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_dir(&src, &dst)?;
        } else if kind.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(fs::read_link(&src)?, &dst)?;
        } else {
            fs::copy(&src, &dst).with_context(|| format!("{}", src.display()))?;
        }
    }
    Ok(())
}

fn say(text: &str) {
    println!("\x1b[1m{text}\x1b[0m");
}

/// Runs a command; an error with its output if it fails.
fn run(cmd: &mut Command) -> Result<()> {
    let out = cmd.output().with_context(|| format!("{cmd:?}"))?;
    if !out.status.success() {
        bail!("{cmd:?}: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// The user's id (`id -u`), for systemd's runtime folder and launchd's domain.
fn uid() -> Result<String> {
    let out = Command::new("id").arg("-u").output().context("id -u")?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `laptimer` of `laptimer.local`.
fn hostname() -> String {
    let out = Command::new("hostname").output().ok();
    let name = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let name = name.split('.').next().unwrap_or("").to_string();
    if name.is_empty() { "localhost".to_string() } else { name }
}

/// The first address of `hostname -I` (Linux; the Mac has no such option).
fn local_ip() -> Option<String> {
    let out = Command::new("hostname").arg("-I").output().ok().filter(|o| o.status.success())?;
    String::from_utf8_lossy(&out.stdout).split_whitespace().next().map(str::to_string)
}

fn home() -> Result<PathBuf> {
    std::env::home_dir().context("Kein Home-Verzeichnis ($HOME) gefunden")
}

// --- the service: systemd (Linux) ---

#[cfg(not(target_os = "macos"))]
fn unit_path() -> Result<PathBuf> {
    Ok(home()?.join(".config/systemd/user/laptimer.service"))
}

#[cfg(not(target_os = "macos"))]
fn service_installed() -> bool {
    unit_path().is_ok_and(|p| p.exists())
}

/// `systemctl --user …`. Over ssh or `sudo -u` the user's systemd is only
/// found with XDG_RUNTIME_DIR.
#[cfg(not(target_os = "macos"))]
fn systemctl(args: &[&str]) -> Result<()> {
    let mut cmd = Command::new("systemctl");
    cmd.arg("--user").args(args);
    if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
        cmd.env("XDG_RUNTIME_DIR", format!("/run/user/{}", uid()?));
    }
    run(&mut cmd)
}

/// The package's unit to `~/.config/systemd/user/`, enabled and (re)started.
#[cfg(not(target_os = "macos"))]
fn install_service(release: &Path) -> Result<()> {
    let unit = unit_path()?;
    fs::create_dir_all(unit.parent().unwrap())?;
    fs::copy(release.join("laptimer.service"), &unit).with_context(|| format!("{}", unit.display()))?;
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "laptimer.service"])?;
    systemctl(&["restart", "laptimer.service"])
}

#[cfg(not(target_os = "macos"))]
fn remove_service() -> Result<()> {
    let _ = systemctl(&["disable", "--now", "laptimer.service"]);
    fs::remove_file(unit_path()?)?;
    let _ = systemctl(&["daemon-reload"]);
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn restart_service() -> Result<()> {
    systemctl(&["restart", "laptimer.service"])
}

// --- the service: launchd (Mac) ---

#[cfg(target_os = "macos")]
const LABEL: &str = "local.laptimer";

#[cfg(target_os = "macos")]
fn plist_path() -> Result<PathBuf> {
    Ok(home()?.join(format!("Library/LaunchAgents/{LABEL}.plist")))
}

#[cfg(target_os = "macos")]
fn service_installed() -> bool {
    plist_path().is_ok_and(|p| p.exists())
}

/// An agent of the logged-in user: starts at login and again after every
/// exit (KeepAlive), like `Restart=always`. Runs `update-check.sh` first,
/// like the systemd unit's `ExecStartPre`.
#[cfg(target_os = "macos")]
fn install_service(_release: &Path) -> Result<()> {
    let root = config::home_dir()?;
    let r = xml_escape(&root.to_string_lossy());
    let start = format!(
        "/bin/sh '{r}/current/update-check.sh'; exec '{r}/current/bin/laptimer' serve"
    );
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<!-- The laptimer as a launchd agent (laptimer install). Log: {r}/laptimer.log -->
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array><string>/bin/sh</string><string>-c</string><string>{start}</string></array>
  <key>EnvironmentVariables</key>
  <dict><key>LAPTIMER_SERVICE</key><string>launchd</string></dict>
  <key>WorkingDirectory</key><string>{r}</string>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>{r}/laptimer.log</string>
  <key>StandardErrorPath</key><string>{r}/laptimer.log</string>
</dict>
</plist>
"#
    );
    let path = plist_path()?;
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(&path, plist).with_context(|| format!("{}", path.display()))?;
    let domain = format!("gui/{}", uid()?);
    // Loaded already (an update): unload first, then load the new file.
    let _ = run(Command::new("launchctl").args(["bootout", &format!("{domain}/{LABEL}")]));
    // bootout returns before the agent is gone; bootstrap fails meanwhile.
    let mut last = Ok(());
    for _ in 0..10 {
        last = run(Command::new("launchctl").arg("bootstrap").arg(&domain).arg(&path));
        if last.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    last.context("launchctl bootstrap: der Dienst braucht einen angemeldeten Nutzer am Mac")
}

#[cfg(target_os = "macos")]
fn remove_service() -> Result<()> {
    let _ = run(Command::new("launchctl").args(["bootout", &format!("gui/{}/{LABEL}", uid()?)]));
    fs::remove_file(plist_path()?)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn restart_service() -> Result<()> {
    run(Command::new("launchctl").args(["kickstart", "-k", &format!("gui/{}/{LABEL}", uid()?)]))
}

#[cfg(target_os = "macos")]
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_folders() {
        let dir = std::env::temp_dir().join(format!("laptimer_install_{}", std::process::id()));
        let from = dir.join("a");
        fs::create_dir_all(from.join("bin")).unwrap();
        fs::write(from.join("VERSION"), "1.0\n").unwrap();
        fs::write(from.join("bin/x"), "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(from.join("bin/x"), fs::Permissions::from_mode(0o755)).unwrap();
        }
        copy_dir(&from, &dir.join("b")).unwrap();
        assert_eq!(fs::read_to_string(dir.join("b/VERSION")).unwrap(), "1.0\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.join("b/bin/x")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755);
        }
        fs::remove_dir_all(&dir).unwrap();
    }
}
