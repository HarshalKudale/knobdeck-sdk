//! macOS focus and selection through Accessibility: the target app's `AXFocusedUIElement`, its
//! role and `AXSelectedText`, and Edit ▸ Copy for apps that don't expose a selection.

use super::window::{needs_accessibility, trusted, Ax};
use crate::focus::Element;
use crate::{Error, Result};
use core_foundation::base::TCFType;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use std::ffi::c_void;
use std::time::{Duration, Instant};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementIsAttributeSettable(el: *const c_void, attr: CFStringRef, settable: *mut u8) -> i32;
}

const TEXT_ROLES: [&str; 4] = ["AXTextField", "AXTextArea", "AXComboBox", "AXSearchField"];

fn settable(el: &Ax, attr: &str) -> bool {
    let name = CFString::new(attr);
    let mut out = 0u8;
    unsafe { AXUIElementIsAttributeSettable(el.as_ptr(), name.as_concrete_TypeRef(), &mut out) == 0 && out != 0 }
}

/// The target app's AX element and pid.
fn target_app() -> Result<(Ax, u32)> {
    if !trusted() {
        return Err(needs_accessibility());
    }
    let w = crate::window::target().ok_or_else(|| Error::msg("No window to read from"))?;
    let pid = w.id.app_pid();
    Ok((Ax::app(pid).ok_or_else(needs_accessibility)?, pid as u32))
}

/// The app's focused element. Chromium and Electron build their accessibility tree only when
/// asked (`AXManualAccessibility`), so ask once when they give none.
fn focused(app: &Ax) -> Option<Ax> {
    app.elem("AXFocusedUIElement").or_else(|| {
        (app.set_bool("AXManualAccessibility", true) == 0).then(|| app.elem("AXFocusedUIElement")).flatten()
    })
}

pub fn element() -> Result<Option<Element>> {
    let (app, pid) = target_app()?;
    let Some(el) = focused(&app) else { return Ok(None) };
    let role = el.string("AXRole").unwrap_or_default();
    let subrole = el.string("AXSubrole").unwrap_or_default();
    Ok(Some(Element {
        password: subrole == "AXSecureTextField" || role == "AXSecureTextField",
        editable: TEXT_ROLES.contains(&role.as_str()) || settable(&el, "AXSelectedTextRange"),
        role,
        pid,
    }))
}

pub fn selected_text() -> Result<Option<String>> {
    let (app, _) = target_app()?;
    let el = focused(&app).ok_or_else(crate::not_exposed)?;
    match el.copy("AXSelectedText") {
        Some(v) => Ok(v.downcast::<CFString>().map(|s| s.to_string()).filter(|s| !s.is_empty())),
        None => Err(crate::not_exposed()),
    }
}

/// The front app's Edit ▸ Copy item: the menu item bound to ⌘C with no other modifier.
fn copy_item(app: &Ax) -> Option<Ax> {
    let bar = app.elem("AXMenuBar")?;
    for top in bar.children("AXChildren") {
        for menu in top.children("AXChildren") {
            for item in menu.children("AXChildren") {
                let cmd = item.string("AXMenuItemCmdChar");
                let mods = item.copy("AXMenuItemCmdModifiers").and_then(|v| v.downcast::<CFNumber>()).and_then(|n| n.to_i64());
                if cmd.as_deref() == Some("C") && mods == Some(0) && item.bool("AXEnabled") != Some(false) {
                    return Some(item);
                }
            }
        }
    }
    None
}

/// Press Edit ▸ Copy (else send ⌘C), read the clipboard's text, put the previous clipboard back.
#[cfg(feature = "selection")]
pub fn text_via_copy(send_copy_keys: impl FnOnce() -> Result) -> Result<Option<String>> {
    use super::clipboard;
    let (app, _) = target_app()?;
    let saved = clipboard::snapshot();
    let before = clipboard::change_count();
    match copy_item(&app) {
        Some(item) if item.perform("AXPress") == 0 => {}
        // No Copy menu item (or it refused): the app's ⌘C, sent by the caller.
        _ => send_copy_keys()?,
    }
    let deadline = Instant::now() + Duration::from_millis(700);
    while clipboard::change_count() == before && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if clipboard::change_count() == before {
        return Ok(None); // nothing was copied: no selection
    }
    let text = clipboard::text();
    clipboard::restore(&saved);
    text
}
