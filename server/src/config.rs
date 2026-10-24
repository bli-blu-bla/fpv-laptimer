//! `~/.laptimer/config.toml`: what an installation sets once (host, port, paths,
//! password). Everything else (channels, pilots, …) is in the database and
//! set on the web page.
//!
//! Order: command line, then this file, then the defaults. Paths in the file
//! may start with `~/`; relative ones are relative to the file's folder.
//!
//! Unknown keys (a typo, or a key of a newer version) are ignored with a
//! warning, so an older version still starts with a newer file.
//!
//! If the default file is missing, `laptimer serve` writes one with a
//! random password and prints it. A file given with `--config` must exist.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The address the web server listens on: `localhost` (default, only
    /// this machine) or `0.0.0.0` (all networks, for the phones).
    pub host: Option<String>,
    pub port: Option<u16>,
    /// Database, session levels, backups, hooks (`<data>/hooks`).
    pub data: Option<PathBuf>,
    /// The built web page.
    pub web: Option<PathBuf>,
    /// The announcement MP3s.
    pub sounds: Option<PathBuf>,
    /// The hooks' scripts.
    pub hooks: Option<PathBuf>,
    pub auth: AuthConfig,
    pub update: UpdateConfig,
}

/// Updates from the web page (`update.rs`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    /// The Forgejo repository with the releases, e.g.
    /// `https://git.homeserver.local/bli-blu-bla/LapTimer`. Empty: only uploads.
    pub url: String,
    /// A Forgejo access token (scope read:repository) for a private repository.
    pub token: String,
    /// More public keys (the base64 line of a `minisign.pub`) besides the
    /// built-in one, e.g. for a new signing key.
    pub extra_keys: Vec<String>,
}

/// The password for everything that changes something (`auth.rs`).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    /// On unless switched off with `enabled = false`.
    pub enabled: bool,
    pub password: String,
}

impl Default for AuthConfig {
    fn default() -> Self {
        AuthConfig {
            enabled: true,
            password: String::new(),
        }
    }
}

/// `~/.laptimer`.
pub fn home_dir() -> Result<PathBuf> {
    let home = std::env::home_dir().context("Kein Home-Verzeichnis ($HOME) gefunden")?;
    Ok(home.join(".laptimer"))
}

/// Reads the file. `explicit`: given with `--config` (then it must exist).
pub fn load(path: &Path, explicit: bool) -> Result<Config> {
    if !path.exists() {
        if explicit {
            bail!("{} gibt es nicht", path.display());
        }
        write_default(path)?;
    }
    let text = std::fs::read_to_string(path).with_context(|| format!("{}", path.display()))?;
    let (mut config, unknown) = parse(&text).with_context(|| format!("{}", path.display()))?;
    for key in unknown {
        eprintln!("laptimer: {}: unbekannter Eintrag `{key}` wird ignoriert", path.display());
    }
    if config.auth.enabled && config.auth.password.is_empty() {
        bail!(
            "{}: kein Passwort gesetzt. Unter [auth] `password = \"…\"` eintragen \
             (oder den Schutz ausdrücklich mit `enabled = false` abschalten).",
            path.display()
        );
    }
    // Paths relative to the file's folder.
    let base = path.parent().unwrap_or(Path::new("."));
    for p in [&mut config.data, &mut config.web, &mut config.sounds, &mut config.hooks] {
        if let Some(v) = p.take() {
            *p = Some(resolve(base, &v)?);
        }
    }
    Ok(config)
}

/// The settings and the keys it did not know (e.g. `auth.enable`).
fn parse(text: &str) -> Result<(Config, Vec<String>)> {
    let mut unknown = Vec::new();
    let de = toml::Deserializer::parse(text)?;
    // serde_ignored calls the closure for every key no field took.
    let config = serde_ignored::deserialize(de, |key| unknown.push(key.to_string()))?;
    Ok((config, unknown))
}

/// `~/x` → home/x, `x` → base/x, absolute stays.
fn resolve(base: &Path, p: &Path) -> Result<PathBuf> {
    if let Ok(rest) = p.strip_prefix("~") {
        let home = std::env::home_dir().context("Kein Home-Verzeichnis ($HOME) gefunden")?;
        return Ok(home.join(rest));
    }
    Ok(base.join(p))
}

/// A new file with a random password, readable only by the user.
fn write_default(path: &Path) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("{}", dir.display()))?;
    }
    let password = random_password()?;
    let text = format!(
        "# Laptimer: Einstellungen der Installation (alles andere auf der Web-Seite).\n\
         # Nach einer Änderung neu starten: systemctl --user restart laptimer\n\
         \n\
         # Erreichbar von: \"localhost\" (nur dieser Rechner) oder \"0.0.0.0\" (alle Netze, Handys)\n\
         host = \"0.0.0.0\"\n\
         # port = 5800\n\
         # data = \"~/.laptimer/data\"      # Datenbank, Pegel, Backups, hooks/\n\
         \n\
         [auth]\n\
         # Passwort für alles, was etwas ändert (Rennleitung, Einstellungen, …).\n\
         # Abschalten nur ausdrücklich mit: enabled = false\n\
         enabled = true\n\
         password = \"{password}\"\n\
         \n\
         [update]\n\
         # Updates von Forgejo (Einstellungen → System → Nach Updates suchen).\n\
         # Ohne url: nur Hochladen. Token (read:repository) für ein privates Repo.\n\
         # url = \"https://git.homeserver.local/bli-blu-bla/LapTimer\"\n\
         # token = \"\"\n"
    );
    write_private(path, &text)?;
    eprintln!(
        "laptimer: {} angelegt, Passwort für die Rennleitung: {password}",
        path.display()
    );
    Ok(())
}

#[cfg(unix)]
fn write_private(path: &Path, text: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("{}", path.display()))?;
    f.write_all(text.as_bytes())?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private(path: &Path, text: &str) -> Result<()> {
    Ok(std::fs::write(path, text)?)
}

/// 12 letters and digits without look-alikes (l/1, o/0): easy to type on a phone.
fn random_password() -> Result<String> {
    const CHARS: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let bytes = crate::auth::random_bytes::<12>()?;
    Ok(bytes.iter().map(|b| CHARS[*b as usize % CHARS.len()] as char).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_resolves() {
        let dir = std::env::temp_dir().join(format!("laptimer_config_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "host = \"0.0.0.0\"\nport = 81\ndata = \"d\"\n[auth]\npassword = \"x\"\n").unwrap();
        let c = load(&path, true).unwrap();
        assert_eq!(c.host.as_deref(), Some("0.0.0.0"));
        assert_eq!(c.port, Some(81));
        assert_eq!(c.data, Some(dir.join("d")));
        assert!(c.auth.enabled);

        // On, but no password: refused.
        std::fs::write(&path, "[auth]\n").unwrap();
        assert!(load(&path, true).is_err());
        // Off only on purpose.
        std::fs::write(&path, "[auth]\nenabled = false\n").unwrap();
        assert!(!load(&path, true).unwrap().auth.enabled);
        // A typo is ignored, with a warning (here: still no password).
        let (c, unknown) = parse("colour = 1\n[auth]\nenable = false\npassword = \"x\"\n").unwrap();
        assert!(c.auth.enabled);
        assert_eq!(unknown, ["auth.enable", "colour"]);
        std::fs::write(&path, "[auth]\nenable = false\n").unwrap();
        assert!(load(&path, true).is_err());

        // Missing default file: written with a password.
        let fresh = dir.join("sub").join("config.toml");
        let c = load(&fresh, false).unwrap();
        assert_eq!(c.auth.password.len(), 12);
        assert_eq!(c.host.as_deref(), Some("0.0.0.0"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
