//! The OS's own one-line input box, for `{input:Prompt}`: `display dialog … default answer`
//! (macOS), Visual Basic's `InputBox` through PowerShell (Windows has no input-box API), `zenity
//! --entry` (GNOME) or `kdialog --inputbox` (KDE Plasma). Blocks until the user answers.

use crate::{Error, Result};
use std::process::Command;

/// The error message for a cancelled input box; see [`is_cancelled`].
pub const CANCELLED: &str = "Cancelled";

/// Whether `e` is the user cancelling an input box (callers stop quietly).
pub fn is_cancelled(e: &Error) -> bool {
    e.message() == CANCELLED
}

/// Ask for one line of text. `Err(CANCELLED)` when the user cancels or closes the box.
pub fn ask(prompt: &str) -> Result<String> {
    let out = command(prompt)?.output().map_err(|e| Error::msg(format!("Couldn't show the input box: {e}")))?;
    if !out.status.success() {
        return Err(Error::msg(CANCELLED));
    }
    let text = String::from_utf8_lossy(&out.stdout).trim_end_matches(['\r', '\n']).to_string();
    // Windows' InputBox gives no separate cancel signal: an empty answer is a cancel there.
    if cfg!(windows) && text.is_empty() {
        return Err(Error::msg(CANCELLED));
    }
    Ok(text)
}

#[cfg(target_os = "macos")]
fn command(prompt: &str) -> Result<Command> {
    let quoted = format!("\"{}\"", prompt.replace('\\', "\\\\").replace('"', "\\\""));
    let mut c = Command::new("/usr/bin/osascript");
    c.args([
        "-e",
        "activate",
        "-e",
        &format!("text returned of (display dialog {quoted} default answer \"\" with title \"KnobDeck\")"),
    ]);
    Ok(c)
}

#[cfg(windows)]
fn command(prompt: &str) -> Result<Command> {
    use std::os::windows::process::CommandExt;
    // The prompt goes in through the environment, so nothing in it is parsed as PowerShell.
    let script = "[Console]::OutputEncoding = [Text.Encoding]::UTF8; Add-Type -AssemblyName Microsoft.VisualBasic; \
                  [Microsoft.VisualBasic.Interaction]::InputBox($env:KNOBDECK_PROMPT, 'KnobDeck', '')";
    let mut c = Command::new("powershell.exe");
    c.args(["-NoProfile", "-NonInteractive", "-Command", script]).env("KNOBDECK_PROMPT", prompt).creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    Ok(c)
}

#[cfg(target_os = "linux")]
fn command(prompt: &str) -> Result<Command> {
    let on_path = |p: &str| std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|d| d.join(p).is_file()));
    let kde = crate::linux::desktop() == crate::linux::Desktop::Kde;
    // KDE's own dialog on Plasma, GNOME's elsewhere; either one if only it is installed.
    let pick = if kde && on_path("kdialog") { "kdialog" } else if on_path("zenity") { "zenity" } else if on_path("kdialog") { "kdialog" } else {
        return Err(Error::msg("The input box needs zenity (GNOME) or kdialog (KDE Plasma)"));
    };
    let mut c = Command::new(pick);
    if pick == "kdialog" {
        c.args(["--title", "KnobDeck", "--inputbox", prompt]);
    } else {
        c.args(["--entry", "--title", "KnobDeck", "--text", prompt]);
    }
    Ok(c)
}
