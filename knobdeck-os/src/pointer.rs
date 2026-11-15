//! The pointer: where it is, and the UI element under it.
//!
//! - Position: `GetCursorPos` (Windows, physical pixels), the current `CGEvent`'s location (macOS,
//!   points), and on Linux the compositor (a KWin script on KDE Plasma, the KnobDeck GNOME Shell
//!   extension, the X server on X11), in its logical pixels: the same space as
//!   [`crate::window::Rect`].
//! - Element: UI Automation `ElementFromPoint` (Windows), `AXUIElementCopyElementAtPosition`
//!   (macOS). Linux has no app-independent hit test, so there it is an error.

use crate::Result;

/// A UI element under the pointer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Element {
    /// The OS's role name: a UIA control type (`button`, `edit`) or an AX role (`AXButton`).
    pub role: String,
    /// Its name or title, as screen readers would say it (may be empty).
    pub name: String,
    /// The owning process.
    pub pid: u32,
}

/// The pointer's position in global coordinates.
pub fn position() -> Result<(f64, f64)> {
    sys::position()
}

/// The element under the pointer, `Ok(None)` over nothing (the desktop) or KnobDeck's own windows.
pub fn element_at() -> Result<Option<Element>> {
    let el = sys::element_at()?;
    Ok(el.filter(|e| !crate::window::skipped(e.pid)))
}

#[cfg(windows)]
use crate::win_pointer as sys;

#[cfg(target_os = "macos")]
use crate::macos::pointer as sys;

#[cfg(target_os = "linux")]
use crate::linux::pointer as sys;

#[cfg(test)]
mod tests {
    /// Prints the pointer position and the element under it; changes nothing.
    /// `cargo test -p knobdeck-os --features pointer live_pointer -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_pointer() {
        eprintln!("position: {:?}", super::position());
        eprintln!("element: {:?}", super::element_at());
    }
}
