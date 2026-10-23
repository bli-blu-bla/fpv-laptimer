//! Sets `LAPTIMER_VERSION` for the program (`main.rs`: `VERSION`).
//!
//! The release build (CI) sets it itself, e.g. `0.2.0`. Otherwise it is
//! `git describe` of the repository (`v0.1.0-12-gabc1234-dirty`), or the
//! Cargo version if there is no git.

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=LAPTIMER_VERSION");
    // A new commit or tag changes the version.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs");
    if std::env::var("LAPTIMER_VERSION").is_ok_and(|v| !v.is_empty()) {
        return; // passed through as it is
    }
    let git = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());
    let version = match git {
        // Without a tag, describe gives only the commit: put the Cargo version in front.
        Some(g) if g.starts_with('v') => g.trim_start_matches('v').to_string(),
        Some(g) => format!("{}-{g}", env!("CARGO_PKG_VERSION")),
        None => env!("CARGO_PKG_VERSION").to_string(),
    };
    println!("cargo:rustc-env=LAPTIMER_VERSION={version}");
}
