//! The target browser window's current tab: its address and title.
//!
//! - macOS: the browser's own AppleScript (Safari, Chrome, Edge, Brave, Arc, Vivaldi, Opera). The
//!   first read asks for the Automation permission for that browser. Firefox has no scripting
//!   dictionary: only the title.
//! - Windows: the address bar through UI Automation (Chromium browsers and Firefox expose it as
//!   an edit field). Chromium shows addresses without `https://`; it is added back.
//! - Linux: only the title (from the window); the address needs the KnobDeck Browser Companion
//!   extension, which the `browser-plugin` plugin talks to.
//!
//! The title is the window title without the browser's name (`Docs — Mozilla Firefox` → `Docs`).

use crate::{Error, Result};

/// The current tab of the target browser window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tab {
    /// The address, where it can be read.
    pub url: Option<String>,
    /// The page title.
    pub title: Option<String>,
    /// The browser's app id (`chrome.exe`, `com.google.Chrome`, `firefox`).
    pub app_id: String,
}

/// Browser app ids (lowercase): Windows exe names, macOS bundle ids, Linux classes / app ids.
const BROWSERS: &[&str] = &[
    // Windows
    "chrome.exe", "msedge.exe", "brave.exe", "firefox.exe", "vivaldi.exe", "opera.exe", "zen.exe", "arc.exe", "librewolf.exe", "waterfox.exe",
    // macOS
    "com.apple.safari", "com.apple.safaritechnologypreview", "com.google.chrome", "com.google.chrome.canary", "com.microsoft.edgemac",
    "com.brave.browser", "company.thebrowser.browser", "com.vivaldi.vivaldi", "com.operasoftware.opera", "org.mozilla.firefox",
    "app.zen-browser.zen", "org.chromium.chromium",
    // Linux
    "firefox", "firefox-esr", "org.mozilla.firefox", "chromium", "chromium-browser", "google-chrome", "brave-browser", "microsoft-edge",
    "vivaldi-stable", "zen", "librewolf", "opera",
];

/// Whether `app_id` is a browser this module knows.
pub fn is_browser(app_id: &str) -> bool {
    BROWSERS.contains(&app_id.to_ascii_lowercase().as_str())
}

/// The current tab of the target window. An error when the target isn't a browser.
pub fn tab() -> Result<Tab> {
    let w = crate::window::target().ok_or_else(|| Error::msg("No window to read from"))?;
    if !is_browser(&w.app_id) {
        return Err(Error::msg("The window in front isn't a browser"));
    }
    let (url, title) = sys::read(&w)?;
    Ok(Tab { url: url.filter(|u| !u.is_empty()), title: title.or_else(|| w.title.as_deref().map(strip_browser_name)).filter(|t| !t.is_empty()), app_id: w.app_id })
}

/// `Docs — Mozilla Firefox` → `Docs`, `Inbox - Google Chrome` → `Inbox`.
pub fn strip_browser_name(title: &str) -> String {
    const NAMES: &[&str] = &["Mozilla Firefox", "Firefox", "Google Chrome", "Chromium", "Microsoft​ Edge", "Microsoft Edge", "Brave", "Vivaldi", "Opera", "Zen Browser", "Zen", "LibreWolf", "Safari", "Arc"];
    for sep in [" — ", " - ", " – "] {
        if let Some((page, browser)) = title.rsplit_once(sep) {
            let b = browser.trim();
            if NAMES.iter().any(|n| b == *n || b.starts_with(&format!("{n} "))) {
                // Edge: "Page and 3 more pages - Profile 1 - Microsoft Edge".
                let page = match page.rsplit_once(" and ") {
                    Some((p, more)) if more.contains(" more page") => p,
                    _ => page,
                };
                return page.trim().to_string();
            }
        }
    }
    title.trim().to_string()
}

/// `example.com/a` → `https://example.com/a` (Chromium hides the scheme in its address bar).
#[allow(dead_code)]
pub(crate) fn with_scheme(v: &str) -> String {
    let v = v.trim();
    if v.is_empty() || v.contains("://") || v.starts_with("about:") || v.starts_with("chrome:") || v.starts_with("edge:") || v.starts_with("file:") {
        v.to_string()
    } else {
        format!("https://{v}")
    }
}

#[cfg(target_os = "macos")]
mod sys {
    use crate::window::Window;
    use crate::{Error, Result};

    /// The AppleScript reading `URL` and title of the front tab, for browsers that have one.
    fn script(bundle: &str) -> Option<String> {
        let b = bundle.to_ascii_lowercase();
        let body = if b.starts_with("com.apple.safari") {
            "set d to front document\nreturn (URL of d) & linefeed & (name of d)"
        } else if ["com.google.chrome", "com.google.chrome.canary", "com.microsoft.edgemac", "com.brave.browser", "company.thebrowser.browser", "com.vivaldi.vivaldi", "com.operasoftware.opera", "org.chromium.chromium"].contains(&b.as_str()) {
            "set t to active tab of front window\nreturn (URL of t) & linefeed & (title of t)"
        } else {
            return None;
        };
        Some(format!("tell application id \"{bundle}\"\n{body}\nend tell"))
    }

    pub fn read(w: &Window) -> Result<(Option<String>, Option<String>)> {
        let Some(script) = script(&w.app_id) else { return Ok((None, None)) };
        let out = std::process::Command::new("/usr/bin/osascript").arg("-e").arg(script).output().map_err(|e| Error::msg(e.to_string()))?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr);
            if msg.contains("-1743") {
                return Err(Error::msg("Allow KnobDeck to control your browser in System Settings ▸ Privacy & Security ▸ Automation"));
            }
            return Err(Error::msg(format!("Browser: {}", msg.trim())));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut lines = text.trim_end_matches('\n').splitn(2, '\n');
        Ok((lines.next().map(String::from), lines.next().map(String::from)))
    }
}

#[cfg(windows)]
mod sys {
    use crate::window::Window;
    use crate::{Error, Result};
    use windows::core::BSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::Accessibility::*;

    fn err(e: windows::core::Error) -> Error {
        Error::msg(format!("UI Automation: {}", e.message()))
    }

    /// The address bar: Firefox's `urlbar-input`, else the first edit field (Chromium's omnibox).
    pub fn read(w: &Window) -> Result<(Option<String>, Option<String>)> {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let uia: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.map_err(err)?;
        let root = unsafe { uia.ElementFromHandle(HWND(w.id.hwnd() as _)) }.map_err(err)?;
        let by_id = unsafe { uia.CreatePropertyCondition(UIA_AutomationIdPropertyId, &VARIANT::from(BSTR::from("urlbar-input"))) }.map_err(err)?;
        let edit = unsafe { uia.CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(UIA_EditControlTypeId.0)) }.map_err(err)?;
        let found = unsafe { root.FindFirst(TreeScope_Descendants, &by_id) }.or_else(|_| unsafe { root.FindFirst(TreeScope_Descendants, &edit) });
        let url = found.ok().and_then(|el| unsafe { el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }.ok()).and_then(|v| unsafe { v.CurrentValue() }.ok()).map(|v| super::with_scheme(&v.to_string()));
        Ok((url, None))
    }
}

#[cfg(target_os = "linux")]
mod sys {
    use crate::window::Window;
    use crate::Result;

    pub fn read(_: &Window) -> Result<(Option<String>, Option<String>)> {
        Ok((None, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_lose_the_browser_name() {
        assert_eq!(strip_browser_name("Docs — Mozilla Firefox"), "Docs");
        assert_eq!(strip_browser_name("Inbox (3) - Gmail - Google Chrome"), "Inbox (3) - Gmail");
        assert_eq!(strip_browser_name("Home - Microsoft Edge"), "Home");
        assert_eq!(strip_browser_name("A - B"), "A - B");
        assert_eq!(strip_browser_name("Salt and pepper - Microsoft Edge"), "Salt and pepper");
        assert_eq!(strip_browser_name("Docs and 2 more pages - Microsoft Edge"), "Docs");
        assert_eq!(with_scheme("example.com/a"), "https://example.com/a");
        assert_eq!(with_scheme("http://x"), "http://x");
        assert!(is_browser("Firefox.exe") && is_browser("com.google.Chrome") && !is_browser("code"));
    }
}
