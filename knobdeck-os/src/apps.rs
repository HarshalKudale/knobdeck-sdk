//! The installed apps, for app pickers and the Launch / Focus App actions.
//!
//! - **Windows:** the AppsFolder virtual folder
//!   (`shell:::{4234d49b-0245-4df3-b780-3893943456e1}`), the Shell's canonical
//!   app catalog: classic Win32 apps, packaged apps, aliases, and UWP entries.
//! - **macOS:** `.app` bundles in the Applications folders; `value` is the
//!   bundle path (launched through `open -a`).
//! - **Linux:** XDG desktop entries (`applications/*.desktop` in the XDG data
//!   dirs, Flatpak exports included); `value` is the `.desktop` path, launched
//!   through its `Exec` line.
//!
//! Pickers use it as `{ label, value }` suggestions; KnobDeck's Launch action also uses
//! `executable` and `working_dir`. The scan can take a while (Windows runs PowerShell), so callers
//! that list often should keep the result.

use serde::Serialize;
#[cfg(target_os = "windows")]
use serde::Deserialize;
#[cfg(target_os = "windows")]
use serde_json::Value;
use std::collections::HashSet;
#[cfg(any(target_os = "windows", target_os = "macos"))]
use std::path::Path;

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct InstalledApp {
    pub label: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    /// Directory to launch the app in ("Start in"). `None` leaves the OS
    /// default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
}

#[cfg(target_os = "windows")]
#[derive(Deserialize)]
struct ShellAppRow {
    #[serde(default)]
    name: String,
    #[serde(default)]
    app_id: String,
    #[serde(default)]
    raw_path: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    working_dir: Option<String>,
}

/// Every installed app, sorted by label. Never fails: an enumeration problem gives an empty list.
pub fn installed() -> Vec<InstalledApp> {
    let mut out = scan_platform_apps();
    out.sort_by(|a, b| {
        a.label
            .to_ascii_lowercase()
            .cmp(&b.label.to_ascii_lowercase())
            .then_with(|| a.value.to_ascii_lowercase().cmp(&b.value.to_ascii_lowercase()))
    });
    out
}

/// Folders holding `.app` bundles, searched one level deep (plus one level
/// of sub-folders such as `/Applications/Utilities`).
#[cfg(target_os = "macos")]
fn app_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = ["/Applications", "/System/Applications", "/System/Library/CoreServices/Applications"]
        .iter()
        .map(Into::into)
        .collect();
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(std::path::PathBuf::from(home).join("Applications"));
    }
    dirs
}

#[cfg(target_os = "macos")]
fn scan_platform_apps() -> Vec<InstalledApp> {
    let mut bundles = Vec::new();
    for dir in app_dirs() {
        collect_app_bundles(&dir, 2, &mut bundles);
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for path in bundles {
        let value = path.to_string_lossy().into_owned();
        if !seen.insert(value.to_ascii_lowercase()) {
            continue;
        }
        let info = plist::Value::from_file(path.join("Contents/Info.plist")).ok();
        let dict = info.as_ref().and_then(|v| v.as_dictionary());
        let get = |k: &str| dict.and_then(|d| d.get(k)).and_then(|v| v.as_string()).map(str::to_owned);
        // Background agents and helpers aren't things a user launches.
        let background = dict.is_some_and(|d| {
            ["LSUIElement", "LSBackgroundOnly"].iter().any(|k| {
                d.get(*k).is_some_and(|v| v.as_boolean() == Some(true) || v.as_string() == Some("1"))
            })
        });
        if background {
            continue;
        }
        let label = get("CFBundleDisplayName")
            .or_else(|| get("CFBundleName"))
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
        if label.is_empty() {
            continue;
        }
        let executable = get("CFBundleExecutable")
            .map(|exe| path.join("Contents/MacOS").join(exe))
            .filter(|p| p.is_file())
            .map(|p| p.to_string_lossy().into_owned());
        out.push(InstalledApp { label, value, executable, working_dir: None });
    }
    out
}

#[cfg(target_os = "macos")]
fn collect_app_bundles(dir: &Path, depth: u32, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_app = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"));
        if is_app {
            out.push(path);
        } else if depth > 1 && path.is_dir() && !path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
            collect_app_bundles(&path, depth - 1, out);
        }
    }
}

#[cfg(target_os = "linux")]
fn scan_platform_apps() -> Vec<InstalledApp> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for e in crate::linux::desktop::desktop_entries() {
        if !e.shown_here() {
            continue;
        }
        let value = e.path.to_string_lossy().into_owned();
        if !seen.insert(value.to_ascii_lowercase()) {
            continue;
        }
        let executable = e
            .argv(&[])
            .into_iter()
            .find(|w| !w.contains('=') && w != "env")
            .and_then(|bin| crate::linux::desktop::in_path(&bin))
            .map(|p| p.to_string_lossy().into_owned());
        out.push(InstalledApp { label: e.name.clone(), value, executable, working_dir: e.working_dir.clone() });
    }
    out
}

#[cfg(target_os = "windows")]
fn scan_platform_apps() -> Vec<InstalledApp> {
    let Ok(rows) = enumerate_shell_appsfolder() else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for row in rows {
        let label = row.name.trim();
        let value = row.value.trim();
        if label.is_empty() || value.is_empty() {
            continue;
        }
        let lower_label = label.to_ascii_lowercase();
        if lower_label.contains("uninstall") || lower_label.contains("read me") || lower_label.contains("readme") {
            continue;
        }
        if Path::new(value).is_dir() {
            continue;
        }
        let key = value.to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        out.push(InstalledApp {
            label: label.to_string(),
            value: launch_value(&row).unwrap_or_else(|| value.to_string()),
            executable: normalize_executable(value),
            working_dir: normalize_working_dir(row.working_dir.as_deref(), value),
        });
    }
    out
}

#[cfg(target_os = "windows")]
fn launch_value(row: &ShellAppRow) -> Option<String> {
    let app_id = row.app_id.trim();
    if !app_id.is_empty() && !Path::new(app_id).is_absolute() {
        return Some(format!("shell:AppsFolder\\{app_id}"));
    }
    let raw_path = row.raw_path.trim();
    if !raw_path.is_empty() && !Path::new(raw_path).is_absolute() {
        return Some(format!("shell:AppsFolder\\{raw_path}"));
    }
    let value = row.value.trim();
    if !value.is_empty() {
        return Some(value.to_string());
    }
    None
}

#[cfg(target_os = "windows")]
fn normalize_executable(value: &str) -> Option<String> {
    let cleaned = clean_path(value)?;
    let path = Path::new(&cleaned);
    if path.is_file() {
        Some(cleaned)
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
fn normalize_working_dir(raw: Option<&str>, value: &str) -> Option<String> {
    raw.and_then(clean_path)
        .or_else(|| {
            let value_path = normalize_executable(value)?;
            let path = Path::new(&value_path);
            if path.is_file() {
                path.parent().map(|p| p.to_string_lossy().into_owned())
            } else {
                None
            }
        })
}

#[cfg(target_os = "windows")]
fn clean_path(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.starts_with("shell:AppsFolder\\") {
        return None;
    }
    let cleaned = trimmed.strip_prefix(r"\\?\").unwrap_or(trimmed);
    let cleaned_path = Path::new(cleaned);
    if cleaned_path.exists() {
        Some(cleaned.to_string())
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
fn enumerate_shell_appsfolder() -> Result<Vec<ShellAppRow>, String> {
    let script = r#"
$shell = New-Object -ComObject Shell.Application
$folder = $shell.Namespace('shell:::{4234d49b-0245-4df3-b780-3893943456e1}')
if ($null -eq $folder) { throw 'AppsFolder namespace unavailable' }
$items = @($folder.Items())
$rows = foreach ($item in $items) {
  $name = [string]$item.Name
  $target = [string]$item.ExtendedProperty('System.Link.TargetParsingPath')
  $appId = [string]$item.ExtendedProperty('System.AppUserModel.ID')
  $rawPath = [string]$item.Path
  $candidate = if (-not [string]::IsNullOrWhiteSpace($target)) { $target } elseif (-not [string]::IsNullOrWhiteSpace($rawPath)) { $rawPath } else { '' }
  $value = if (-not [string]::IsNullOrWhiteSpace($candidate) -and [System.IO.Path]::IsPathRooted($candidate)) {
    $candidate
  } elseif (-not [string]::IsNullOrWhiteSpace($appId)) {
    'shell:AppsFolder\' + $appId
  } elseif (-not [string]::IsNullOrWhiteSpace($rawPath)) {
    'shell:AppsFolder\' + $rawPath
  } else {
    ''
  }
  $workingDir = if ([System.IO.Path]::IsPathRooted($target)) { Split-Path -Parent $target } else { $null }
  if (-not [string]::IsNullOrWhiteSpace($name) -and -not [string]::IsNullOrWhiteSpace($value)) {
    [pscustomobject]@{
      name = $name
      app_id = $appId
      raw_path = $rawPath
      value = $value
      working_dir = $workingDir
    }
  }
}
$rows | ConvertTo-Json -Depth 3 -Compress
"#;

    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW: no console flash on launch
        .output()
        .map_err(|e| format!("failed to enumerate AppsFolder: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("AppsFolder enumeration failed: {stderr}"));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    match serde_json::from_str::<Value>(trimmed)
        .map_err(|e| format!("failed to parse AppsFolder JSON: {e}"))?
    {
        Value::Array(items) => items
            .into_iter()
            .map(serde_json::from_value)
            .collect::<Result<Vec<ShellAppRow>, _>>()
            .map_err(|e| format!("failed to decode AppsFolder rows: {e}")),
        Value::Object(_) => serde_json::from_value::<ShellAppRow>(
            serde_json::from_str(trimmed).map_err(|e| format!("failed to parse AppsFolder row: {e}"))?
        )
        .map(|row| vec![row])
        .map_err(|e| format!("failed to decode AppsFolder row: {e}")),
        _ => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps_use_label_and_value_shape() {
        let apps = installed();
        for app in &apps {
            assert!(!app.label.trim().is_empty(), "app label should not be empty");
            assert!(!app.value.trim().is_empty(), "app value should not be empty");
        }
    }

    #[test]
    fn list_has_no_duplicate_values() {
        let apps = installed();
        let mut seen = HashSet::new();
        for a in &apps {
            assert!(
                seen.insert(a.value.to_ascii_lowercase()),
                "duplicate value: {}",
                a.value
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_lists_system_apps_as_bundle_paths() {
        let apps = installed();
        let calc = apps.iter().find(|a| a.value.ends_with("/Calculator.app"));
        let calc = calc.expect("Calculator.app should be found in /System/Applications");
        assert_eq!(calc.label, "Calculator");
        assert!(calc.executable.as_deref().is_some_and(|e| e.ends_with("/Contents/MacOS/Calculator")));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn shell_apps_have_an_existing_working_dir_when_set() {
        let apps = installed();
        for app in apps.iter() {
            if let Some(wd) = &app.working_dir {
                assert!(
                    Path::new(wd).is_dir(),
                    "{}: working_dir {wd:?} is not a real directory",
                    app.label
                );
            }
        }
    }
}
