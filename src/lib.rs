//! # knobdeck-sdk
//!
//! Declarative Rust SDK for authoring [KnobDeck](https://github.com/HarshalKudale/knobdeck)
//! plugins — the Rust counterpart of `@knobdeck/sdk`. A plugin is a native
//! executable: declare actions with [`Plugin`] / [`Action`], call `.run()`, and
//! let the `knobdeck-plugin` CLI generate `manifest.json` / `actions.json`.
//!
//! The host spawns the plugin and talks to it over stdio (protocol v2). Handlers
//! are plain synchronous closures: invokes run in order per instance, queries
//! and events in parallel, so a slow handler never blocks the connection; the
//! host ack is sent before the handler starts.

mod codegen;
mod ctx;
mod error;
mod host;
pub mod input;
mod log;
#[cfg(feature = "oauth")]
pub mod oauth;
mod plugin;
mod runtime;
mod setup;

pub use codegen::{make_full_id, Stage};
pub use ctx::{Ctx, Event, InitialCtx, OpenCtx, QueryCtx};
pub use error::{Error, Result};
pub use host::{focus_app, installed_apps, send_keys};
pub use log::log;
pub use plugin::{Action, Kind, Plugin};
pub use runtime::{inputs_match, Runtime, Visual, FEATURE_STATE_MATCH, PROTOCOL};
pub use serde_json::{json, Value};
pub use setup::{run_setup, SetupCtx};

use std::path::{Path, PathBuf};

/// Tell the host this plugin needs setup again (expired token, revoked auth…).
/// Inside a handler prefer `ctx.mark_setup_required(reason)`.
pub fn mark_setup_required(reason: impl ToString) {
    match runtime::global() {
        Some(rt) => rt.mark_setup_required(reason),
        None => log("mark_setup_required: not connected to the host"),
    }
}

/// This plugin's persistent data folder (`%APPDATA%\KnobDeck\plugin-data\<id>` on Windows,
/// `~/Library/Application Support/KnobDeck/plugin-data/<id>` on macOS):
/// survives updates and reinstalls. Works in the plugin (from the host's
/// `hello`) and in setup programs (`KNOBDECK_PLUGIN_DATA`); `.` otherwise.
pub fn data_dir() -> PathBuf {
    if let Some(rt) = runtime::global() {
        return rt.data_dir().to_path_buf();
    }
    std::env::var_os("KNOBDECK_PLUGIN_DATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

/// `data_dir()/name`. If the file still sits in the plugin folder (where
/// pre-0.3 plugins kept it) and not in the data dir yet, it is moved over once.
pub fn data_file(name: &str) -> PathBuf {
    let target = data_dir().join(name);
    let legacy = Path::new(name);
    if !target.exists() && legacy.is_file() && legacy.canonicalize().ok() != target.canonicalize().ok() {
        if std::fs::rename(legacy, &target).is_err() {
            // Different volume: copy, then drop the old copy.
            if std::fs::copy(legacy, &target).is_ok() {
                let _ = std::fs::remove_file(legacy);
            }
        }
        log(format!("moved {name} into the plugin data folder"));
    }
    target
}

/// Load `KEY=VALUE` lines from `.env` in the plugin's data folder (moved there
/// from the plugin folder on first run) into the process environment. Existing
/// variables win. Missing file is fine.
pub fn load_env() {
    let Ok(text) = std::fs::read_to_string(data_file(".env")) else { return };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim();
        let v = v
            .strip_prefix('"').and_then(|s| s.strip_suffix('"'))
            .or_else(|| v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
            .unwrap_or(v);
        if std::env::var_os(k.trim()).is_none() {
            std::env::set_var(k.trim(), v);
        }
    }
}

/// The OS this plugin runs on, as the host names it: `"windows"`, `"macos"` or `"linux"`
/// (`KNOBDECK_PLATFORM` from the host, else the compile-time OS).
pub fn platform() -> &'static str {
    match std::env::var("KNOBDECK_PLATFORM").as_deref() {
        Ok("windows") => "windows",
        Ok("macos") => "macos",
        Ok("linux") => "linux",
        _ => std::env::consts::OS,
    }
}

/// `true` on macOS. Use it to pick per-OS key chords: `if is_mac() { "cmd t" } else { "ctrl t" }`
/// (or send the `mod` token, which is ⌘ on macOS and Ctrl on Windows and Linux).
pub fn is_mac() -> bool {
    platform() == "macos"
}

/// `true` on Linux.
pub fn is_linux() -> bool {
    platform() == "linux"
}

/// `env_or("VLC_PORT", "8080")` — env var with a default (empty counts as unset).
pub fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| default.into())
}

/// Write `.env` in the plugin's data folder (what [`load_env`] reads), replacing it.
/// For setup programs: `save_env(&[("VLC_HOST", &host), ("VLC_PORT", &port)])?`.
/// Values are written as they are; a line break in one is refused (it would split the line).
pub fn save_env(pairs: &[(&str, &str)]) -> Result<()> {
    let mut text = String::new();
    for (k, v) in pairs {
        if v.contains(['\n', '\r']) || k.contains(['\n', '\r', '=']) {
            return Err(Error::msg(format!("{k}: line breaks are not allowed")));
        }
        text.push_str(&format!("{k}={v}\n"));
    }
    std::fs::write(data_dir().join(".env"), text)?;
    Ok(())
}

/// Open a URL in the default browser (or any URI with its registered app).
pub fn open_url(url: &str) -> Result<()> {
    // url.dll hands the URL to the shell as one argument, so `&` and spaces need no cmd quoting.
    let status = if cfg!(windows) {
        std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).status()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).status()
    } else {
        // freedesktop: hands the URL to the desktop's handler (GNOME, KDE and others).
        std::process::Command::new("xdg-open").arg(url).status()
    };
    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(Error::msg(format!("Could not open {url} ({s})"))),
        Err(e) => Err(Error::msg(format!("Could not open {url}: {e}"))),
    }
}

/// Tests that point `KNOBDECK_PLUGIN_DATA` (process-wide) at their own folder hold this.
#[cfg(test)]
pub(crate) static DATA_DIR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    #[test]
    fn save_env_round_trips_and_refuses_line_breaks() {
        let _guard = super::DATA_DIR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("kd-env-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("KNOBDECK_PLUGIN_DATA", &dir);
        super::save_env(&[("KD_TEST_A", "x=y"), ("KD_TEST_B", "")]).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join(".env")).unwrap(), "KD_TEST_A=x=y\nKD_TEST_B=\n");
        super::load_env();
        assert_eq!(std::env::var("KD_TEST_A").unwrap(), "x=y");
        assert!(super::save_env(&[("K", "a\nb")]).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
