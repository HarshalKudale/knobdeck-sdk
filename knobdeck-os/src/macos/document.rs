//! macOS: the target app's focused window's `AXDocument` (a `file://` URL).

use super::window::{needs_accessibility, trusted, Ax};
use crate::{Error, Result};
use std::path::PathBuf;

pub fn path() -> Result<Option<PathBuf>> {
    if !trusted() {
        return Err(needs_accessibility());
    }
    let w = crate::window::target().ok_or_else(|| Error::msg("No window to read from"))?;
    let app = Ax::app(w.id.app_pid()).ok_or_else(needs_accessibility)?;
    let Some(win) = app.elem("AXFocusedWindow").or_else(|| app.elem("AXMainWindow")) else { return Ok(None) };
    Ok(win.string("AXDocument").and_then(|url| from_file_url(&url)))
}

/// `file:///Users/me/My%20Notes.txt` → `/Users/me/My Notes.txt`; other schemes → `None`.
fn from_file_url(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let path = if rest.starts_with('/') { rest } else { &rest[rest.find('/')?..] };
    let p = crate::percent_decode(path);
    // Folders come with a trailing slash.
    let p = if p.len() > 1 { p.trim_end_matches('/').to_string() } else { p };
    Some(PathBuf::from(p))
}
