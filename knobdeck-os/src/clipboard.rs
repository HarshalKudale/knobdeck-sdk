//! The clipboard, read only: what kinds of data it holds, its text, its files, its image.
//!
//! Windows: the Win32 clipboard. macOS: `NSPasteboard`. Linux: Wayland clients can't read the
//! clipboard without focus, so it goes through the compositor's data-control protocol
//! (`ext-data-control-v1` or `wlr-data-control`: KDE Plasma, Hyprland, sway and other wlroots
//! desktops), the KnobDeck GNOME Shell extension on GNOME, or the X11 selection.

use crate::Result;
use std::path::PathBuf;

/// Longest file list returned.
pub const MAX_FILES: usize = 10_000;

/// A kind of data on the clipboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Text,
    Files,
    Image,
    Html,
}

/// An image as the OS holds it: PNG where the clipboard has one, else TIFF (macOS) or BMP
/// (Windows device-independent bitmaps).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// `image/png`, `image/tiff` or `image/bmp`.
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// The kinds of data the clipboard holds now.
pub fn types() -> Result<Vec<Kind>> {
    let mut k = sys::types()?;
    k.dedup();
    Ok(k)
}

/// The clipboard's text, `None` when it holds none.
pub fn text() -> Result<Option<String>> {
    Ok(sys::text()?.map(crate::cap_text))
}

/// The files on the clipboard (copied in Explorer / Finder / a file manager), at most
/// [`MAX_FILES`].
pub fn files() -> Result<Vec<PathBuf>> {
    let mut f = sys::files()?;
    f.truncate(MAX_FILES);
    Ok(f)
}

/// The clipboard's image, `None` when it holds none.
pub fn image() -> Result<Option<Image>> {
    sys::image()
}

#[cfg_attr(windows, allow(dead_code))]
/// Paths from a `text/uri-list` (`file://` URIs; other schemes and comments skipped).
pub(crate) fn parse_uri_list(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.strip_prefix("file://"))
        .map(|rest| {
            // `file://host/path`: drop a host part (usually empty or localhost).
            let path = if rest.starts_with('/') { rest } else { rest.find('/').map_or("", |i| &rest[i..]) };
            PathBuf::from(percent_decode(path))
        })
        .filter(|p| p.as_os_str().len() > 1)
        .collect()
}

#[cfg_attr(windows, allow(dead_code))]
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(windows)]
use crate::win_clipboard as sys;

#[cfg(target_os = "macos")]
use crate::macos::clipboard as sys;

#[cfg(target_os = "linux")]
use crate::linux::clipboard as sys;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_lists() {
        let l = parse_uri_list("# comment\r\nfile:///home/a/My%20File.png\r\nhttps://x.y/z\nfile://localhost/tmp/b\n");
        assert_eq!(l, vec![PathBuf::from("/home/a/My File.png"), PathBuf::from("/tmp/b")]);
        assert_eq!(percent_decode("a%2"), "a%2");
    }
}
