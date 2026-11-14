//! Linux window back end: Hyprland, X11, or the KDE / GNOME bridge, picked once per process.

use super::{bridge, desktop, gnome, hypr, is_wayland, kwin, x11, Desktop};
use crate::window::{skipped, Rect, Window, WindowId, WindowState};
use crate::{Error, Result};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::mpsc::Sender;
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RawId {
    Hypr(String),
    X11(u32),
    /// A KWin `internalId` or a Mutter window id.
    Bridge(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Backend {
    Hypr,
    X11,
    Kde,
    Gnome,
}

fn backend() -> Option<Backend> {
    static B: OnceLock<Option<Backend>> = OnceLock::new();
    *B.get_or_init(|| {
        if hypr::available() {
            Some(Backend::Hypr)
        } else if !is_wayland() {
            Some(Backend::X11)
        } else {
            match desktop() {
                Desktop::Kde => Some(Backend::Kde),
                Desktop::Gnome => Some(Backend::Gnome),
                Desktop::Other => None,
            }
        }
    })
}

fn unsupported() -> Error {
    Error::msg("Window actions need GNOME, KDE Plasma, Hyprland or an X11 session")
}

/// A window as `kwin.js` and the GNOME extension describe it.
pub(crate) fn from_bridge(v: &Value) -> Option<Window> {
    let id = v["id"].as_str().filter(|s| !s.is_empty())?;
    let s = |k: &str| v[k].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    let b = |k: &str| v[k].as_bool().unwrap_or(false);
    let f = &v["frame"];
    let n = |k: &str| f[k].as_f64().unwrap_or(0.0);
    Some(Window {
        id: WindowId(RawId::Bridge(id.to_owned())),
        pid: v["pid"].as_u64().unwrap_or(0) as u32,
        app_id: s("app").unwrap_or_default(),
        app_name: None,
        title: s("title"),
        bounds: Rect { x: n("x"), y: n("y"), w: n("w"), h: n("h") },
        monitor: v["output"].as_i64().filter(|i| *i >= 0).map(|i| i as usize),
        state: WindowState { maximized: b("maximized"), minimized: b("minimized"), fullscreen: b("fullscreen"), topmost: b("above") },
        desktop_id: s("desktopFile").map(|d| d.trim_end_matches(".desktop").to_owned()),
    })
}

pub fn foreground() -> Option<Window> {
    match backend()? {
        Backend::Hypr => hypr::foreground(),
        Backend::X11 => x11::foreground(),
        Backend::Kde | Backend::Gnome => from_bridge(&bridge("active", &json!({})).ok()?),
    }
}

pub fn list() -> Result<Vec<Window>> {
    match backend().ok_or_else(unsupported)? {
        Backend::Hypr => hypr::list(),
        Backend::X11 => x11::list(),
        Backend::Kde | Backend::Gnome => Ok(bridge("list", &json!({}))?.as_array().into_iter().flatten().filter_map(from_bridge).collect()),
    }
}

pub fn focus(id: &RawId) -> Result {
    match id {
        RawId::Hypr(addr) => hypr::focus(addr),
        RawId::X11(win) => x11::focus(*win),
        RawId::Bridge(id) => bridge("activate", &json!({ "id": id })).map(|_| ()),
    }
}

/// Lower-cased names `app`'s windows may carry as class / app id / desktop entry id.
fn candidates(app: &str, hint: Option<&str>) -> Vec<String> {
    let mut c = if app.ends_with(".desktop") {
        super::desktop::DesktopEntry::parse(Path::new(app)).map(|e| e.class_candidates()).unwrap_or_default()
    } else {
        Vec::new()
    };
    if let Some(stem) = Path::new(app).file_name().map(|n| n.to_string_lossy().trim_end_matches(".desktop").to_ascii_lowercase()) {
        c.push(stem);
    }
    c.extend(hint.map(str::to_ascii_lowercase));
    c.retain(|s| !s.is_empty());
    c
}

/// `org.gnome.Nautilus` matches `nautilus`; nothing matches an empty name.
fn matches(name: &str, candidates: &[String]) -> bool {
    let name = name.to_ascii_lowercase();
    let last = |s: &str| s.rsplit('.').next().unwrap_or(s).to_owned();
    !name.is_empty() && candidates.iter().any(|c| name == *c || last(&name) == *c || last(&name) == last(c))
}

pub fn focus_app(app: &str, hint: Option<&str>) -> Result<bool> {
    let cands = candidates(app, hint);
    if cands.is_empty() {
        return Ok(false);
    }
    let hit = list()?.into_iter().filter(|w| !skipped(w.pid)).find(|w| matches(&w.app_id, &cands) || w.desktop_id.as_deref().is_some_and(|d| matches(d, &cands)));
    match hit {
        Some(w) => focus(&w.id.0).map(|_| true),
        None => Ok(false),
    }
}

pub enum Source {
    Hypr(#[allow(dead_code)] hypr::Watch),
    X11(#[allow(dead_code)] x11::Watch),
    Kde(#[allow(dead_code)] kwin::Watch),
    Gnome(#[allow(dead_code)] gnome::Watch),
}

pub fn source(tick: Sender<()>) -> Result<Source> {
    Ok(match backend().ok_or_else(unsupported)? {
        Backend::Hypr => Source::Hypr(hypr::watch(tick)?),
        Backend::X11 => Source::X11(x11::watch(tick)?),
        Backend::Kde => Source::Kde(kwin::watch(tick)?),
        Backend::Gnome => Source::Gnome(gnome::watch(tick)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bridge_windows() {
        let w = from_bridge(&json!({"id": "{a}", "title": "T", "app": "org.mozilla.firefox", "desktopFile": "firefox.desktop", "pid": 42, "frame": {"x": 1, "y": 2, "w": 3, "h": 4}, "output": 1, "maximized": true, "above": true})).unwrap();
        assert_eq!(w.desktop_id.as_deref(), Some("firefox"));
        assert_eq!(w.bounds, Rect { x: 1.0, y: 2.0, w: 3.0, h: 4.0 });
        assert_eq!(w.monitor, Some(1));
        assert!(w.state.maximized && w.state.topmost && !w.state.minimized);
        assert!(from_bridge(&Value::Null).is_none());
    }

    #[test]
    fn app_matching() {
        let c = candidates("/usr/bin/firefox", Some("Firefox"));
        assert!(matches("org.mozilla.firefox", &c));
        assert!(matches("Firefox", &c));
        assert!(!matches("fire", &c));
        assert!(!matches("", &c));
        assert!(matches("org.kde.dolphin", &candidates("org.kde.dolphin", None)));
    }
}
