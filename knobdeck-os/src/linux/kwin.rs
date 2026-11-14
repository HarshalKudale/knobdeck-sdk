//! KDE Plasma: run a bridge command as a one-shot KWin script.
//!
//! KWin's scripting API (`workspace`, `Window`) is the only interface that sees and moves every
//! window on Plasma Wayland. The script is loaded over `org.kde.kwin.Scripting`, run once, and
//! hands its result back by calling `Reply` on an object this process serves on the session bus
//! (KWin scripts can call D-Bus but not return values to the loader). Then it is unloaded.
//! [`watch`] keeps a second script loaded that reports window changes the same way.

use super::bus;
use crate::{Error, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};
use zbus::blocking::Connection;

const LIB: &str = include_str!("kwin.js");
const PATH: &str = "/dev/knobdeck/KWinBridge";
const IFACE: &str = "dev.knobdeck.KWinBridge";
const TIMEOUT: Duration = Duration::from_secs(3);

/// Replies by token: (JSON value, error text).
static REPLIES: Mutex<Option<HashMap<String, (String, String)>>> = Mutex::new(None);
static ARRIVED: Condvar = Condvar::new();
static SEQ: AtomicU64 = AtomicU64::new(0);

struct Bridge;

#[zbus::interface(name = "dev.knobdeck.KWinBridge")]
impl Bridge {
    fn reply(&self, token: String, value: String, error: String) {
        REPLIES.lock().unwrap().get_or_insert_with(HashMap::new).insert(token, (value, error));
        ARRIVED.notify_all();
    }
}

fn serve(conn: &Connection) -> Result {
    static SERVED: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    SERVED
        .get_or_init(|| conn.object_server().at(PATH, Bridge).map(|_| ()).map_err(|e| e.to_string()))
        .clone()
        .map_err(|e| Error::msg(format!("Could not register with D-Bus: {e}")))
}

fn script_dir() -> std::path::PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(Into::into).filter(|p: &std::path::PathBuf| p.is_dir()).unwrap_or_else(std::env::temp_dir)
}

/// The JS that `kwin.js` runs with: the command, its arguments and the reply callback.
fn script(cmd: &str, args: &Value, token: &str, service: &str) -> String {
    format!(
        "const CMD = {cmd}; const ARGS = {args}; const TOKEN = {token};\n\
         function reply(v, e) {{ callDBus({service}, {path}, {iface}, \"Reply\", TOKEN, JSON.stringify(v === undefined ? null : v), e); }}\n{LIB}",
        cmd = json!(cmd),
        token = json!(token),
        service = json!(service),
        path = json!(PATH),
        iface = json!(IFACE),
    )
}

pub fn call(cmd: &str, args: &Value) -> Result<Value> {
    let conn = bus::session()?;
    serve(&conn)?;
    let service = conn.unique_name().ok_or_else(|| Error::msg("D-Bus: no unique name"))?.to_string();
    let token = format!("{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed));
    let plugin = format!("knobdeck-{token}");
    let file = script_dir().join(format!("{plugin}.js"));
    std::fs::write(&file, script(cmd, args, &token, &service))?;
    let result = run(&conn, &file, &plugin, &token);
    unload(&conn, &plugin);
    let _ = std::fs::remove_file(&file);
    result
}

/// Load and start a script file under the name `plugin`.
fn load(conn: &Connection, file: &std::path::Path, plugin: &str) -> Result {
    let path = file.to_string_lossy();
    let id: i32 = bus::call(conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting", "loadScript", &(path.as_ref(), plugin))
        .map_err(|e| Error::msg(format!("KWin scripting is not available: {e}")))?;
    if id < 0 {
        return Err(Error::msg("KWin refused to load the window script"));
    }
    // Plasma 6 puts scripts at /Scripting/Script<id>; Plasma 5 used /<id>.
    if bus::call0(conn, "org.kde.KWin", &format!("/Scripting/Script{id}"), "org.kde.kwin.Script", "run", &()).is_err() {
        bus::call0(conn, "org.kde.KWin", &format!("/{id}"), "org.kde.kwin.Script", "run", &())?;
    }
    Ok(())
}

fn unload(conn: &Connection, plugin: &str) {
    let _ = bus::call::<_, bool>(conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting", "unloadScript", &(plugin,));
}

fn run(conn: &Connection, file: &std::path::Path, plugin: &str, token: &str) -> Result<Value> {
    load(conn, file, plugin)?;
    let deadline = Instant::now() + TIMEOUT;
    let mut replies = REPLIES.lock().unwrap();
    loop {
        if let Some((value, error)) = replies.get_or_insert_with(HashMap::new).remove(token) {
            if !error.is_empty() {
                return Err(Error::msg(error));
            }
            return Ok(serde_json::from_str(&value).unwrap_or(Value::Null));
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Error::msg("KWin did not answer"));
        }
        replies = ARRIVED.wait_timeout(replies, left).unwrap().0;
    }
}

// ── Watch ───────────────────────────────────────────────────────────────────

const WATCH_JS: &str = include_str!("kwin_watch.js");
const WATCH_IFACE: &str = "dev.knobdeck.KWinWatch";

/// Live watchers by id: the KWin script of each calls `Changed` on its own object.
static WATCHERS: Mutex<Option<HashMap<u64, Sender<()>>>> = Mutex::new(None);

struct WatchObject(u64);

#[zbus::interface(name = "dev.knobdeck.KWinWatch")]
impl WatchObject {
    fn changed(&self) {
        if let Some(tx) = WATCHERS.lock().unwrap().as_ref().and_then(|w| w.get(&self.0)) {
            let _ = tx.send(());
        }
    }
}

/// A KWin script that stays loaded and calls back when the active window changes, or the active
/// window's caption, maximize, full screen, minimize or keep-above state does. Unloaded on drop.
/// (If KWin itself restarts, the script is gone: the watcher then reports nothing until it is
/// made again.)
pub struct Watch {
    id: u64,
    plugin: String,
    path: String,
    file: std::path::PathBuf,
}

pub fn watch(tick: Sender<()>) -> Result<Watch> {
    let conn = bus::session()?;
    let service = conn.unique_name().ok_or_else(|| Error::msg("D-Bus: no unique name"))?.to_string();
    let id = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = format!("/dev/knobdeck/KWinWatch{id}");
    conn.object_server().at(path.as_str(), WatchObject(id)).map_err(|e| Error::msg(format!("Could not register with D-Bus: {e}")))?;
    WATCHERS.lock().unwrap().get_or_insert_with(HashMap::new).insert(id, tick);
    let plugin = format!("knobdeck-watch-{}-{id}", std::process::id());
    let file = script_dir().join(format!("{plugin}.js"));
    let w = Watch { id, plugin, path, file };
    let js = format!("const SERVICE = {}; const PATH = {}; const IFACE = {};\n{WATCH_JS}", json!(service), json!(w.path), json!(WATCH_IFACE));
    std::fs::write(&w.file, js)?;
    load(&conn, &w.file, &w.plugin)?;
    Ok(w)
}

impl Drop for Watch {
    fn drop(&mut self) {
        if let Ok(conn) = bus::session() {
            unload(&conn, &self.plugin);
            let _ = conn.object_server().remove::<WatchObject, _>(self.path.as_str());
        }
        let _ = std::fs::remove_file(&self.file);
        if let Some(w) = WATCHERS.lock().unwrap().as_mut() {
            w.remove(&self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_quotes_its_inputs() {
        let s = script("move", &json!({"id": "{a\"b}"}), "1-2", ":1.42");
        assert!(s.starts_with("const CMD = \"move\"; const ARGS = {\"id\":\"{a\\\"b}\"}; const TOKEN = \"1-2\";"));
        assert!(s.contains("callDBus(\":1.42\", \"/dev/knobdeck/KWinBridge\", \"dev.knobdeck.KWinBridge\", \"Reply\""));
        assert!(s.contains("function run(cmd, a)"));
    }
}
