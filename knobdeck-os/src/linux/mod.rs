//! Linux: the desktop in use, D-Bus helpers, the KWin script and GNOME Shell extension bridges,
//! Hyprland and X11, and XDG desktop entries.
//!
//! Wayland gives clients no way to see or focus other apps' windows, so on Wayland the window
//! functions ask the compositor: Hyprland through its IPC socket, KDE Plasma through a KWin script
//! ([`kwin`]), GNOME through the KnobDeck Shell extension ([`gnome`]). Both bridges understand
//! the same commands (`list`, `active`, `activate`, ... see `knobdeck-plugins/linux-desktop/src/wm.rs`)
//! and answer with the same JSON. Any X11 session is read through EWMH.

pub mod bus;
pub mod desktop;
pub mod gnome;
pub mod kwin;

#[cfg(feature = "window")]
mod hypr;
#[cfg(feature = "window")]
pub(crate) mod window;
#[cfg(feature = "window")]
mod x11;

use crate::{Error, Result};
use serde_json::Value;

/// The desktop environments with a window bridge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Desktop {
    Kde,
    Gnome,
    Other,
}

/// The running desktop: the compositor that owns its D-Bus name, else `XDG_CURRENT_DESKTOP`.
pub fn desktop() -> Desktop {
    if bus::session_has_owner("org.kde.KWin") {
        return Desktop::Kde;
    }
    if bus::session_has_owner("org.gnome.Shell") {
        return Desktop::Gnome;
    }
    desktop_from_env(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default())
}

fn desktop_from_env(v: &str) -> Desktop {
    // `KDE`, `GNOME`, `ubuntu:GNOME`, `Pop:GNOME`, ...
    let parts: Vec<String> = v.split(':').map(|p| p.trim().to_ascii_lowercase()).collect();
    if parts.iter().any(|p| p == "kde") {
        Desktop::Kde
    } else if parts.iter().any(|p| p == "gnome") {
        Desktop::Gnome
    } else {
        Desktop::Other
    }
}

pub fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland")
}

/// Run one bridge command on KDE Plasma or GNOME.
pub fn bridge(cmd: &str, args: &Value) -> Result<Value> {
    match desktop() {
        Desktop::Kde => kwin::call(cmd, args),
        Desktop::Gnome => gnome::call(cmd, args),
        Desktop::Other => Err(Error::msg("Window actions need GNOME or KDE Plasma")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_env_parsing() {
        assert_eq!(desktop_from_env("KDE"), Desktop::Kde);
        assert_eq!(desktop_from_env("ubuntu:GNOME"), Desktop::Gnome);
        assert_eq!(desktop_from_env("GNOME-Classic:GNOME"), Desktop::Gnome);
        assert_eq!(desktop_from_env("Hyprland"), Desktop::Other);
        assert_eq!(desktop_from_env(""), Desktop::Other);
    }
}
