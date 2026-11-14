//! Windows and focus.
//!
//! - [`foreground`]: the window in front, whoever owns it (one OS query).
//! - [`target`]: the window an action should act on: the frontmost normal window that belongs
//!   neither to this process nor to the process set with [`exclude_pid`] (KnobDeck itself, so a
//!   slider that keeps the dial focused still acts on the app under it).
//! - [`list`]: normal windows, front to back, for switcher wheels.
//! - [`focus`] / [`focus_app`]: bring a window or an app's frontmost window to the front.
//! - [`watch`]: call back when the target window, its title or its state changes. Event-driven
//!   on every OS; dropping the [`WatchHandle`] stops it.
//!
//! `app_id` is the exe file name on Windows (`code.exe`), the bundle id on macOS
//! (`com.microsoft.VSCode`) and the window class / Wayland app id on Linux (`code`,
//! `org.gnome.Nautilus`).

use crate::Result;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use crate::linux::window as sys;
#[cfg(target_os = "macos")]
use crate::macos::window as sys;
#[cfg(windows)]
use crate::win as sys;

/// A window frame in global coordinates: physical pixels on Windows, points on macOS, logical
/// pixels on Linux (the compositor's own space).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    pub fn contains(&self, (x, y): (f64, f64)) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// The window states an action may toggle. `false` where the OS has no such state (macOS has no
/// "topmost" for other apps' windows).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowState {
    pub maximized: bool,
    pub minimized: bool,
    pub fullscreen: bool,
    pub topmost: bool,
}

/// Opaque handle of one window: an `HWND` on Windows, an Accessibility element on macOS, a
/// compositor / X11 id on Linux. Valid while the window is open.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowId(pub(crate) sys::RawId);

impl WindowId {
    /// The window's `HWND`.
    #[cfg(windows)]
    pub fn hwnd(&self) -> isize {
        self.0
    }
    #[cfg(windows)]
    pub fn from_hwnd(hwnd: isize) -> WindowId {
        WindowId(hwnd)
    }
    /// The window's `AXUIElementRef` (borrowed: retain it to keep it), when Accessibility is granted.
    #[cfg(target_os = "macos")]
    pub fn ax_element(&self) -> Option<*const std::ffi::c_void> {
        self.0.window.as_ref().map(|w| w.as_ptr())
    }
    /// The owning app's pid.
    #[cfg(target_os = "macos")]
    pub fn app_pid(&self) -> i32 {
        self.0.pid
    }
    /// The KWin / GNOME bridge id (`knobdeck-plugins/linux-desktop` commands take it).
    #[cfg(target_os = "linux")]
    pub fn bridge_id(&self) -> Option<&str> {
        match &self.0 {
            sys::RawId::Bridge(id) => Some(id),
            _ => None,
        }
    }
}

/// One top-level window.
#[derive(Clone, Debug)]
pub struct Window {
    pub id: WindowId,
    /// Owning process (0 when the OS doesn't say).
    pub pid: u32,
    /// Exe name (Windows), bundle id (macOS), window class / app id (Linux).
    pub app_id: String,
    /// The app's display name, where the OS has one.
    pub app_name: Option<String>,
    pub title: Option<String>,
    pub bounds: Rect,
    /// Index of the monitor it is on, where the OS says.
    pub monitor: Option<usize>,
    pub state: WindowState,
    /// Linux: the app's desktop entry id (`firefox`, `org.kde.dolphin`) when the compositor knows it.
    pub desktop_id: Option<String>,
}

impl Window {
    /// What [`watch`] compares: a change of window, app, title or state (not position).
    fn key(&self) -> (WindowId, String, Option<String>, WindowState) {
        (self.id.clone(), self.app_id.clone(), self.title.clone(), self.state)
    }
}

static EXCLUDED: AtomicU32 = AtomicU32::new(0);

/// Never treat windows of `pid` as the target (KnobDeck's own pid, in a plugin). This process's
/// windows are always skipped.
pub fn exclude_pid(pid: u32) {
    EXCLUDED.store(pid, Ordering::Relaxed);
}

/// The pid set with [`exclude_pid`], if any.
pub fn excluded_pid() -> Option<u32> {
    Some(EXCLUDED.load(Ordering::Relaxed)).filter(|p| *p != 0)
}

pub(crate) fn skipped(pid: u32) -> bool {
    pid != 0 && (pid == std::process::id() || Some(pid) == excluded_pid())
}

/// The window in front, whoever owns it. `None` when nothing has focus or the OS won't say.
pub fn foreground() -> Option<Window> {
    sys::foreground()
}

/// The frontmost normal window that isn't this process's or the excluded one's.
pub fn target() -> Option<Window> {
    if let Some(w) = foreground().filter(|w| !skipped(w.pid)) {
        return Some(w);
    }
    sys::list().ok()?.into_iter().find(|w| !skipped(w.pid) && !w.state.minimized)
}

/// Normal windows, front to back (minimized ones last on some systems), without this process's
/// and the excluded process's windows.
pub fn list() -> Result<Vec<Window>> {
    Ok(sys::list()?.into_iter().filter(|w| !skipped(w.pid)).collect())
}

/// Bring a window to the front and give it focus, restoring it if it is minimized.
pub fn focus(id: &WindowId) -> Result {
    sys::focus(&id.0)
}

/// Bring `app`'s frontmost window to the front: `app` is an exe name or path (Windows), a bundle
/// id, `.app` path or exe name (macOS), a window class, app id or `.desktop` path (Linux).
/// `title_hint` also matches a window title (Windows packaged apps) or the app's display name.
/// `Ok(false)` when the app has no open window.
pub fn focus_app(app: &str, title_hint: Option<&str>) -> Result<bool> {
    let app = app.trim();
    if app.is_empty() {
        return Err(crate::Error::msg("No app given"));
    }
    sys::focus_app(app, title_hint.map(str::trim).filter(|s| !s.is_empty()))
}

/// Stops its watcher when dropped.
pub struct WatchHandle {
    _source: sys::Source,
    _tick: Sender<()>,
}

/// After an event, wait this long for more before reading the window (a drag or an app's
/// title animation fires many).
const SETTLE: Duration = Duration::from_millis(40);
/// ... but never longer than this.
const MAX_SETTLE: Duration = Duration::from_millis(200);

/// Call `on_change` with the [`target`] window now and whenever it changes: another window comes
/// to the front, or the target's title or [`WindowState`] changes. Event-driven: Windows
/// WinEvent hooks, macOS workspace notifications + an Accessibility observer, Hyprland's event
/// socket, X11 property events, a KWin script on KDE, the GNOME extension's D-Bus signal.
///
/// macOS: the events arrive on the main run loop, so the process must run it (the KnobDeck app
/// does; the SDK runs it for plugins with the `os-window` feature, see [`crate::macos::run_main_loop`]).
pub fn watch(on_change: impl Fn(Option<&Window>) + Send + 'static) -> Result<WatchHandle> {
    let (tx, rx) = channel::<()>();
    let source = sys::source(tx.clone())?;
    std::thread::Builder::new()
        .name("knobdeck-os-watch".into())
        .spawn(move || run_watch(rx, on_change))
        .map_err(|e| crate::Error::msg(format!("Could not start the window watcher: {e}")))?;
    Ok(WatchHandle { _source: source, _tick: tx })
}

fn run_watch(rx: Receiver<()>, on_change: impl Fn(Option<&Window>)) {
    let mut last = None;
    let emit = |last: &mut Option<_>| {
        let cur = target();
        let key = cur.as_ref().map(Window::key);
        if *last != Some(key.clone()) {
            *last = Some(key);
            on_change(cur.as_ref());
        }
    };
    emit(&mut last);
    while rx.recv().is_ok() {
        let start = Instant::now();
        loop {
            match rx.recv_timeout(SETTLE) {
                Ok(()) if start.elapsed() < MAX_SETTLE => continue,
                Ok(()) | Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        emit(&mut last);
    }
}

/// Wait up to `timeout` for a target window that `pred` accepts, driven by [`watch`]. Returns it,
/// or `None` on timeout.
pub fn wait_for(timeout: Duration, pred: impl Fn(&Window) -> bool + Send + 'static) -> Option<Window> {
    let (tx, rx) = channel();
    let _handle = watch(move |w| {
        if let Some(w) = w.filter(|w| pred(w)) {
            let _ = tx.send(w.clone());
        }
    })
    .ok()?;
    rx.recv_timeout(timeout).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_process_is_always_skipped() {
        assert!(skipped(std::process::id()));
        assert!(!skipped(0));
        exclude_pid(424242);
        assert!(skipped(424242));
        exclude_pid(0);
        assert!(!skipped(424242));
    }

    /// Prints what the running desktop reports; changes nothing.
    /// `cargo test -p knobdeck-os live_read -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_read() {
        let t = Instant::now();
        eprintln!("foreground: {:?} in {:?}", foreground(), t.elapsed());
        let t = Instant::now();
        eprintln!("target: {:?} in {:?}", target(), t.elapsed());
        for w in list().unwrap() {
            eprintln!("{} | {:?} | {:?} | {:?}", w.app_id, w.title, w.state, w.bounds);
        }
    }

    /// Prints the target for 20 s while you switch windows / maximize / retitle.
    #[test]
    #[ignore]
    fn live_watch() {
        let _h = watch(|w| eprintln!("{:?} {:?} {:?}", w.map(|w| &w.app_id), w.and_then(|w| w.title.as_ref()), w.map(|w| w.state))).unwrap();
        std::thread::sleep(Duration::from_secs(std::env::var("WATCH_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(20)));
    }
}
