//! Linux: the files selected in a file manager.
//!
//! - Dolphin: its accessibility tree (see [`super::atspi::dolphin_views`]): each view's folder and
//!   selected items. Accessibility is switched on for the session when it is off.
//! - Others: the PRIMARY selection, when the file manager puts the selected files there as a
//!   `text/uri-list`. Only read while a file manager is the target window, so a list left in
//!   PRIMARY by another app isn't taken for a selection.

use crate::{Error, Result};
use std::path::PathBuf;

/// Window classes / app ids of file managers (lowercase, matched as the id's last part).
const FILE_MANAGERS: &[&str] = &["dolphin", "nautilus", "nemo", "thunar", "caja", "pcmanfm", "pcmanfm-qt"];

fn last_part(app_id: &str) -> String {
    let id = app_id.to_ascii_lowercase();
    id.rsplit('.').next().unwrap_or(&id).to_string()
}

fn is_file_manager(app_id: &str) -> bool {
    FILE_MANAGERS.contains(&last_part(app_id).as_str())
}

/// The target window when it is a file manager: (its pid, whether it is Dolphin).
fn target() -> Result<(u32, bool)> {
    let w = crate::window::target().ok_or_else(crate::files::not_a_file_manager)?;
    let ids = [Some(w.app_id.as_str()), w.desktop_id.as_deref()];
    if !ids.iter().flatten().any(|id| is_file_manager(id)) {
        return Err(crate::files::not_a_file_manager());
    }
    Ok((w.pid, ids.iter().flatten().any(|id| last_part(id) == "dolphin")))
}

/// Dolphin's view with the keyboard focus, else one with a selection, else the first.
fn dolphin_view(pid: u32) -> Result<super::atspi::DolphinView> {
    let mut views = super::atspi::dolphin_views(pid)?;
    let pick = views.iter().position(|v| v.focused).or_else(|| views.iter().position(|v| !v.selected.is_empty())).unwrap_or(0);
    Ok(views.swap_remove(pick))
}

pub fn selected() -> Result<Vec<PathBuf>> {
    let (pid, dolphin) = target()?;
    if dolphin {
        return Ok(dolphin_view(pid)?.selected);
    }
    let files = super::clipboard::primary_files()?;
    if files.is_empty() {
        return Err(Error::msg("This file manager doesn't share its selection; Dolphin does"));
    }
    Ok(files)
}

pub fn current_folder() -> Result<Option<PathBuf>> {
    let (pid, dolphin) = target()?;
    if dolphin {
        return Ok(dolphin_view(pid)?.folder);
    }
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

    /// Prints the selection of every running Dolphin (no focus change needed).
    /// `cargo test -p knobdeck-os --features files live_dolphin -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_dolphin() {
        let out = std::process::Command::new("pidof").arg("dolphin").output().unwrap();
        for pid in String::from_utf8_lossy(&out.stdout).split_whitespace().filter_map(|p| p.parse().ok()) {
            let t = std::time::Instant::now();
            eprintln!("dolphin {pid}: {:?} in {:?}", super::super::atspi::dolphin_views(pid), t.elapsed());
        }
    }
}
