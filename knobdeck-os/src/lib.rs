//! # knobdeck-os
//!
//! What KnobDeck and its plugins read from the OS, in one implementation for Windows, macOS and
//! Linux. The KnobDeck host uses it directly; plugins get it through `knobdeck_sdk::os` (SDK
//! features `os-window`, `os-apps`).
//!
//! - [`window`]: the foreground window, the *target* window (the frontmost one that isn't
//!   KnobDeck's), the window list, focusing a window or an app, and an event-driven watcher.
//! - [`apps`]: the installed apps (Start menu / `.app` bundles / XDG desktop entries).
//! - [`focus`]: the focused UI element of the target app (role, editable, password).
//! - [`selection`]: the selected text of the target app.
//! - [`clipboard`]: the clipboard's kinds, text, files and image (read only).
//! - [`placeholders`]: `{selection}`, `{clipboard}`, `{date}`… in action text: parse, escape, expand.
//!
//! Nothing here polls: [`window::watch`] runs on the OS's own events and stops when its handle
//! is dropped.
//!
//! On Linux the window functions work on Hyprland (its IPC socket), any X11 session (EWMH), KDE
//! Plasma (a KWin script) and GNOME (the KnobDeck Shell extension, see [`linux::gnome`]); other
//! Wayland desktops get a clear error.

mod error;
pub use error::{Error, Result};

#[cfg(feature = "window")]
pub mod window;

#[cfg(feature = "apps")]
pub mod apps;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(all(windows, feature = "window"))]
mod win;

#[cfg(feature = "focus")]
pub mod focus;

#[cfg(feature = "selection")]
pub mod selection;

#[cfg(feature = "clipboard")]
pub mod clipboard;

#[cfg(feature = "placeholders")]
pub mod placeholders;

#[cfg(all(windows, feature = "focus"))]
mod win_focus;

#[cfg(all(windows, feature = "clipboard"))]
mod win_clipboard;

/// Longest text returned by the selection and clipboard readers (1 MiB).
#[allow(dead_code)]
pub(crate) const MAX_TEXT: usize = 1 << 20;

/// The error for an app that doesn't expose its focus or selection.
#[allow(dead_code)]
pub(crate) fn not_exposed() -> Error {
    Error::msg("This app doesn't expose its focused element or selected text")
}

/// Cut text at [`MAX_TEXT`], on a character boundary.
#[allow(dead_code)]
pub(crate) fn cap_text(mut s: String) -> String {
    if s.len() > MAX_TEXT {
        let mut end = MAX_TEXT;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
    s
}

#[cfg(test)]
mod tests {
    #[test]
    fn caps_at_a_char_boundary() {
        let c = super::cap_text("é".repeat(super::MAX_TEXT)); // 2 bytes each
        assert!(c.len() <= super::MAX_TEXT && c.is_char_boundary(c.len()));
        assert_eq!(super::cap_text("ab".into()), "ab");
    }
}
