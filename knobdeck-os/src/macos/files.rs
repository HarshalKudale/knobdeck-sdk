//! macOS: Finder's selection and folder through Apple Events.
//!
//! `osascript` runs the script: `NSAppleScript` is main-thread only and plugins read context on
//! worker threads. The first read asks for the Automation permission (KnobDeck → Finder); a
//! refusal comes back as error -1743 with a message that says where to allow it.

use crate::{Error, Result};
use std::path::PathBuf;
use std::process::Command;

const FINDER: &str = "com.apple.finder";

const SELECTION: &str = r#"tell application "Finder"
  set out to ""
  repeat with f in (get selection)
    set out to out & POSIX path of (f as alias) & linefeed
  end repeat
  return out
end tell"#;

const FOLDER: &str = r#"tell application "Finder" to return POSIX path of (insertion location as alias)"#;

fn run(script: &str) -> Result<String> {
    let target = crate::window::target();
    if !target.as_ref().is_some_and(|w| w.app_id == FINDER) {
        return Err(crate::files::not_a_file_manager());
    }
    let out = Command::new("/usr/bin/osascript").arg("-e").arg(script).output().map_err(|e| Error::msg(e.to_string()))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let msg = String::from_utf8_lossy(&out.stderr);
    if msg.contains("-1743") {
        return Err(Error::msg("Allow KnobDeck to control Finder in System Settings ▸ Privacy & Security ▸ Automation"));
    }
    Err(Error::msg(format!("Finder: {}", msg.trim())))
}

/// Finder's POSIX paths end folders with `/`; keep the path, drop the slash (except `/`).
fn path(line: &str) -> PathBuf {
    let t = line.strip_suffix('/').filter(|t| !t.is_empty()).unwrap_or(line);
    PathBuf::from(t)
}

pub fn selected() -> Result<Vec<PathBuf>> {
    Ok(run(SELECTION)?.lines().filter(|l| !l.is_empty()).map(path).collect())
}

pub fn current_folder() -> Result<Option<PathBuf>> {
    Ok(Some(run(FOLDER)?.trim_end_matches('\n')).filter(|s| !s.is_empty()).map(path))
}
