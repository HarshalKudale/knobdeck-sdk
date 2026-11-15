//! The selected text in the target app, read through UI Automation's `TextPattern` (Windows),
//! `AXSelectedText` (macOS) or AT-SPI `Text` (Linux, with the PRIMARY selection as a fallback for
//! apps that expose no accessibility tree). Never from a password field.

use crate::Result;

/// Longest text returned (1 MiB); longer selections are cut at a character boundary.
pub const MAX_TEXT: usize = crate::MAX_TEXT;

/// The selected text of the target app's focused element. `Ok(None)` when nothing is selected or
/// the focus is a password field; an error when the app doesn't expose its selection (try
/// [`text_via_copy`]).
pub fn text() -> Result<Option<String>> {
    if crate::focus::element().ok().flatten().is_some_and(|e| e.password) {
        return Ok(None);
    }
    sys::selected_text().map(|t| t.map(crate::cap_text).filter(|t| !t.is_empty()))
}

/// For apps that don't expose a selection (Electron apps, some editors): copy it and read the
/// clipboard, then put the previous clipboard back.
///
/// macOS presses the app's own Edit ▸ Copy menu item through Accessibility, falling back to
/// `send_copy_keys` (⌘C) for apps without one. Windows has no app-independent copy request, so
/// `send_copy_keys` is called (Ctrl+C): a keystroke, which is why this is a separate, opt-in call.
/// The caller passes its own way of sending keys (a plugin: `knobdeck_sdk::send_keys("ctrl+c")`,
/// which KnobDeck injects). Linux isn't supported yet: restoring the clipboard there needs a
/// process that keeps serving it.
pub fn text_via_copy(send_copy_keys: impl FnOnce() -> Result) -> Result<Option<String>> {
    if crate::focus::element().ok().flatten().is_some_and(|e| e.password) {
        return Ok(None);
    }
    via_copy(send_copy_keys).map(|t| t.map(crate::cap_text).filter(|t| !t.is_empty()))
}

#[cfg(target_os = "macos")]
fn via_copy(send_copy_keys: impl FnOnce() -> Result) -> Result<Option<String>> {
    crate::macos::focus::text_via_copy(send_copy_keys)
}

#[cfg(windows)]
fn via_copy(send_copy_keys: impl FnOnce() -> Result) -> Result<Option<String>> {
    crate::win_clipboard::copy_and_restore(send_copy_keys)
}

#[cfg(target_os = "linux")]
fn via_copy(_send_copy_keys: impl FnOnce() -> Result) -> Result<Option<String>> {
    Err(crate::Error::msg("Reading the selection by copying isn't supported on Linux yet"))
}

#[cfg(windows)]
use crate::win_focus as sys;

#[cfg(target_os = "macos")]
use crate::macos::focus as sys;

#[cfg(target_os = "linux")]
use crate::linux::atspi as sys;

