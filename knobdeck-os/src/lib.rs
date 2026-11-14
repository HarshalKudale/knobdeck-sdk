//! # knobdeck-os
//!
//! What KnobDeck and its plugins read from the OS, in one implementation for Windows, macOS and
//! Linux. The KnobDeck host uses it directly; plugins get it through `knobdeck_sdk::os` (SDK
//! features `os-window`, `os-apps`).
//!
//! - [`window`]: the foreground window, the *target* window (the frontmost one that isn't
//!   KnobDeck's), the window list, focusing a window or an app, and an event-driven watcher.
//! - [`apps`]: the installed apps (Start menu / `.app` bundles / XDG desktop entries).
//!
//! Nothing here polls: [`window::watch`] runs on the OS's own events and stops when its handle
//! is dropped.
//!
//! On Linux the window functions work on Hyprland (its IPC socket), any X11 session (EWMH), KDE
//! Plasma (a KWin script) and GNOME (the KnobDeck Shell extension, see [`linux::gnome`]); other
//! Wayland desktops get a clear error.

mod error;
pub use error::{Error, Result};

#[cfg(feature = "window")]
pub mod window;

#[cfg(feature = "apps")]
pub mod apps;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(all(windows, feature = "window"))]
mod win;
