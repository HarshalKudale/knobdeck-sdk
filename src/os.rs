//! OS context, read in the plugin's own process when an action needs it, from the
//! [`knobdeck-os`](https://github.com/Knobdeck/knobdeck-sdk/tree/master/knobdeck-os) crate
//! KnobDeck itself uses. Nothing is synced or pushed to plugins; each part is a Cargo feature:
//!
//! - `os-window`: [`window`]: `target()` (the window an action should act on: the frontmost one
//!   that isn't KnobDeck's), `foreground()`, `list()`, `focus(id)`, `focus_app(app, hint)`,
//!   `watch(cb)`. KnobDeck's windows are excluded automatically.
//! - `os-apps`: [`apps`]: the installed apps, scanned once and kept.
//! - `os-focus`: [`focus`]: the target app's focused element (role, editable, password).
//! - `os-selection`: [`selection`]: the target app's selected text (`text()`, and the opt-in
//!   `text_via_copy()` for apps that don't expose it).
//! - `os-clipboard`: [`clipboard`]: the clipboard's kinds, text, files and image, read only.
//! - `os-files`: [`files`]: the files selected in File Explorer / Finder / Dolphin
//!   (`selected()`), and the folder that window shows (`current_folder()`).
//! - `os-document`: [`document`]: the file the target window has open (`path()`, macOS).
//! - `os-browser`: [`browser`]: the target browser's current tab (`tab()`: url, title).
//! - `os-pointer`: [`pointer`]: `position()` and `element_at()` (role, name, pid).
//!
//! In a handler, prefer the readers on [`Ctx`](crate::Ctx) (`ctx.window()`, `ctx.selection()`…)
//! with [`Action::needs`](crate::Action::needs): they read once per press, within a time budget.
//!
//! On Linux, `linux` also gives the KWin / GNOME bridges and XDG desktop entries.

pub use knobdeck_os::{Error, Result};

#[cfg(feature = "os-window")]
pub use knobdeck_os::window;

#[cfg(feature = "os-focus")]
pub use knobdeck_os::focus;

#[cfg(feature = "os-selection")]
pub use knobdeck_os::selection;

#[cfg(feature = "os-clipboard")]
pub use knobdeck_os::clipboard;

#[cfg(feature = "os-files")]
pub use knobdeck_os::files;

#[cfg(feature = "os-document")]
pub use knobdeck_os::document;

#[cfg(feature = "os-browser")]
pub use knobdeck_os::browser;

#[cfg(feature = "os-pointer")]
pub use knobdeck_os::pointer;

#[cfg(feature = "placeholders")]
pub use knobdeck_os::{input_box, placeholders};

#[cfg(target_os = "linux")]
pub use knobdeck_os::linux;

#[cfg(target_os = "macos")]
pub use knobdeck_os::macos;

/// Installed apps, scanned once per process (the scan can take a second: Windows asks the Shell
/// through PowerShell).
#[cfg(feature = "os-apps")]
pub mod apps {
    pub use knobdeck_os::apps::InstalledApp;
    use std::sync::Mutex;

    static CACHE: Mutex<Option<Vec<InstalledApp>>> = Mutex::new(None);

    /// The installed apps, from the cache once scanned.
    pub fn installed() -> Vec<InstalledApp> {
        if let Some(list) = CACHE.lock().unwrap().as_ref() {
            return list.clone();
        }
        rescan()
    }

    /// Scan again and keep the result.
    pub fn rescan() -> Vec<InstalledApp> {
        let list = knobdeck_os::apps::installed();
        *CACHE.lock().unwrap() = Some(list.clone());
        list
    }
}
