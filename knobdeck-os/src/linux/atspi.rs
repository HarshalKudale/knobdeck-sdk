//! AT-SPI: the focused element and its selected text, over the accessibility bus.
//!
//! The target window's app is found among the registry's apps by process id; its focused
//! object comes from the `Collection` interface (one call answered by the app), or, for toolkits
//! without it, a bounded walk of the app's showing objects. GTK, Qt, Firefox and LibreOffice
//! register when the desktop's accessibility bus is up. Apps that don't (most Electron apps
//! unless a screen reader runs) fall back to the PRIMARY selection for the selected text.

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
    let app = app_of(&c, pid)?;
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
    let app = app_of(&c, pid)?;
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

#[cfg(test)]
mod tests {
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
