//! Linux: the files selected in a file manager, from the PRIMARY selection.
//!
//! Dolphin puts the selected items in PRIMARY as a `text/uri-list` (as does any app that sets
//! PRIMARY to files). Only read while a file manager is the target window, so a list left in
//! PRIMARY by another app isn't taken for a selection.

use crate::{Error, Result};
use std::path::PathBuf;

/// Window classes / app ids of file managers (lowercase, matched as a suffix of the id).
const FILE_MANAGERS: &[&str] = &["dolphin", "nautilus", "nemo", "thunar", "caja", "pcmanfm", "pcmanfm-qt"];

fn is_file_manager(app_id: &str) -> bool {
    let id = app_id.to_ascii_lowercase();
    let last = id.rsplit('.').next().unwrap_or(&id);
    FILE_MANAGERS.contains(&last)
}

pub fn selected() -> Result<Vec<PathBuf>> {
    let target = crate::window::target();
    if !target.as_ref().is_some_and(|w| is_file_manager(&w.app_id) || w.desktop_id.as_deref().is_some_and(is_file_manager)) {
        return Err(crate::files::not_a_file_manager());
    }
    let files = super::clipboard::primary_files()?;
    if files.is_empty() && target.is_some_and(|w| !w.app_id.to_ascii_lowercase().contains("dolphin")) {
        return Err(Error::msg("This file manager doesn't share its selection; Dolphin does"));
    }
    Ok(files)
}

pub fn current_folder() -> Result<Option<PathBuf>> {
    Ok(selected()?.first().and_then(|p| p.parent()).map(PathBuf::from))
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_managers_by_id() {
        assert!(super::is_file_manager("org.kde.dolphin"));
        assert!(super::is_file_manager("org.gnome.Nautilus"));
        assert!(super::is_file_manager("dolphin"));
        assert!(!super::is_file_manager("firefox"));
    }
}
