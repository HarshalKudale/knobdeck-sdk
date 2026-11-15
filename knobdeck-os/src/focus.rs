//! The focused UI element of the target app: what kind of control has the keyboard focus, whether
//! it takes text, and whether it is a password field. Read through UI Automation (Windows),
//! Accessibility (macOS) and AT-SPI (Linux), only when called.

use crate::Result;

/// The control that has the keyboard focus in the target app.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Element {
    /// The OS's role name: a UIA control type (`edit`, `document`), an AX role (`AXTextField`) or
    /// an AT-SPI role (`text`, `entry`, `terminal`).
    pub role: String,
    /// It takes typed text.
    pub editable: bool,
    /// A password field. [`crate::selection::text`] never reads one.
    pub password: bool,
    /// The owning process.
    pub pid: u32,
}

/// The focused element of the target app (the frontmost one that isn't KnobDeck's). `Ok(None)`
/// when nothing in it has focus; an error when the OS or the app won't say (Accessibility not
/// granted, the app exposes no accessibility tree).
pub fn element() -> Result<Option<Element>> {
    sys::element()
}

#[cfg(windows)]
use crate::win_focus as sys;

#[cfg(target_os = "macos")]
use crate::macos::focus as sys;

#[cfg(target_os = "linux")]
use crate::linux::atspi as sys;
