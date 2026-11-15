//! Windows: the selection and folder of a File Explorer window or the desktop, through the
//! Shell's COM objects. Explorer runs out of process, so the calls go through COM proxies.
//!
//! Explorer's tabs (Windows 11) share one top-level window; each tab is a separate shell window
//! in `IShellWindows`, and only the active tab's view window is visible.

use crate::{Error, Result};
use std::path::PathBuf;
use windows::core::{Interface, PWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, IServiceProvider, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::*;

fn err(e: windows::core::Error) -> Error {
    Error::msg(format!("File Explorer: {}", e.message()))
}

fn class_name(hwnd: isize) -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;
    let mut class = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd as _, class.as_mut_ptr(), class.len() as i32) }.max(0) as usize;
    String::from_utf16_lossy(&class[..n])
}

fn visible(hwnd: HWND) -> bool {
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd.0 as _) != 0 }
}

/// The folder view of the target window: an Explorer window's active tab, or the desktop.
fn folder_view() -> Result<IFolderView> {
    // The foreground may be the desktop (Progman / WorkerW), which `target()` skips.
    let fg = crate::window::foreground().filter(|w| !crate::window::skipped(w.pid));
    let desktop = fg.as_ref().is_some_and(|w| matches!(class_name(w.id.hwnd()).as_str(), "Progman" | "WorkerW"));
    let target = if desktop { None } else { crate::window::target() };
    if !desktop && !target.as_ref().is_some_and(|w| w.app_id.eq_ignore_ascii_case("explorer.exe")) {
        return Err(crate::files::not_a_file_manager());
    }

    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let windows: IShellWindows = unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL) }.map_err(err)?;

    let view_of = |disp: windows::Win32::System::Com::IDispatch| -> Result<(IFolderView, HWND)> {
        let sp: IServiceProvider = disp.cast().map_err(err)?;
        let browser: IShellBrowser = unsafe { sp.QueryService(&SID_STopLevelBrowser) }.map_err(err)?;
        let view = unsafe { browser.QueryActiveShellView() }.map_err(err)?;
        let hwnd = unsafe { view.GetWindow() }.unwrap_or_default();
        Ok((view.cast().map_err(err)?, hwnd))
    };

    if desktop {
        let none = VARIANT::default();
        let mut hwnd = 0i32;
        let disp = unsafe { windows.FindWindowSW(&none, &none, SWC_DESKTOP, &mut hwnd, SWFO_NEEDDISPATCH) }.map_err(err)?;
        return view_of(disp).map(|(v, _)| v);
    }

    let target = target.expect("checked above").id.hwnd();
    let count = unsafe { windows.Count() }.map_err(err)?;
    let mut fallback = None;
    for i in 0..count {
        let Ok(disp) = (unsafe { windows.Item(&VARIANT::from(i)) }) else { continue };
        let Ok(app) = disp.cast::<IWebBrowserApp>() else { continue };
        if unsafe { app.HWND() }.map(|h| h.0) != Ok(target) {
            continue;
        }
        let Ok((view, view_hwnd)) = view_of(disp) else { continue };
        if visible(view_hwnd) {
            return Ok(view);
        }
        fallback.get_or_insert(view);
    }
    fallback.ok_or_else(|| Error::msg("File Explorer didn't report this window"))
}

fn take(p: PWSTR) -> String {
    let s = unsafe { p.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(p.0 as _)) };
    s
}

pub fn selected() -> Result<Vec<PathBuf>> {
    let view = folder_view()?;
    // Nothing selected: Items fails or returns an empty array.
    let Ok(items) = (unsafe { view.Items::<IShellItemArray>(SVGIO_SELECTION) }) else { return Ok(vec![]) };
    let n = unsafe { items.GetCount() }.unwrap_or(0) as usize;
    let mut out = Vec::with_capacity(n.min(crate::files::MAX_FILES));
    for i in 0..n.min(crate::files::MAX_FILES) {
        let Ok(item) = (unsafe { items.GetItemAt(i as u32) }) else { continue };
        // Virtual items (This PC, a phone, a zip's inside) have no file-system path.
        if let Ok(p) = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) } {
            out.push(PathBuf::from(take(p)));
        }
    }
    Ok(out)
}

pub fn current_folder() -> Result<Option<PathBuf>> {
    let view = folder_view()?;
    let Ok(folder) = (unsafe { view.GetFolder::<IPersistFolder2>() }) else { return Ok(None) };
    let pidl = unsafe { folder.GetCurFolder() }.map_err(err)?;
    let name = unsafe { SHGetNameFromIDList(pidl, SIGDN_FILESYSPATH) };
    unsafe { CoTaskMemFree(Some(pidl as _)) };
    Ok(name.ok().map(take).filter(|s| !s.is_empty()).map(PathBuf::from))
}
