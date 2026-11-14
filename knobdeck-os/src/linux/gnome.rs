//! GNOME: talk to the KnobDeck Shell extension (`gnome-extension/`), and install it.
//!
//! Linux Tools installs it (`ensure_installed`) when it starts; the window functions of this crate
//! then work on GNOME, in KnobDeck itself as well as in plugins.
//!
//! GNOME Shell keeps window management to itself on Wayland and restricts its own D-Bus methods
//! (Eval, Introspect, FocusSearch, ShowApplications) to GNOME's components, so a Shell extension
//! is the only supported way in. Linux Tools installs it into the user's extensions folder and
//! enables it; GNOME loads new extensions at login, so the first install needs one log out/in.

use super::bus;
use crate::{Error, Result};
use serde_json::Value;
use std::path::PathBuf;

pub const UUID: &str = "knobdeck@knobdeck.harshalkudale.com";
const BUS_NAME: &str = "dev.knobdeck.Shell";
const PATH: &str = "/dev/knobdeck/Shell";
const EXTENSION_JS: &str = include_str!("gnome-extension/extension.js");
const METADATA_JSON: &str = include_str!("gnome-extension/metadata.json");

/// The extension version this build ships (`version` in metadata.json).
pub fn bundled_version() -> u32 {
    serde_json::from_str::<Value>(METADATA_JSON).ok().and_then(|m| m["version"].as_u64()).unwrap_or(0) as u32
}

/// The running extension's version, or `None` when GNOME hasn't loaded it.
pub fn running_version() -> Option<u32> {
    let conn = bus::session().ok()?;
    bus::get::<u32>(&conn, BUS_NAME, PATH, BUS_NAME, "Version").ok()
}

/// Run a bridge command in GNOME Shell.
pub fn call(cmd: &str, args: &Value) -> Result<Value> {
    let conn = bus::session()?;
    let reply: String = match bus::call(&conn, BUS_NAME, PATH, BUS_NAME, "Call", &(cmd, args.to_string())) {
        Ok(r) => r,
        Err(_) if !bus::session_has_owner(BUS_NAME) => return Err(not_running()),
        Err(e) => return Err(e),
    };
    let v: Value = serde_json::from_str(&reply).map_err(|_| Error::msg("The KnobDeck GNOME extension sent a bad reply"))?;
    match v["error"].as_str() {
        Some(e) => Err(Error::msg(e)),
        None => Ok(v["ok"].clone()),
    }
}

fn not_running() -> Error {
    if extension_dir().join("metadata.json").is_file() {
        Error::msg("The KnobDeck GNOME extension isn't running yet: log out and back in once (or turn it on in the Extensions app)")
    } else {
        Error::msg("Window actions on GNOME need the KnobDeck GNOME extension: install the Linux Tools plugin, then log out and back in once")
    }
}

fn extension_dir() -> PathBuf {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share"));
    data.join("gnome-shell/extensions").join(UUID)
}

/// What `ensure_installed` did.
#[derive(Debug, PartialEq, Eq)]
pub enum Install {
    /// The current version is loaded and answering.
    Running,
    /// Files were written (new or updated); GNOME picks them up at the next login.
    NeedsRelogin,
}

/// Install or update the extension's files and enable it. Safe to call at every start.
pub fn ensure_installed() -> Result<Install> {
    let want = bundled_version();
    if running_version().is_some_and(|v| v >= want) {
        return Ok(Install::Running);
    }
    let dir = extension_dir();
    let current = std::fs::read_to_string(dir.join("metadata.json")).ok().and_then(|m| serde_json::from_str::<Value>(&m).ok()).and_then(|m| m["version"].as_u64());
    if current != Some(want as u64) || !dir.join("extension.js").is_file() {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("extension.js"), EXTENSION_JS)?;
        std::fs::write(dir.join("metadata.json"), METADATA_JSON)?;
    }
    enable()?;
    Ok(if running_version().is_some_and(|v| v >= want) { Install::Running } else { Install::NeedsRelogin })
}

/// Add the extension to `org.gnome.shell enabled-extensions` (what the Extensions app does).
fn enable() -> Result {
    // The Shell's own call works when it already knows the extension (an update, or X11 after a restart).
    if let Ok(conn) = bus::session() {
        if bus::call::<_, bool>(&conn, "org.gnome.Shell.Extensions", "/org/gnome/Shell/Extensions", "org.gnome.Shell.Extensions", "EnableExtension", &(UUID,)).unwrap_or(false) {
            return Ok(());
        }
    }
    let out = std::process::Command::new("gsettings").args(["get", "org.gnome.shell", "enabled-extensions"]).output()?;
    let current = String::from_utf8_lossy(&out.stdout).to_string();
    if current.contains(UUID) {
        return Ok(());
    }
    let list = with_uuid(&current);
    let ok = std::process::Command::new("gsettings").args(["set", "org.gnome.shell", "enabled-extensions", &list]).status().is_ok_and(|s| s.success());
    if !ok {
        return Err(Error::msg("Could not enable the KnobDeck GNOME extension (gsettings failed)"));
    }
    Ok(())
}

/// `gsettings` list text with our UUID appended: `['a', 'b']`, `@as []`.
fn with_uuid(current: &str) -> String {
    let inner = current.trim().trim_start_matches("@as").trim().trim_start_matches('[').trim_end_matches(']').trim();
    if inner.is_empty() {
        format!("['{UUID}']")
    } else {
        format!("[{inner}, '{UUID}']")
    }
}

// ── Watch ───────────────────────────────────────────────────────────────────

/// Listens for the extension's `WindowChanged` signal on its own connection. Dropping it emits
/// that signal once from the same connection, which wakes the listener so it can stop (other
/// listeners just read the window once more).
pub struct Watch {
    conn: zbus::blocking::Connection,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

pub fn watch(tick: std::sync::mpsc::Sender<()>) -> Result<Watch> {
    use std::sync::atomic::Ordering;
    if !bus::session_has_owner(BUS_NAME) {
        return Err(not_running());
    }
    let conn = zbus::blocking::Connection::session().map_err(bus::err)?;
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface(BUS_NAME)
        .and_then(|b| b.member("WindowChanged"))
        .map_err(bus::err)?
        .build();
    let messages = zbus::blocking::MessageIterator::for_match_rule(rule, &conn, Some(16)).map_err(bus::err)?;
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped = stop.clone();
    std::thread::Builder::new()
        .name("knobdeck-os-gnome-watch".into())
        .spawn(move || {
            for m in messages {
                if stopped.load(Ordering::Relaxed) || m.is_err() || tick.send(()).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| Error::msg(format!("Could not start the window watcher: {e}")))?;
    Ok(Watch { conn, stop })
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = self.conn.emit_signal(None::<&str>, PATH, BUS_NAME, "WindowChanged", &());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_uuid_to_gsettings_lists() {
        assert_eq!(with_uuid("@as []\n"), format!("['{UUID}']"));
        assert_eq!(with_uuid("['a@b']\n"), format!("['a@b', '{UUID}']"));
    }

    #[test]
    fn bundled_metadata_is_valid() {
        assert!(bundled_version() >= 1);
        let m: Value = serde_json::from_str(METADATA_JSON).unwrap();
        assert_eq!(m["uuid"], UUID);
        assert!(EXTENSION_JS.contains(BUS_NAME) && EXTENSION_JS.contains(PATH));
    }
}
