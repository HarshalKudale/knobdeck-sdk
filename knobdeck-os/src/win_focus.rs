//! Windows focus and selection through UI Automation: the focused element, its control type and
//! `IsPassword`, and `TextPattern.GetSelection()`.
// The `windows` crate names UIA's constants as the SDK does (`UIA_EditControlTypeId`).
#![allow(non_upper_case_globals)]

use crate::focus::Element;
use crate::{Error, Result};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::UI::Accessibility::*;

fn err(e: windows::core::Error) -> Error {
    Error::msg(format!("UI Automation: {}", e.message()))
}

fn automation() -> Result<IUIAutomation> {
    // Each calling thread joins the multithreaded apartment; a thread that is already in an
    // apartment (RPC_E_CHANGED_MODE) still works.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.map_err(err)
}

/// The focused element, unless it is KnobDeck's own (the dial still holds the focus).
fn focused(uia: &IUIAutomation) -> Result<Option<IUIAutomationElement>> {
    let el = match unsafe { uia.GetFocusedElement() } {
        Ok(el) => el,
        Err(_) => return Ok(None),
    };
    let pid = unsafe { el.CurrentProcessId() }.unwrap_or(0) as u32;
    if pid == std::process::id() || crate::window::excluded_pid() == Some(pid) {
        return Err(Error::msg("KnobDeck has the focus"));
    }
    Ok(Some(el))
}

fn control_name(t: UIA_CONTROLTYPE_ID) -> &'static str {
    match t {
        UIA_EditControlTypeId => "edit",
        UIA_DocumentControlTypeId => "document",
        UIA_ComboBoxControlTypeId => "combobox",
        UIA_ButtonControlTypeId => "button",
        UIA_ListItemControlTypeId => "listitem",
        UIA_ListControlTypeId => "list",
        UIA_TreeItemControlTypeId => "treeitem",
        UIA_DataItemControlTypeId => "dataitem",
        UIA_PaneControlTypeId => "pane",
        UIA_WindowControlTypeId => "window",
        UIA_TextControlTypeId => "text",
        UIA_CustomControlTypeId => "custom",
        _ => "other",
    }
}

pub fn element() -> Result<Option<Element>> {
    let uia = automation()?;
    let Some(el) = focused(&uia)? else { return Ok(None) };
    let ty = unsafe { el.CurrentControlType() }.map_err(err)?;
    let password = unsafe { el.CurrentIsPassword() }.map(|b| b.as_bool()).unwrap_or(false);
    let read_only = unsafe { el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
        .and_then(|v| unsafe { v.CurrentIsReadOnly() })
        .map(|b| b.as_bool());
    let editable = match read_only {
        Ok(ro) => !ro && matches!(ty, UIA_EditControlTypeId | UIA_DocumentControlTypeId | UIA_ComboBoxControlTypeId),
        // Rich editors (Word, browsers' content-editable) expose TextPattern without ValuePattern.
        Err(_) => ty == UIA_DocumentControlTypeId || ty == UIA_EditControlTypeId,
    };
    let pid = unsafe { el.CurrentProcessId() }.unwrap_or(0) as u32;
    Ok(Some(Element { role: control_name(ty).into(), editable, password, pid }))
}

pub fn selected_text() -> Result<Option<String>> {
    let uia = automation()?;
    let el = focused(&uia)?.ok_or_else(crate::not_exposed)?;
    let pattern = unsafe { el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) }.map_err(|_| crate::not_exposed())?;
    let ranges = unsafe { pattern.GetSelection() }.map_err(err)?;
    let n = unsafe { ranges.Length() }.unwrap_or(0);
    let mut out = String::new();
    for i in 0..n {
        let range = unsafe { ranges.GetElement(i) }.map_err(err)?;
        let text = unsafe { range.GetText(crate::MAX_TEXT as i32) }.map_err(err)?.to_string();
        if !text.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&text);
        }
    }
    Ok(Some(out).filter(|s| !s.is_empty()))
}
