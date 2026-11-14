//! X11 sessions: EWMH (`_NET_ACTIVE_WINDOW`, `_NET_CLIENT_LIST_STACKING`, `_NET_WM_STATE`).

use crate::window::{Rect, Window, WindowId, WindowState};
use crate::{Error, Result};
use std::sync::mpsc::Sender;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConnectionExt, CreateWindowAux, EventMask, WindowClass};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

type XWindow = u32;

struct Atoms {
    active: u32,
    stacking: u32,
    wm_name: u32,
    utf8: u32,
    pid: u32,
    state: u32,
    max_v: u32,
    max_h: u32,
    hidden: u32,
    fullscreen: u32,
    above: u32,
}

fn atoms(conn: &impl Connection) -> Option<Atoms> {
    let get = |n: &str| conn.intern_atom(false, n.as_bytes()).ok()?.reply().ok().map(|r| r.atom);
    Some(Atoms {
        active: get("_NET_ACTIVE_WINDOW")?,
        stacking: get("_NET_CLIENT_LIST_STACKING")?,
        wm_name: get("_NET_WM_NAME")?,
        utf8: get("UTF8_STRING")?,
        pid: get("_NET_WM_PID")?,
        state: get("_NET_WM_STATE")?,
        max_v: get("_NET_WM_STATE_MAXIMIZED_VERT")?,
        max_h: get("_NET_WM_STATE_MAXIMIZED_HORZ")?,
        hidden: get("_NET_WM_STATE_HIDDEN")?,
        fullscreen: get("_NET_WM_STATE_FULLSCREEN")?,
        above: get("_NET_WM_STATE_ABOVE")?,
    })
}

fn connect() -> Option<(RustConnection, XWindow, Atoms)> {
    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots.get(screen)?.root;
    let a = atoms(&conn)?;
    Some((conn, root, a))
}

fn prop32(conn: &impl Connection, win: XWindow, atom: u32, ty: impl Into<u32>) -> Vec<u32> {
    conn.get_property(false, win, atom, ty, 0, 4096).ok().and_then(|c| c.reply().ok()).and_then(|r| r.value32().map(|v| v.collect())).unwrap_or_default()
}

fn class_of(conn: &impl Connection, win: XWindow) -> Option<String> {
    let r = conn.get_property(false, win, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 1024).ok()?.reply().ok()?;
    // "instance\0Class\0"
    let parts: Vec<&[u8]> = r.value.split(|b| *b == 0).filter(|p| !p.is_empty()).collect();
    parts.last().map(|c| String::from_utf8_lossy(c).into_owned())
}

fn title_of(conn: &impl Connection, a: &Atoms, win: XWindow) -> Option<String> {
    let r = conn.get_property(false, win, a.wm_name, a.utf8, 0, 4096).ok()?.reply().ok()?;
    let r = if r.value.is_empty() { conn.get_property(false, win, AtomEnum::WM_NAME, AtomEnum::STRING, 0, 4096).ok()?.reply().ok()? } else { r };
    Some(String::from_utf8_lossy(&r.value).into_owned()).filter(|s| !s.is_empty())
}

fn describe(conn: &impl Connection, root: XWindow, a: &Atoms, win: XWindow) -> Window {
    let state = prop32(conn, win, a.state, AtomEnum::ATOM);
    let has = |atom: u32| state.contains(&atom);
    let bounds = conn
        .get_geometry(win)
        .ok()
        .and_then(|g| g.reply().ok())
        .and_then(|g| {
            let t = conn.translate_coordinates(win, root, 0, 0).ok()?.reply().ok()?;
            Some(Rect { x: t.dst_x as f64, y: t.dst_y as f64, w: g.width as f64, h: g.height as f64 })
        })
        .unwrap_or_default();
    Window {
        id: WindowId(super::window::RawId::X11(win)),
        pid: prop32(conn, win, a.pid, AtomEnum::CARDINAL).first().copied().unwrap_or(0),
        app_id: class_of(conn, win).unwrap_or_default(),
        app_name: None,
        title: title_of(conn, a, win),
        bounds,
        monitor: None,
        state: WindowState { maximized: has(a.max_v) && has(a.max_h), minimized: has(a.hidden), fullscreen: has(a.fullscreen), topmost: has(a.above) },
        desktop_id: None,
    }
}

fn active(conn: &impl Connection, root: XWindow, a: &Atoms) -> Option<XWindow> {
    prop32(conn, root, a.active, AtomEnum::WINDOW).first().copied().filter(|w| *w != 0)
}

pub fn foreground() -> Option<Window> {
    let (conn, root, a) = connect()?;
    let win = active(&conn, root, &a)?;
    Some(describe(&conn, root, &a, win))
}

/// Managed windows, top of the stack first.
pub fn list() -> Result<Vec<Window>> {
    let (conn, root, a) = connect().ok_or_else(|| Error::msg("Could not connect to the X server"))?;
    let mut wins = prop32(&conn, root, a.stacking, AtomEnum::WINDOW);
    wins.reverse();
    Ok(wins.into_iter().map(|w| describe(&conn, root, &a, w)).collect())
}

pub fn focus(win: XWindow) -> Result {
    let (conn, root, a) = connect().ok_or_else(|| Error::msg("Could not connect to the X server"))?;
    // Source indication 2 = pager / tool request, which window managers honour over focus-stealing rules.
    let ev = ClientMessageEvent::new(32, win, a.active, [2u32, x11rb::CURRENT_TIME, 0, 0, 0]);
    let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
    conn.send_event(false, root, mask, ev).map_err(|e| Error::msg(format!("X11: {e}")))?;
    conn.flush().map_err(|e| Error::msg(format!("X11: {e}")))
}

/// A thread reading property events on its own connection. Dropping it sends that thread's
/// helper window a client message, which ends it.
pub struct Watch {
    helper: XWindow,
    stop: u32,
}

impl Drop for Watch {
    fn drop(&mut self) {
        if let Ok((conn, _)) = x11rb::connect(None) {
            let ev = ClientMessageEvent::new(32, self.helper, self.stop, [0u32; 5]);
            let _ = conn.send_event(false, self.helper, EventMask::NO_EVENT, ev);
            let _ = conn.flush();
        }
    }
}

pub fn watch(tick: Sender<()>) -> Result<Watch> {
    let x = |e: &dyn std::fmt::Display| Error::msg(format!("X11: {e}"));
    let (conn, root, a) = connect().ok_or_else(|| Error::msg("Could not connect to the X server"))?;
    let stop = conn.intern_atom(false, b"_KNOBDECK_WATCH_STOP").map_err(|e| x(&e))?.reply().map_err(|e| x(&e))?.atom;
    let helper = conn.generate_id().map_err(|e| x(&e))?;
    conn.create_window(0, helper, root, -1, -1, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new()).map_err(|e| x(&e))?;
    let props = ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE);
    conn.change_window_attributes(root, &props).map_err(|e| x(&e))?;
    conn.flush().map_err(|e| x(&e))?;
    std::thread::Builder::new()
        .name("knobdeck-os-x11-watch".into())
        .spawn(move || {
            let follow = |win: Option<XWindow>| {
                if let Some(w) = win {
                    let _ = conn.change_window_attributes(w, &ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE));
                    let _ = conn.flush();
                }
            };
            let mut cur = active(&conn, root, &a);
            follow(cur);
            let window_props = [a.wm_name, u32::from(AtomEnum::WM_NAME), a.state];
            while let Ok(ev) = conn.wait_for_event() {
                let changed = match ev {
                    Event::ClientMessage(m) if m.window == helper && m.type_ == stop => break,
                    Event::PropertyNotify(p) if p.window == root && p.atom == a.active => {
                        let next = active(&conn, root, &a);
                        if next != cur {
                            if let Some(old) = cur {
                                let _ = conn.change_window_attributes(old, &ChangeWindowAttributesAux::new().event_mask(EventMask::NO_EVENT));
                            }
                            cur = next;
                            follow(cur);
                        }
                        true
                    }
                    Event::PropertyNotify(p) => Some(p.window) == cur && window_props.contains(&p.atom),
                    _ => false,
                };
                if changed && tick.send(()).is_err() {
                    break;
                }
            }
        })
        .map_err(|e| Error::msg(format!("Could not start the window watcher: {e}")))?;
    Ok(Watch { helper, stop })
}
