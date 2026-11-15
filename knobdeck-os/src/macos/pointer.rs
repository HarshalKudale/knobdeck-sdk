//! macOS pointer: the current event's location, and Accessibility's hit test.

use super::window::{needs_accessibility, trusted, Ax};
use crate::pointer::Element;
use crate::{Error, Result};
use std::ffi::c_void;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreate(source: *const c_void) -> *const c_void;
    fn CGEventGetLocation(event: *const c_void) -> CGPoint;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> *const c_void;
    fn AXUIElementCopyElementAtPosition(app: *const c_void, x: f32, y: f32, element: *mut *const c_void) -> i32;
}

/// Global position in points, top-left origin (CoreGraphics' space, like window bounds).
pub fn position() -> Result<(f64, f64)> {
    unsafe {
        let ev = CGEventCreate(std::ptr::null());
        if ev.is_null() {
            return Err(Error::msg("Couldn't read the pointer position"));
        }
        let p = CGEventGetLocation(ev);
        CFRelease(ev);
        Ok((p.x, p.y))
    }
}

pub fn element_at() -> Result<Option<Element>> {
    if !trusted() {
        return Err(needs_accessibility());
    }
    let (x, y) = position()?;
    let Some(system) = Ax::owned(unsafe { AXUIElementCreateSystemWide() }) else { return Err(needs_accessibility()) };
    let mut out: *const c_void = std::ptr::null();
    let err = unsafe { AXUIElementCopyElementAtPosition(system.as_ptr(), x as f32, y as f32, &mut out) };
    if err != 0 || out.is_null() {
        return Ok(None);
    }
    let Some(el) = Ax::owned(out) else { return Ok(None) };
    let name = ["AXTitle", "AXDescription", "AXValue", "AXHelp"].iter().find_map(|a| el.string(a).filter(|s| !s.is_empty())).unwrap_or_default();
    Ok(Some(Element { role: el.string("AXRole").unwrap_or_default(), name, pid: el.pid().unwrap_or(0) as u32 }))
}
