//! Linux pointer position from the compositor: KWin's `workspace.cursorPos` (a one-shot script),
//! the KnobDeck GNOME Shell extension's `global.get_pointer()`, or the X server's root pointer.
//! Wayland clients can't read the global pointer themselves.

use super::{bus, desktop, Desktop};
use crate::pointer::Element;
use crate::{Error, Result};
use serde_json::{json, Value};

fn xy(v: &Value) -> Option<(f64, f64)> {
    Some((v["x"].as_f64()?, v["y"].as_f64()?))
}

pub fn position() -> Result<(f64, f64)> {
    let reply = match desktop() {
        Desktop::Kde if super::is_wayland() => super::kwin::call("cursor", &json!({}))?,
        Desktop::Gnome if bus::session_has_owner("dev.knobdeck.Shell") => super::gnome::call("pointer", &json!({}))?,
        _ if std::env::var_os("DISPLAY").is_some() => return x11_pointer(),
        _ => return Err(Error::msg("Reading the pointer needs GNOME, KDE Plasma or X11")),
    };
    xy(&reply).ok_or_else(|| Error::msg("The desktop didn't report the pointer"))
}

fn x11_pointer() -> Result<(f64, f64)> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::ConnectionExt;
    let (c, screen) = x11rb::connect(None).map_err(|e| Error::msg(format!("X11: {e}")))?;
    let root = c.setup().roots[screen].root;
    let p = c.query_pointer(root).map_err(|e| Error::msg(format!("X11: {e}")))?.reply().map_err(|e| Error::msg(format!("X11: {e}")))?;
    Ok((p.root_x as f64, p.root_y as f64))
}

pub fn element_at() -> Result<Option<Element>> {
    Err(Error::msg("Finding the element under the pointer isn't supported on Linux"))
}
