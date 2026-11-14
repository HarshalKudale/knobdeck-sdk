//! macOS window back end.
//!
//! Window order comes from CGWindowList (owner pids only: no Screen Recording permission), the
//! windows themselves from Accessibility (`AXUIElement`), which KnobDeck holds and its plugins
//! inherit. Without Accessibility, [`foreground`] still names the front app, without a window.

use crate::window::{Rect, Window, WindowId, WindowState};
use crate::{Error, Result};
use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFEqual, CFGetTypeID, CFRelease, CFRetain, CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopSourceRef};
use core_foundation::string::{CFString, CFStringRef};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};
use objc2_foundation::{NSNotification, NSObjectProtocol};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

type AXUIElementRef = *const c_void;
type AXObserverRef = *const c_void;
type AXObserverCallback = unsafe extern "C" fn(AXObserverRef, AXUIElementRef, CFStringRef, *mut c_void);

const AX_VALUE_CGPOINT: u32 = 1;
const AX_VALUE_CGSIZE: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGSize {
    width: f64,
    height: f64,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(el: AXUIElementRef, attr: CFStringRef, value: *mut CFTypeRef) -> i32;
    fn AXUIElementSetAttributeValue(el: AXUIElementRef, attr: CFStringRef, value: CFTypeRef) -> i32;
    fn AXUIElementPerformAction(el: AXUIElementRef, action: CFStringRef) -> i32;
    fn AXUIElementGetPid(el: AXUIElementRef, pid: *mut i32) -> i32;
    fn AXUIElementGetTypeID() -> usize;
    fn AXValueGetValue(v: CFTypeRef, kind: u32, out: *mut c_void) -> bool;
    fn AXObserverCreate(pid: i32, callback: AXObserverCallback, out: *mut AXObserverRef) -> i32;
    fn AXObserverAddNotification(obs: AXObserverRef, el: AXUIElementRef, notification: CFStringRef, refcon: *mut c_void) -> i32;
    fn AXObserverGetRunLoopSource(obs: AXObserverRef) -> CFRunLoopSourceRef;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFArrayRef;
}

// ── AX elements ─────────────────────────────────────────────────────────────

/// An owned (+1) AXUIElement. AX calls are thread-safe.
pub struct Ax(AXUIElementRef);

unsafe impl Send for Ax {}
unsafe impl Sync for Ax {}

impl Ax {
    /// Take ownership of a +1 reference; `None` for NULL.
    fn owned(r: AXUIElementRef) -> Option<Ax> {
        (!r.is_null()).then(|| Ax(r))
    }
    fn app(pid: i32) -> Option<Ax> {
        Ax::owned(unsafe { AXUIElementCreateApplication(pid) })
    }
    pub fn as_ptr(&self) -> AXUIElementRef {
        self.0
    }
    fn copy(&self, name: &str) -> Option<CFType> {
        let attr = CFString::new(name);
        let mut value: CFTypeRef = std::ptr::null();
        let err = unsafe { AXUIElementCopyAttributeValue(self.0, attr.as_concrete_TypeRef(), &mut value) };
        (err == 0 && !value.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(value) })
    }
    fn elem(&self, name: &str) -> Option<Ax> {
        let v = self.copy(name)?;
        (v.type_of() as usize == unsafe { AXUIElementGetTypeID() }).then(|| {
            unsafe { CFRetain(v.as_CFTypeRef()) };
            Ax(v.as_CFTypeRef())
        })
    }
    fn string(&self, name: &str) -> Option<String> {
        self.copy(name)?.downcast::<CFString>().map(|s| s.to_string()).filter(|s| !s.is_empty())
    }
    fn bool(&self, name: &str) -> Option<bool> {
        self.copy(name)?.downcast::<CFBoolean>().map(bool::from)
    }
    fn set_bool(&self, name: &str, on: bool) -> i32 {
        let attr = CFString::new(name);
        let v = if on { CFBoolean::true_value() } else { CFBoolean::false_value() };
        unsafe { AXUIElementSetAttributeValue(self.0, attr.as_concrete_TypeRef(), v.as_CFTypeRef()) }
    }
    fn perform(&self, action: &str) -> i32 {
        let a = CFString::new(action);
        unsafe { AXUIElementPerformAction(self.0, a.as_concrete_TypeRef()) }
    }
    fn value<T: Default>(&self, name: &str, kind: u32) -> Option<T> {
        let v = self.copy(name)?;
        let mut out = T::default();
        unsafe { AXValueGetValue(v.as_CFTypeRef(), kind, &mut out as *mut T as *mut c_void) }.then_some(out)
    }
    fn pid(&self) -> Option<i32> {
        let mut pid = 0;
        (unsafe { AXUIElementGetPid(self.0, &mut pid) } == 0).then_some(pid)
    }
    fn children(&self, name: &str) -> Vec<Ax> {
        let Some(v) = self.copy(name) else { return vec![] };
        let Some(arr) = v.downcast::<CFArray>() else { return vec![] };
        arr.get_all_values()
            .into_iter()
            .filter(|e| !e.is_null() && unsafe { CFGetTypeID(*e) } as usize == unsafe { AXUIElementGetTypeID() })
            .map(|e| {
                unsafe { CFRetain(e) };
                Ax(e)
            })
            .collect()
    }
}

impl Drop for Ax {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) }
    }
}

impl Clone for Ax {
    fn clone(&self) -> Ax {
        unsafe { CFRetain(self.0) };
        Ax(self.0)
    }
}

impl PartialEq for Ax {
    fn eq(&self, other: &Ax) -> bool {
        unsafe { CFEqual(self.0, other.0) != 0 }
    }
}

impl std::fmt::Debug for Ax {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Ax({:p})", self.0)
    }
}

/// A window: its app's pid, and its AX element when Accessibility is granted.
#[derive(Clone, Debug, PartialEq)]
pub struct RawId {
    pub pid: i32,
    pub window: Option<Ax>,
}

fn trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

fn needs_accessibility() -> Error {
    Error::msg("This needs the Accessibility permission: System Settings → Privacy & Security → Accessibility → turn on KnobDeck")
}

// ── Apps and window order ───────────────────────────────────────────────────

fn exe_path(pid: i32) -> Option<String> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) };
    if n <= 0 {
        return None;
    }
    buf.truncate(n as usize);
    String::from_utf8(buf).ok()
}

/// (bundle id or exe name, display name).
fn app_info(pid: i32) -> (String, Option<String>) {
    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid);
    let bundle = app.as_ref().and_then(|a| a.bundleIdentifier()).map(|s| s.to_string());
    let name = app.as_ref().and_then(|a| a.localizedName()).map(|s| s.to_string());
    let exe = || exe_path(pid).and_then(|p| std::path::Path::new(&p).file_name().map(|n| n.to_string_lossy().into_owned()));
    (bundle.or_else(exe).unwrap_or_default(), name)
}

/// Pids owning normal (layer 0) windows, front to back; `on_screen` = visible ones only.
fn window_owner_pids(on_screen: bool) -> Vec<i32> {
    const ON_SCREEN_ONLY: u32 = 1;
    const EXCLUDE_DESKTOP: u32 = 16;
    let raw = unsafe { CGWindowListCopyWindowInfo(EXCLUDE_DESKTOP | if on_screen { ON_SCREEN_ONLY } else { 0 }, 0) };
    if raw.is_null() {
        return vec![];
    }
    let arr: CFArray<CFDictionary<CFString, CFType>> = unsafe { CFArray::wrap_under_create_rule(raw) };
    let (k_pid, k_layer) = (CFString::from_static_string("kCGWindowOwnerPID"), CFString::from_static_string("kCGWindowLayer"));
    let num = |d: &CFDictionary<CFString, CFType>, k: &CFString| d.find(k).and_then(|v| v.downcast::<CFNumber>()).and_then(|n| n.to_i64());
    let mut pids = vec![];
    for d in arr.iter() {
        if num(&d, &k_layer) != Some(0) {
            continue;
        }
        if let Some(pid) = num(&d, &k_pid).map(|p| p as i32) {
            if !pids.contains(&pid) {
                pids.push(pid);
            }
        }
    }
    pids
}

fn standard_windows(pid: i32) -> Vec<Ax> {
    let Some(app) = Ax::app(pid) else { return vec![] };
    app.children("AXWindows").into_iter().filter(|w| w.string("AXSubrole").as_deref() == Some("AXStandardWindow")).collect()
}

fn describe(pid: i32, window: Option<Ax>) -> Window {
    let (app_id, app_name) = app_info(pid);
    let w = window.as_ref();
    let pos: CGPoint = w.and_then(|w| w.value("AXPosition", AX_VALUE_CGPOINT)).unwrap_or_default();
    let size: CGSize = w.and_then(|w| w.value("AXSize", AX_VALUE_CGSIZE)).unwrap_or_default();
    let flag = |name: &str| w.and_then(|w| w.bool(name)).unwrap_or(false);
    Window {
        pid: pid as u32,
        app_id,
        app_name,
        title: w.and_then(|w| w.string("AXTitle")),
        bounds: Rect { x: pos.x, y: pos.y, w: size.width, h: size.height },
        monitor: None,
        state: WindowState { maximized: false, minimized: flag("AXMinimized"), fullscreen: flag("AXFullScreen"), topmost: false },
        desktop_id: None,
        id: WindowId(RawId { pid, window }),
    }
}

/// The app's focused window, else its main one, else its first standard one.
fn front_window_of(pid: i32) -> Option<Ax> {
    let app = Ax::app(pid)?;
    app.elem("AXFocusedWindow").or_else(|| app.elem("AXMainWindow")).or_else(|| standard_windows(pid).into_iter().next())
}

pub fn foreground() -> Option<Window> {
    if trusted() {
        let sys = Ax::owned(unsafe { AXUIElementCreateSystemWide() })?;
        if let Some(pid) = sys.elem("AXFocusedApplication").and_then(|a| a.pid()) {
            return Some(describe(pid, front_window_of(pid)));
        }
    }
    // No Accessibility: the front app only. `frontmostApplication` is current only while the main
    // run loop runs, so prefer the window server's order.
    let pid = window_owner_pids(true).into_iter().next().or_else(|| NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier()))?;
    Some(describe(pid, None))
}

/// Every standard window, frontmost app first (minimized windows included).
pub fn list() -> Result<Vec<Window>> {
    if !trusted() {
        return Err(needs_accessibility());
    }
    let mut pids = window_owner_pids(true);
    for p in window_owner_pids(false) {
        if !pids.contains(&p) {
            pids.push(p);
        }
    }
    let mut out = vec![];
    for pid in pids.into_iter().filter(|p| !crate::window::skipped(*p as u32)) {
        out.extend(standard_windows(pid).into_iter().map(|w| describe(pid, Some(w))));
    }
    Ok(out)
}

fn activate(app: &NSRunningApplication) -> bool {
    if app.isHidden() {
        app.unhide();
    }
    #[allow(deprecated)]
    app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows | NSApplicationActivationOptions::ActivateIgnoringOtherApps)
}

/// Unminimize, raise and make main through Accessibility, then activate the app.
pub fn focus(id: &RawId) -> Result {
    if let Some(w) = &id.window {
        if w.bool("AXMinimized") == Some(true) {
            w.set_bool("AXMinimized", false);
        }
        w.perform("AXRaise");
        w.set_bool("AXMain", true);
        if let Some(app) = Ax::app(id.pid) {
            app.set_bool("AXFrontmost", true);
        }
    }
    let app = NSRunningApplication::runningApplicationWithProcessIdentifier(id.pid).ok_or_else(|| Error::msg("That app is no longer running"))?;
    activate(&app);
    Ok(())
}

/// `app` is a `.app` path, a bundle id or an executable name; `name_hint` matches the display
/// name. `Ok(false)` when no such app is running.
pub fn focus_app(app: &str, name_hint: Option<&str>) -> Result<bool> {
    let target = app.trim_end_matches('/');
    let exe = std::path::Path::new(target).file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let stem = exe.trim_end_matches(".app").to_owned();
    for running in NSWorkspace::sharedWorkspace().runningApplications().iter() {
        let bundle_path = running.bundleURL().and_then(|u| u.path()).map(|p| p.to_string());
        let bundle_id = running.bundleIdentifier().map(|s| s.to_string());
        let exe_name = running.executableURL().and_then(|u| u.lastPathComponent()).map(|s| s.to_string().to_lowercase());
        let name = running.localizedName().map(|s| s.to_string());
        let hit = bundle_path.as_deref().is_some_and(|p| p.eq_ignore_ascii_case(target))
            || bundle_id.as_deref().is_some_and(|b| b.eq_ignore_ascii_case(target))
            || exe_name.as_deref().is_some_and(|e| !stem.is_empty() && e == stem)
            || name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(&stem) || name_hint.is_some_and(|h| n.eq_ignore_ascii_case(h)));
        if hit && !crate::window::skipped(running.processIdentifier() as u32) {
            let pid = running.processIdentifier();
            if let Some(w) = front_window_of(pid) {
                return focus(&RawId { pid, window: Some(w) }).map(|_| true);
            }
            return Ok(activate(&running));
        }
    }
    Ok(false)
}

// ── Watcher ─────────────────────────────────────────────────────────────────

/// Accessibility notifications on the front app that can change its target window, title or state.
const AX_NOTIFICATIONS: [&str; 6] = ["AXFocusedWindowChanged", "AXMainWindowChanged", "AXTitleChanged", "AXWindowMiniaturized", "AXWindowDeminiaturized", "AXWindowResized"];

unsafe extern "C" fn on_ax(_obs: AXObserverRef, _el: AXUIElementRef, _name: CFStringRef, refcon: *mut c_void) {
    let tick = &*(refcon as *const Sender<()>);
    let _ = tick.send(());
}

struct Observer {
    obs: AXObserverRef,
    source: CFRunLoopSourceRef,
}

struct Shared {
    /// Boxed so its address is stable: the AX callback's refcon.
    tick: Box<Sender<()>>,
    observer: Option<Observer>,
    pid: i32,
}

unsafe impl Send for Shared {}

impl Shared {
    /// Point the AX observer at `pid`'s app (KnobDeck's own windows don't matter, nor do apps
    /// without Accessibility).
    fn follow(&mut self, pid: i32) {
        if pid == self.pid {
            return;
        }
        self.unfollow();
        self.pid = pid;
        if pid <= 0 || !trusted() {
            return;
        }
        let Some(app) = Ax::app(pid) else { return };
        let mut obs: AXObserverRef = std::ptr::null();
        if unsafe { AXObserverCreate(pid, on_ax, &mut obs) } != 0 || obs.is_null() {
            return;
        }
        let refcon = &*self.tick as *const Sender<()> as *mut c_void;
        for n in AX_NOTIFICATIONS {
            let name = CFString::new(n);
            unsafe { AXObserverAddNotification(obs, app.as_ptr(), name.as_concrete_TypeRef(), refcon) };
        }
        let source = unsafe { AXObserverGetRunLoopSource(obs) };
        unsafe { core_foundation::runloop::CFRunLoopAddSource(CFRunLoop::get_main().as_concrete_TypeRef(), source, kCFRunLoopDefaultMode) };
        self.observer = Some(Observer { obs, source });
    }

    fn unfollow(&mut self) {
        if let Some(o) = self.observer.take() {
            unsafe {
                core_foundation::runloop::CFRunLoopRemoveSource(CFRunLoop::get_main().as_concrete_TypeRef(), o.source, kCFRunLoopDefaultMode);
                CFRelease(o.obs);
            }
        }
    }
}

fn front_pid() -> i32 {
    NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier()).unwrap_or(0)
}

/// The workspace's app-activation observer plus an AX observer on the front app, both on the
/// main run loop. Dropping it removes both.
pub struct Source {
    shared: Arc<Mutex<Shared>>,
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

// The token is only handed back to the notification center, which is thread-safe.
unsafe impl Send for Source {}
unsafe impl Sync for Source {}

impl Drop for Source {
    fn drop(&mut self) {
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        unsafe { center.removeObserver(self.token.as_ref()) };
        self.shared.lock().unwrap().unfollow();
    }
}

pub fn source(tick: Sender<()>) -> Result<Source> {
    let shared = Arc::new(Mutex::new(Shared { tick: Box::new(tick), observer: None, pid: 0 }));
    shared.lock().unwrap().follow(front_pid());
    let on_activate = shared.clone();
    let block = block2::RcBlock::new(move |_n: NonNull<NSNotification>| {
        let mut s = on_activate.lock().unwrap();
        s.follow(front_pid());
        let _ = s.tick.send(());
    });
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    let token = unsafe { center.addObserverForName_object_queue_usingBlock(Some(objc2_app_kit::NSWorkspaceDidActivateApplicationNotification), None, None, &block) };
    Ok(Source { shared, token })
}
