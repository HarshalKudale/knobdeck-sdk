//! Windows: Win32 windows, focus and WinEvent hooks.

use crate::window::{Rect, Window, WindowState};
use crate::{Error, Result};
use std::ffi::c_void;
use std::sync::mpsc::Sender;
use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, RECT, TRUE};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub type RawId = isize;

fn h(id: RawId) -> HWND {
    id as HWND
}

fn title(hwnd: HWND) -> Option<String> {
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
        return None;
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    Some(String::from_utf16_lossy(&buf[..n.max(0) as usize])).filter(|t| !t.is_empty())
}

fn pid_of(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid
}

/// Full path of the exe running as `pid`.
pub fn exe_path(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string())
}

fn style(hwnd: HWND) -> u32 {
    unsafe { GetWindowLongW(hwnd, GWL_STYLE) as u32 }
}

fn ex_style(hwnd: HWND) -> u32 {
    unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 }
}

fn rect_of(r: RECT) -> Rect {
    Rect { x: r.left as f64, y: r.top as f64, w: (r.right - r.left) as f64, h: (r.bottom - r.top) as f64 }
}

/// Monitor handles, left to right then top to bottom (the order win-tools numbers them in).
fn monitors() -> Vec<(HMONITOR, RECT)> {
    unsafe extern "system" fn cb(hm: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let list = &mut *(data as *mut Vec<(HMONITOR, RECT)>);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(hm, &mut info) != 0 {
            list.push((hm, info.rcMonitor));
        }
        TRUE
    }
    let mut list: Vec<(HMONITOR, RECT)> = vec![];
    unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(cb), &mut list as *mut _ as LPARAM) };
    list.sort_by_key(|(_, r)| (r.left, r.top));
    list
}

/// Covers its whole monitor without a title bar (a game, a video, a presentation). A captioned
/// window covering the monitor is only maximized (with an auto-hiding taskbar).
fn is_fullscreen(hwnd: HWND, mon: &RECT) -> bool {
    let mut class = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) }.max(0) as usize;
    if matches!(String::from_utf16_lossy(&class[..n]).as_str(), "Progman" | "WorkerW") {
        return false;
    }
    if style(hwnd) & WS_CAPTION == WS_CAPTION {
        return false;
    }
    let mut wr: RECT = unsafe { std::mem::zeroed() };
    if unsafe { GetWindowRect(hwnd, &mut wr) } == 0 {
        return false;
    }
    wr.left <= mon.left && wr.top <= mon.top && wr.right >= mon.right && wr.bottom >= mon.bottom
}

fn describe(hwnd: HWND) -> Option<Window> {
    if hwnd.is_null() || unsafe { IsWindow(hwnd) } == 0 {
        return None;
    }
    let pid = pid_of(hwnd);
    let path = exe_path(pid);
    let mut r: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetWindowRect(hwnd, &mut r) };
    let hm = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let mons = monitors();
    let monitor = mons.iter().position(|(m, _)| *m == hm);
    let fullscreen = monitor.is_some_and(|i| is_fullscreen(hwnd, &mons[i].1));
    Some(Window {
        id: crate::window::WindowId(hwnd as isize),
        pid,
        app_id: path.as_deref().map(file_name).unwrap_or_default(),
        app_name: path.as_deref().and_then(|p| std::path::Path::new(p).file_stem()).map(|s| s.to_string_lossy().into_owned()),
        title: title(hwnd),
        bounds: rect_of(r),
        monitor,
        state: WindowState {
            maximized: unsafe { IsZoomed(hwnd) } != 0,
            minimized: unsafe { IsIconic(hwnd) } != 0,
            fullscreen,
            topmost: ex_style(hwnd) & WS_EX_TOPMOST != 0,
        },
        desktop_id: None,
    })
}

pub fn foreground() -> Option<Window> {
    describe(unsafe { GetForegroundWindow() })
}

fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    let hr = unsafe { DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED as u32, &mut cloaked as *mut u32 as *mut c_void, 4) };
    hr == 0 && cloaked != 0
}

/// Visible, top level, not owned, not a bare tool window, not cloaked: what the taskbar shows.
fn is_open_window(hwnd: HWND) -> bool {
    unsafe {
        if IsWindowVisible(hwnd) == 0 || !GetWindow(hwnd, GW_OWNER).is_null() {
            return false;
        }
    }
    let ex = ex_style(hwnd);
    if ex & WS_EX_TOOLWINDOW != 0 && ex & WS_EX_APPWINDOW == 0 {
        return false;
    }
    !is_cloaked(hwnd)
}

fn top_level_windows() -> Vec<HWND> {
    unsafe extern "system" fn cb(hwnd: HWND, data: LPARAM) -> BOOL {
        (*(data as *mut Vec<HWND>)).push(hwnd);
        TRUE
    }
    let mut out: Vec<HWND> = vec![];
    unsafe { EnumWindows(Some(cb), &mut out as *mut _ as LPARAM) };
    out
}

/// Titled taskbar windows in Z-order (front first).
pub fn list() -> Result<Vec<Window>> {
    Ok(top_level_windows().into_iter().filter(|w| is_open_window(*w) && title(*w).is_some()).filter_map(describe).collect())
}

/// Restore if minimized, raise and foreground. A bare `SetForegroundWindow` from a background
/// process only flashes the taskbar button, so borrow the foreground thread's input state with
/// `AttachThreadInput` for the call. (The KnobDeck host also grants plugins the right to take
/// the foreground with `AllowSetForegroundWindow` before each invoke.)
pub fn focus(id: &RawId) -> Result {
    let hwnd = h(*id);
    if unsafe { IsWindow(hwnd) } == 0 {
        return Err(Error::msg("That window is no longer open"));
    }
    unsafe {
        let me = GetCurrentThreadId();
        let fg = GetForegroundWindow();
        let fg_thread = if fg.is_null() { 0 } else { GetWindowThreadProcessId(fg, std::ptr::null_mut()) };
        let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, TRUE) != 0;
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        } else {
            ShowWindow(hwnd, SW_SHOW);
        }
        BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd);
        if attached {
            AttachThreadInput(me, fg_thread, 0);
        }
    }
    Ok(())
}

/// `app` is an exe path or name (matched on the file name), or a packaged app's
/// `shell:AppsFolder\<AUMID>` (no exe to compare: matched on the title hint).
pub fn focus_app(app: &str, title_hint: Option<&str>) -> Result<bool> {
    let exe = file_name(app).to_ascii_lowercase();
    let title_only = app.starts_with("shell:AppsFolder\\") && !exe.ends_with(".exe");
    let hint = title_hint.map(str::to_ascii_lowercase);
    let hit = list()?.into_iter().filter(|w| !crate::window::skipped(w.pid)).find(|w| {
        let by_exe = !title_only && w.app_id.eq_ignore_ascii_case(&exe);
        let by_title = title_only && hint.as_ref().is_some_and(|n| w.title.as_deref().is_some_and(|t| t.to_ascii_lowercase().contains(n.as_str())));
        by_exe || by_title
    });
    match hit {
        Some(w) => focus(&w.id.0).map(|_| true),
        None => Ok(false),
    }
}

// ── Watcher ─────────────────────────────────────────────────────────────────

const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
const EVENT_SYSTEM_MINIMIZESTART: u32 = 0x0016;
const EVENT_SYSTEM_MINIMIZEEND: u32 = 0x0017;
const EVENT_OBJECT_LOCATIONCHANGE: u32 = 0x800B;
const EVENT_OBJECT_NAMECHANGE: u32 = 0x800C;

thread_local! {
    static TICK: std::cell::RefCell<Option<Sender<()>>> = const { std::cell::RefCell::new(None) };
}

unsafe extern "system" fn on_event(_hook: HWINEVENTHOOK, event: u32, hwnd: HWND, id_object: i32, _child: i32, _thread: u32, _time: u32) {
    // Title and location changes count only for the foreground window itself (not its child
    // objects or background windows): LOCATIONCHANGE also reports maximize / restore / full screen.
    let foreground_only = matches!(event, EVENT_OBJECT_NAMECHANGE | EVENT_OBJECT_LOCATIONCHANGE);
    if foreground_only && (id_object != OBJID_WINDOW || hwnd != GetForegroundWindow()) {
        return;
    }
    TICK.with(|t| {
        if let Some(tx) = t.borrow().as_ref() {
            let _ = tx.send(());
        }
    });
}

/// A thread with three WinEvent hooks and a message loop; dropping it posts `WM_QUIT`.
pub struct Source {
    thread_id: u32,
}

impl Drop for Source {
    fn drop(&mut self) {
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
    }
}

pub fn source(tick: Sender<()>) -> Result<Source> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("knobdeck-os-winevent".into())
        .spawn(move || unsafe {
            TICK.with(|t| *t.borrow_mut() = Some(tick));
            let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
            // Separate ranges: a combined one would wake us for every object event system-wide.
            let hooks = [
                SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, std::ptr::null_mut(), Some(on_event), 0, 0, flags),
                SetWinEventHook(EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND, std::ptr::null_mut(), Some(on_event), 0, 0, flags),
                SetWinEventHook(EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_NAMECHANGE, std::ptr::null_mut(), Some(on_event), 0, 0, flags),
            ];
            // Make sure this thread has a message queue before anyone posts WM_QUIT to it.
            let mut msg: MSG = std::mem::zeroed();
            PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
            let _ = ready_tx.send(GetCurrentThreadId());
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            for hk in hooks {
                if !hk.is_null() {
                    UnhookWinEvent(hk);
                }
            }
        })
        .map_err(|e| Error::msg(format!("Could not start the window watcher: {e}")))?;
    let thread_id = ready_rx.recv().map_err(|_| Error::msg("The window watcher did not start"))?;
    Ok(Source { thread_id })
}
