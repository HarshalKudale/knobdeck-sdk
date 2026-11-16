//! AT-SPI: the focused element and its selected text, over the accessibility bus.
//!
//! The target window's app is found among the registry's apps by process id; its focused
//! object comes from the `Collection` interface (one call answered by the app), or, for toolkits
//! without it, a bounded walk of the app's showing objects. GTK, Qt, Firefox and LibreOffice
//! register when the desktop's accessibility bus is up. Apps that don't (most Electron apps
//! unless a screen reader runs) fall back to the PRIMARY selection for the selected text.
//!
//! Qt apps (Dolphin, Kate, Konsole) publish their tree only while accessibility is switched on
//! for the session (`org.a11y.Status.IsEnabled`, off by default on Plasma). [`ensure_enabled`]
//! switches it on, as a screen reader does, and waits for the target app to register.
//!
//! [`dolphin_selection`] reads Dolphin's selected files: each view is a list whose description
//! ends with its folder's `file://` URL, and its selected items are the selected files.

use super::bus;
use crate::focus::Element;
use crate::{Error, Result};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use zbus::blocking::Connection;
use zbus::zvariant::OwnedObjectPath;

const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
#[cfg_attr(not(feature = "selection"), allow(dead_code))]
const TEXT: &str = "org.a11y.atspi.Text";
const STATE_EDITABLE: u32 = 7;
const STATE_FOCUSED: u32 = 12;
#[cfg_attr(not(feature = "files"), allow(dead_code))]
const STATE_SELECTED: u32 = 23;
const STATE_SHOWING: u32 = 25;
const ROLE_PASSWORD_TEXT: u32 = 40;
/// Objects visited by the fallback walk, and its time budget.
const WALK_LIMIT: usize = 4000;
const WALK_BUDGET: Duration = Duration::from_millis(250);

type Obj = (String, OwnedObjectPath);

static CONN: Mutex<Option<Connection>> = Mutex::new(None);

/// The accessibility bus (its address comes from `org.a11y.Bus` on the session bus).
fn a11y() -> Result<Connection> {
    if let Some(c) = CONN.lock().unwrap().clone() {
        return Ok(c);
    }
    let session = bus::session()?;
    let addr: String = bus::call(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus", "GetAddress", &())
        .map_err(|_| Error::msg("The desktop's accessibility bus isn't running"))?;
    let conn = zbus::blocking::connection::Builder::address(addr.as_str())
        .and_then(|b| b.method_timeout(Duration::from_millis(800)).build())
        .map_err(bus::err)?;
    *CONN.lock().unwrap() = Some(conn.clone());
    Ok(conn)
}

fn call<A, R>(c: &Connection, o: &Obj, iface: &str, method: &str, args: &A) -> Result<R>
where
    A: serde::Serialize + zbus::zvariant::DynamicType,
    R: for<'d> serde::Deserialize<'d> + zbus::zvariant::Type,
{
    bus::call(c, &o.0, o.1.as_str(), iface, method, args)
}

fn states(c: &Connection, o: &Obj) -> Result<u64> {
    let s: Vec<u32> = call(c, o, ACCESSIBLE, "GetState", &())?;
    Ok(s.first().copied().unwrap_or(0) as u64 | (s.get(1).copied().unwrap_or(0) as u64) << 32)
}

fn has(states: u64, bit: u32) -> bool {
    states & (1 << bit) != 0
}

/// Switch the session's accessibility on when it is off (Qt apps then publish their trees).
/// Returns whether it was off.
fn ensure_enabled() -> bool {
    let Ok(session) = bus::session() else { return false };
    let on: bool = bus::get(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Status", "IsEnabled").unwrap_or(true);
    if on {
        return false;
    }
    bus::set(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Status", "IsEnabled", true).is_ok()
}

/// [`app_of`], switching accessibility on first when the app isn't there; an app needs a moment
/// to register after that.
fn app_of_enabling(c: &Connection, pid: u32) -> Result<Obj> {
    if let Ok(app) = app_of(c, pid) {
        return Ok(app);
    }
    if !ensure_enabled() {
        return app_of(c, pid);
    }
    let deadline = Instant::now() + Duration::from_millis(2000);
    loop {
        std::thread::sleep(Duration::from_millis(100));
        match app_of(c, pid) {
            Ok(app) => return Ok(app),
            Err(e) if Instant::now() > deadline => return Err(e),
            Err(_) => {}
        }
    }
}

/// The app on the bus whose process is `pid`.
fn app_of(c: &Connection, pid: u32) -> Result<Obj> {
    let root: Obj = ("org.a11y.atspi.Registry".into(), OwnedObjectPath::try_from("/org/a11y/atspi/accessible/root").unwrap());
    let apps: Vec<Obj> = call(c, &root, ACCESSIBLE, "GetChildren", &())?;
    for app in apps {
        let p: Result<u32> = bus::call(c, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetConnectionUnixProcessID", &(app.0.as_str(),));
        if p.ok() == Some(pid) {
            return Ok(app);
        }
    }
    Err(crate::not_exposed())
}

/// The app's focused object: `Collection.GetMatches` for STATE_FOCUSED, else a bounded walk.
fn focused(c: &Connection, app: &Obj) -> Option<Obj> {
    // MatchRule: states, state match, attributes, attr match, roles, role match, interfaces,
    // interface match, invert. MATCH_ALL = 1, MATCH_INVALID (ignored) = 0.
    let rule = (vec![1i32 << STATE_FOCUSED, 0i32], 1i32, HashMap::<String, String>::new(), 0i32, Vec::<i32>::new(), 0i32, Vec::<String>::new(), 0i32, false);
    let hits: Result<Vec<Obj>> = call(c, app, "org.a11y.atspi.Collection", "GetMatches", &(rule, 0u32, 1i32, true));
    if let Ok(Some(hit)) = hits.map(|h| h.into_iter().next()) {
        return Some(hit);
    }
    let deadline = Instant::now() + WALK_BUDGET;
    let mut stack: Vec<Obj> = call(c, app, ACCESSIBLE, "GetChildren", &()).unwrap_or_default();
    let mut seen = 0;
    while let Some(o) = stack.pop() {
        seen += 1;
        if seen > WALK_LIMIT || Instant::now() > deadline {
            return None;
        }
        let Ok(s) = states(c, &o) else { continue };
        if has(s, STATE_FOCUSED) {
            return Some(o);
        }
        if has(s, STATE_SHOWING) {
            stack.extend(call::<_, Vec<Obj>>(c, &o, ACCESSIBLE, "GetChildren", &()).unwrap_or_default());
        }
    }
    None
}

fn target_pid() -> Result<u32> {
    crate::window::target().map(|w| w.pid).filter(|p| *p != 0).ok_or_else(|| Error::msg("No window to read from"))
}

fn describe(c: &Connection, o: &Obj, pid: u32) -> Result<Element> {
    let s = states(c, o)?;
    let role: u32 = call(c, o, ACCESSIBLE, "GetRole", &()).unwrap_or(0);
    let name: String = call(c, o, ACCESSIBLE, "GetRoleName", &()).unwrap_or_default();
    Ok(Element { password: role == ROLE_PASSWORD_TEXT || name == "password text", editable: has(s, STATE_EDITABLE), role: name, pid })
}

pub fn element() -> Result<Option<Element>> {
    let pid = target_pid()?;
    let c = a11y()?;
    let app = app_of_enabling(&c, pid)?;
    match focused(&c, &app) {
        Some(o) => describe(&c, &o, pid).map(Some),
        None => Ok(None),
    }
}

#[cfg_attr(not(feature = "selection"), allow(dead_code))]
/// The focused object's selected text. `Err` only when the app isn't on the accessibility bus
/// or nothing in it has focus; those fall back to PRIMARY.
fn atspi_text() -> Result<Option<String>> {
    let pid = target_pid()?;
    let c = a11y()?;
    let app = app_of_enabling(&c, pid)?;
    let o = focused(&c, &app).ok_or_else(crate::not_exposed)?;
    let n: i32 = match call(&c, &o, TEXT, "GetNSelections", &()) {
        Ok(n) => n,
        Err(_) => return Ok(None), // no Text interface: a button, a list…
    };
    let mut out = String::new();
    for i in 0..n.max(0) {
        let (start, end): (i32, i32) = call(&c, &o, TEXT, "GetSelection", &(i,))?;
        if end > start {
            let t: String = call(&c, &o, TEXT, "GetText", &(start, end))?;
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&t);
            if out.len() > crate::MAX_TEXT {
                break;
            }
        }
    }
    Ok(Some(out).filter(|s| !s.is_empty()))
}

#[cfg(feature = "selection")]
pub fn selected_text() -> Result<Option<String>> {
    match atspi_text() {
        Ok(t) => Ok(t),
        // Not on the bus: the PRIMARY selection holds the last text selected anywhere.
        Err(e) => super::clipboard::primary_text().map_err(|_| e),
    }
}

/// An object's property through a plain `Properties.Get` (Qt's bridge answers that, but not the
/// property proxy's `GetAll`).
#[cfg_attr(not(feature = "files"), allow(dead_code))]
fn prop<T: TryFrom<zbus::zvariant::OwnedValue>>(c: &Connection, o: &Obj, iface: &str, name: &str) -> Option<T> {
    let v: zbus::zvariant::OwnedValue = call(c, o, "org.freedesktop.DBus.Properties", "Get", &(iface, name)).ok()?;
    T::try_from(v).ok()
}

/// A Dolphin view: its folder and the items selected in it.
#[cfg(feature = "files")]
#[derive(Debug, Default)]
pub(crate) struct DolphinView {
    pub folder: Option<std::path::PathBuf>,
    pub selected: Vec<std::path::PathBuf>,
    pub focused: bool,
}

/// `… in location file:///home/me/My%20Pics` → `/home/me/My Pics` (the description is translated;
/// the URL is its last word).
#[cfg_attr(not(feature = "files"), allow(dead_code))]
fn folder_of_description(desc: &str) -> Option<std::path::PathBuf> {
    let at = desc.rfind("file://")?;
    let rest = desc[at + "file://".len()..].trim();
    let path = if rest.starts_with('/') { rest } else { &rest[rest.find('/')?..] };
    let p = crate::percent_decode(path);
    let p = if p.len() > 1 { p.trim_end_matches('/').to_string() } else { p };
    Some(std::path::PathBuf::from(p))
}

/// Dolphin's views (one, or two in split view) in the process `pid`, with their selections.
#[cfg(feature = "files")]
pub(crate) fn dolphin_views(pid: u32) -> Result<Vec<DolphinView>> {
    let c = a11y()?;
    let app = app_of_enabling(&c, pid).map_err(|_| Error::msg("Dolphin isn't on the accessibility bus"))?;
    let deadline = Instant::now() + Duration::from_millis(600);
    let mut views: Vec<(bool, DolphinView)> = vec![];
    let mut title = String::new();
    let mut stack: Vec<Obj> = call(&c, &app, ACCESSIBLE, "GetChildren", &()).unwrap_or_default();
    let mut seen = 0;
    while let Some(o) = stack.pop() {
        seen += 1;
        if seen > WALK_LIMIT || Instant::now() > deadline {
            break;
        }
        let role: String = call(&c, &o, ACCESSIBLE, "GetRoleName", &()).unwrap_or_default();
        let s = states(&c, &o).unwrap_or(0);
        if role == "frame" && title.is_empty() {
            title = prop(&c, &o, ACCESSIBLE, "Name").unwrap_or_default();
        }
        if role == "list" {
            let desc: String = prop(&c, &o, ACCESSIBLE, "Description").unwrap_or_default();
            if let Some(folder) = folder_of_description(&desc) {
                views.push((has(s, STATE_SHOWING), DolphinView { selected: selected_items(&c, &o, &folder), folder: Some(folder), focused: has(s, STATE_FOCUSED) }));
            }
        }
        // Qt doesn't mark every container as showing, so walk all of them; but not into item
        // lists and tables (a folder's items, the Places panel), which can be long.
        if !matches!(role.as_str(), "list" | "table" | "tree" | "tree table" | "list box" | "menu" | "menu bar") {
            stack.extend(call::<_, Vec<Obj>>(&c, &o, ACCESSIBLE, "GetChildren", &()).unwrap_or_default());
        }
    }
    // Hidden views (a closed split, another tab) only when nothing is showing.
    let any_showing = views.iter().any(|(showing, _)| *showing);
    let mut views: Vec<DolphinView> = views.into_iter().filter(|(showing, _)| *showing || !any_showing).map(|(_, v)| v).collect();
    // The window title names the active view's folder (its full path when Dolphin shows it).
    let shown = title.rsplit_once(" — ").map_or(title.as_str(), |(t, _)| t).trim();
    if !views.iter().any(|v| v.focused) {
        let full = views.iter().position(|v| v.folder.as_deref().is_some_and(|f| f.as_os_str() == shown));
        let named = || views.iter().position(|v| v.folder.as_deref().and_then(|f| f.file_name()).is_some_and(|n| n == shown));
        if let Some(i) = full.or_else(named) {
            views[i].focused = true;
        }
    }
    if views.is_empty() {
        return Err(Error::msg("Couldn't find Dolphin's folder view"));
    }
    Ok(views)
}

/// A view's selected items as paths in `folder`: through the `Selection` interface, else each
/// item's SELECTED state.
#[cfg(feature = "files")]
fn selected_items(c: &Connection, list: &Obj, folder: &std::path::Path) -> Vec<std::path::PathBuf> {
    const SELECTION: &str = "org.a11y.atspi.Selection";
    let mut items: Vec<Obj> = vec![];
    match prop::<i32>(c, list, SELECTION, "NSelectedChildren") {
        Some(n) if n >= 0 => {
            for i in 0..n.min(crate::files::MAX_FILES as i32) {
                if let Ok(o) = call::<_, Obj>(c, list, SELECTION, "GetSelectedChild", &(i,)) {
                    items.push(o);
                }
            }
        }
        _ => {
            for o in call::<_, Vec<Obj>>(c, list, ACCESSIBLE, "GetChildren", &()).unwrap_or_default() {
                if states(c, &o).is_ok_and(|s| has(s, STATE_SELECTED)) {
                    items.push(o);
                }
            }
        }
    }
    items
        .iter()
        .filter_map(|o| prop::<String>(c, o, ACCESSIBLE, "Name"))
        .filter(|n| !n.is_empty() && !n.contains('/'))
        .map(|n| folder.join(n))
        .filter(|p| p.symlink_metadata().is_ok())
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn folder_from_dolphin_description() {
        assert_eq!(super::folder_of_description("in a grid layout in location file:///home/me/My%20Pics"), Some("/home/me/My Pics".into()));
        assert_eq!(super::folder_of_description("im Rasterlayout am Ort file:///home/me/"), Some("/home/me".into()));
        assert_eq!(super::folder_of_description("in location file:///"), Some("/".into()));
        assert_eq!(super::folder_of_description("Glob Pattern"), None);
    }

    /// Prints the focused element and selection of the frontmost window.
    #[test]
    #[ignore]
    fn live_focus() {
        eprintln!("element: {:?}", super::element());
        eprintln!("atspi text: {:?}", super::atspi_text());
        // Every app on the bus, without touching focus: its name, pid and focused object.
        let c = super::a11y().unwrap();
        let root: super::Obj = ("org.a11y.atspi.Registry".into(), zbus::zvariant::OwnedObjectPath::try_from("/org/a11y/atspi/accessible/root").unwrap());
        for app in super::call::<_, Vec<super::Obj>>(&c, &root, super::ACCESSIBLE, "GetChildren", &()).unwrap() {
            let name: String = super::bus::get(&c, &app.0, app.1.as_str(), super::ACCESSIBLE, "Name").unwrap_or_default();
            let f = super::focused(&c, &app);
            eprintln!("app {name:?} {}: focused {:?}", app.0, f.as_ref().map(|o| super::describe(&c, o, 0)));
        }
    }
}
