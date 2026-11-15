//! Windows pointer: `GetCursorPos`, and UI Automation's `ElementFromPoint`.

use crate::pointer::Element;
use crate::{Error, Result};
use windows::Win32::Foundation::POINT;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};

/// Physical pixels on the virtual screen (the space window bounds use).
pub fn position() -> Result<(f64, f64)> {
    let mut p = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
    if unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p) } == 0 {
        return Err(Error::msg("Couldn't read the pointer position"));
    }
    Ok((p.x as f64, p.y as f64))
}

pub fn element_at() -> Result<Option<Element>> {
    let (x, y) = position()?;
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let uia: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.map_err(|e| Error::msg(format!("UI Automation: {}", e.message())))?;
    let Ok(el) = (unsafe { uia.ElementFromPoint(POINT { x: x as i32, y: y as i32 }) }) else { return Ok(None) };
    let role = unsafe { el.CurrentLocalizedControlType() }.map(|s| s.to_string()).unwrap_or_default();
    let name = unsafe { el.CurrentName() }.map(|s| s.to_string()).unwrap_or_default();
    let pid = unsafe { el.CurrentProcessId() }.unwrap_or(0) as u32;
    Ok(Some(Element { role, name, pid }))
}
