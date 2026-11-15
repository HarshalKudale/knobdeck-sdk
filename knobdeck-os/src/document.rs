//! The file the target window has open, where the OS says: macOS apps report it as the focused
//! window's `AXDocument` (TextEdit, Preview, Pages, Xcode, most document-based apps). Windows and
//! Linux have no such property; there it is an error.

use crate::Result;
use std::path::PathBuf;

/// The target window's document. `Ok(None)` when the window has no saved document (a new,
/// unsaved one, or an app that doesn't report it).
pub fn path() -> Result<Option<PathBuf>> {
    sys::path()
}

#[cfg(target_os = "macos")]
use crate::macos::document as sys;

#[cfg(not(target_os = "macos"))]
mod sys {
    pub fn path() -> crate::Result<Option<std::path::PathBuf>> {
        Err(crate::Error::msg("Reading the open document is supported on macOS only"))
    }
}
