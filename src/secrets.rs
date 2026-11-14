//! Secrets (sign-in tokens, passwords, API keys) kept in the OS credential store (feature `secrets`,
//! also enabled by `oauth`): the macOS Keychain, the Windows Credential Manager, or the Linux Secret
//! Service (GNOME Keyring, KWallet).
//!
//! ```ignore
//! // setup program
//! knobdeck_sdk::secrets::set("api-token", &token)?;
//! // plugin
//! let token = knobdeck_sdk::secrets::get("api-token");
//! ```
//!
//! **Only the plugin binary uses the credential store.** A setup program's [`set`] leaves the value in
//! a hand-off file named `name` in the plugin data folder (readable by the user only), and the plugin
//! moves it into the store the next time it reads it. On macOS a keychain item trusts only the binary
//! that created it, so the plugin reads it without a prompt and another program (setup included)
//! would make the user approve access. Windows and Linux stores are per user, not per program: they
//! keep secrets out of plain files and encrypted at rest, but any program running as the same user
//! can ask for them.
//!
//! Where there is no store (a Linux session without a Secret Service) the plugin keeps the hand-off
//! file, readable by the user only, and logs why.
//!
//! `name` doubles as the hand-off file's name, so a secret that older builds kept in a plain file
//! (`oauth-token.json`) moves into the store by being read under that file name.

use crate::{data_dir, data_file, log, write_private, Result};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Set by [`crate::run_setup`]: setup programs never touch the credential store.
static IN_SETUP: AtomicBool = AtomicBool::new(false);
/// Values this process already read or wrote (`None` = deleted), so the store is asked once.
static CACHE: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

pub(crate) fn mark_setup_process() {
    IN_SETUP.store(true, Ordering::Relaxed);
}

fn uses_store() -> bool {
    !IN_SETUP.load(Ordering::Relaxed) && !cfg!(test)
}

fn cached(name: &str) -> Option<Option<String>> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(name).cloned()
}

fn remember(name: &str, value: Option<&str>) {
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(name.into(), value.map(String::from));
}

/// The secret saved under `name`, if any.
pub fn get(name: &str) -> Option<String> {
    let path = data_file(name);
    if let Ok(text) = std::fs::read_to_string(&path) {
        if !uses_store() {
            return Some(text).filter(|t| !t.is_empty());
        }
        // A hand-off from setup (empty = setup removed the secret): move it into the store.
        if text.is_empty() {
            store_delete(name);
            let _ = std::fs::remove_file(&path);
            remember(name, None);
            return None;
        }
        store_put(name, &text);
        return Some(text);
    }
    if let Some(v) = cached(name) {
        return v;
    }
    if !uses_store() {
        return None;
    }
    let v = entry(name).ok().and_then(|e| e.get_password().ok());
    remember(name, v.as_deref());
    v
}

/// Save `value` under `name` (an empty value removes it, like [`delete`]).
pub fn set(name: &str, value: &str) -> Result<()> {
    if uses_store() {
        if value.is_empty() {
            delete(name);
        } else {
            store_put(name, value);
        }
        return Ok(());
    }
    write_private(name, value)?;
    remember(name, Some(value).filter(|v| !v.is_empty()));
    Ok(())
}

/// Remove the secret saved under `name`.
pub fn delete(name: &str) {
    if uses_store() {
        store_delete(name);
        let _ = std::fs::remove_file(data_file(name));
    } else if let Err(e) = write_private(name, "") {
        // The empty hand-off tells the plugin to drop the stored value.
        log(e.message());
    }
    remember(name, None);
}

/// The environment variable `key` when set (development, or a setup program that set it before
/// testing a connection), else the secret stored under `key`, else `""`.
pub fn env_or_stored(key: &str) -> String {
    match std::env::var(key) {
        Ok(v) if !v.is_empty() => v,
        _ => get(key).unwrap_or_default(),
    }
}

/// For a plugin's start-up, after `load_env()`: setup programs built with SDK 0.6 or older wrote
/// passwords and tokens to `.env`. Each of `keys` found there moves into the credential store under
/// its own name (read it back with [`env_or_stored`]), and `.env` is rewritten without them.
pub fn migrate_env(keys: &[&str]) {
    let Ok(text) = std::fs::read_to_string(data_file(".env")) else { return };
    let (found, kept): (Vec<_>, Vec<_>) = crate::parse_env(&text).into_iter().partition(|(k, _)| keys.contains(&k.as_str()));
    if found.is_empty() {
        return;
    }
    for (k, v) in &found {
        if let Err(e) = set(k, v) {
            return log(format!("could not move {k} out of .env: {}", e.message()));
        }
    }
    let kept: Vec<(&str, &str)> = kept.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    match crate::save_env(&kept) {
        Ok(()) => {
            for (k, _) in &found {
                // load_env put it in the environment, where it would shadow the stored value.
                std::env::remove_var(k);
            }
            log(format!("moved {} from .env into the OS credential store", found.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", ")));
        }
        Err(e) => log(format!("could not rewrite .env: {}", e.message())),
    }
}

/// One credential-store item per plugin and name. The service names the plugin by its id (the data
/// folder's name), so two plugins never share an item.
fn entry(name: &str) -> keyring::Result<keyring::Entry> {
    let id = data_dir()
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty() && n != ".")
        .or_else(|| std::env::current_exe().ok()?.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "plugin".into());
    keyring::Entry::new(&format!("KnobDeck plugin {id}"), name)
}

/// Save into the store and drop the hand-off file; without a store keep the file (user-only).
fn store_put(name: &str, value: &str) {
    remember(name, Some(value));
    match entry(name).and_then(|e| e.set_password(value)) {
        Ok(()) => {
            let _ = std::fs::remove_file(data_file(name));
        }
        Err(e) => {
            log(format!("OS credential store unavailable ({e}); keeping {name} in the plugin data folder, readable by this user only"));
            if let Err(e) = write_private(name, value) {
                log(e.message());
            }
        }
    }
}

fn store_delete(name: &str) {
    if let Ok(e) = entry(name) {
        match e.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => log(format!("could not remove {name} from the OS credential store: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests run like a setup program: hand-off files only, owner-only on Unix.
    #[test]
    fn handoff_round_trip() {
        let _guard = crate::DATA_DIR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("kd-secrets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("KNOBDECK_PLUGIN_DATA", &dir);

        set("kd-test-secret", "s3cret").unwrap();
        assert_eq!(get("kd-test-secret").as_deref(), Some("s3cret"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.join("kd-test-secret")).unwrap().permissions().mode() & 0o777, 0o600);
        }
        delete("kd-test-secret");
        assert_eq!(get("kd-test-secret"), None);
        // The empty file stays as the "removed" hand-off for the plugin.
        assert_eq!(std::fs::read_to_string(dir.join("kd-test-secret")).unwrap(), "");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn migrate_env_moves_only_the_named_keys() {
        let _guard = crate::DATA_DIR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("kd-secrets-env-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("KNOBDECK_PLUGIN_DATA", &dir);
        std::fs::write(dir.join(".env"), "KD_MIG_HOST=localhost\nKD_MIG_PASSWORD=\"p=w\"\n").unwrap();
        crate::load_env();

        migrate_env(&["KD_MIG_PASSWORD"]);
        assert_eq!(std::fs::read_to_string(dir.join(".env")).unwrap(), "KD_MIG_HOST=localhost\n");
        assert!(std::env::var_os("KD_MIG_PASSWORD").is_none());
        assert_eq!(env_or_stored("KD_MIG_PASSWORD"), "p=w");
        // Nothing left to move: .env stays as it is.
        migrate_env(&["KD_MIG_PASSWORD"]);
        assert_eq!(std::fs::read_to_string(dir.join(".env")).unwrap(), "KD_MIG_HOST=localhost\n");
        std::env::remove_var("KD_MIG_HOST");
        let _ = std::fs::remove_dir_all(dir);
    }
}
