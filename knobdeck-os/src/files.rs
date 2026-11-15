//! The files selected in the target file manager window, and the folder it shows.
//!
//! - Windows: File Explorer through the Shell's COM objects (`IShellWindows` → the Explorer
//!   window that is the target, its active tab's `IFolderView`), and the desktop the same way.
//! - macOS: Finder through Apple Events (`selection`, `insertion location`). The first read asks
//!   for the Automation permission ("KnobDeck wants to control Finder").
//! - Linux (GNOME, KDE Plasma): file managers that put the selected files in the PRIMARY
//!   selection as a `text/uri-list` (Dolphin; Nautilus and Nemo where they do). The current
//!   folder is the selection's parent; with nothing selected it isn't known.
//!
//! Other apps get a clear error: the target window must be a file manager.

use crate::Result;
use std::path::PathBuf;

/// Longest file list returned.
pub const MAX_FILES: usize = 10_000;

/// The files and folders selected in the target window (the frontmost one that isn't
/// KnobDeck's), at most [`MAX_FILES`]. Empty when nothing is selected; an error when the target
/// isn't a file manager KnobDeck can read.
pub fn selected() -> Result<Vec<PathBuf>> {
    let mut f = sys::selected()?;
    f.truncate(MAX_FILES);
    Ok(f)
}

/// The folder the target file manager window shows (the desktop folder for the desktop). `None`
/// where it isn't a file-system folder (This PC, Network) or the OS won't say.
pub fn current_folder() -> Result<Option<PathBuf>> {
    sys::current_folder()
}

/// An error for a target window that isn't a supported file manager.
#[allow(dead_code)]
pub(crate) fn not_a_file_manager() -> crate::Error {
    crate::Error::msg(if cfg!(windows) {
        "Select files in File Explorer or on the desktop first"
    } else if cfg!(target_os = "macos") {
        "Select files in Finder first"
    } else {
        "Select files in Dolphin first"
    })
}

#[cfg(windows)]
use crate::win_files as sys;

#[cfg(target_os = "macos")]
use crate::macos::files as sys;

#[cfg(target_os = "linux")]
use crate::linux::files as sys;
