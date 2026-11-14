//! Hyprland: its command socket (`.socket.sock`) and event socket (`.socket2.sock`).

use crate::window::{Rect, Window, WindowId, WindowState};
use crate::{Error, Result};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

fn socket(name: &str) -> Option<PathBuf> {
    let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
    [runtime.join("hypr").join(&sig).join(name), PathBuf::from("/tmp/hypr").join(&sig).join(name)].into_iter().find(|p| p.exists())
}

pub fn available() -> bool {
    socket(".socket.sock").is_some()
}

/// One request on the command socket (`hyprctl` without the program).
pub fn request(req: &str) -> Option<String> {
    let mut s = UnixStream::connect(socket(".socket.sock")?).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(500))).ok()?;
    s.write_all(req.as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    Some(out)
}

pub fn json(req: &str) -> Option<Value> {
    serde_json::from_str(&request(&format!("j/{req}"))?).ok()
}

fn window(v: &Value) -> Option<Window> {
    let address = v["address"].as_str().filter(|a| !a.is_empty())?;
    let s = |k: &str| v[k].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    let pair = |k: &str| (v[k][0].as_f64().unwrap_or(0.0), v[k][1].as_f64().unwrap_or(0.0));
    let ((x, y), (w, h)) = (pair("at"), pair("size"));
    // Current Hyprland: 0 none, 1 maximized, 2 full screen (3 both). Older: a bool plus `fullscreenMode` (1 = maximized).
    let (maximized, fullscreen) = match &v["fullscreen"] {
        Value::Bool(on) => {
            let max_mode = v["fullscreenMode"].as_i64() == Some(1);
            (*on && max_mode, *on && !max_mode)
        }
        n => {
            let n = n.as_i64().unwrap_or(0);
            (n == 1, n >= 2)
        }
    };
    Some(Window {
        id: WindowId(super::window::RawId::Hypr(address.to_owned())),
        pid: v["pid"].as_i64().filter(|p| *p > 0).unwrap_or(0) as u32,
        app_id: s("class").or_else(|| s("initialClass")).unwrap_or_default(),
        app_name: None,
        title: s("title"),
        bounds: Rect { x, y, w, h },
        monitor: v["monitor"].as_i64().filter(|m| *m >= 0).map(|m| m as usize),
        state: WindowState { maximized, minimized: v["hidden"].as_bool() == Some(true), fullscreen, topmost: v["pinned"].as_bool() == Some(true) },
        desktop_id: None,
    })
}

pub fn foreground() -> Option<Window> {
    window(&json("activewindow")?)
}

/// Mapped windows, most recently focused first.
pub fn list() -> Result<Vec<Window>> {
    let clients = json("clients").ok_or_else(|| Error::msg("Hyprland did not answer"))?;
    let mut clients: Vec<&Value> = clients.as_array().into_iter().flatten().filter(|c| c["mapped"].as_bool() != Some(false)).collect();
    clients.sort_by_key(|c| c["focusHistoryID"].as_i64().unwrap_or(i64::MAX));
    Ok(clients.into_iter().filter_map(window).collect())
}

pub fn focus(address: &str) -> Result {
    let reply = request(&format!("dispatch focuswindow address:{address}")).unwrap_or_default();
    if reply.trim() == "ok" {
        Ok(())
    } else {
        Err(Error::msg(format!("Hyprland could not focus the window: {}", reply.trim())))
    }
}

/// Reads the event socket on a thread; dropping it shuts the socket, which ends the thread.
pub struct Watch(UnixStream);

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }
}

/// Events that can change the target window or its title / state.
fn relevant(line: &str) -> bool {
    let name = line.split(">>").next().unwrap_or("");
    matches!(name, "activewindow" | "activewindowv2" | "windowtitle" | "windowtitlev2" | "fullscreen" | "openwindow" | "closewindow" | "movewindow" | "movewindowv2" | "changefloatingmode" | "pin" | "minimized")
}

pub fn watch(tick: Sender<()>) -> Result<Watch> {
    let path = socket(".socket2.sock").ok_or_else(|| Error::msg("Hyprland's event socket is missing"))?;
    let stream = UnixStream::connect(path)?;
    let reader = BufReader::new(stream.try_clone()?);
    std::thread::Builder::new()
        .name("knobdeck-os-hypr-watch".into())
        .spawn(move || {
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if relevant(&line) && tick.send(()).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| Error::msg(format!("Could not start the window watcher: {e}")))?;
    Ok(Watch(stream))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clients_of_both_formats() {
        let w = window(&serde_json::json!({"address": "0x1", "at": [10, 20], "size": [300, 200], "class": "kitty", "title": "t", "pid": 42, "monitor": 1, "fullscreen": 1, "pinned": true})).unwrap();
        assert_eq!(w.bounds, Rect { x: 10.0, y: 20.0, w: 300.0, h: 200.0 });
        assert!(w.state.maximized && !w.state.fullscreen && w.state.topmost);
        assert_eq!((w.pid, w.app_id.as_str(), w.monitor), (42, "kitty", Some(1)));
        let old = window(&serde_json::json!({"address": "0x2", "fullscreen": true, "fullscreenMode": 0})).unwrap();
        assert!(old.state.fullscreen && !old.state.maximized);
        assert!(window(&serde_json::json!({})).is_none());
    }

    #[test]
    fn filters_events() {
        assert!(relevant("activewindowv2>>55d1a2"));
        assert!(relevant("windowtitle>>55d1a2"));
        assert!(!relevant("workspace>>2"));
    }
}
