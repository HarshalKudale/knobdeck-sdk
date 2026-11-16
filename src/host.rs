//! Host calls: what only KnobDeck itself can do, asked for over the protocol (`host_call`).
//!
//! `send_keys`: keystrokes must come from the app (on Linux, from KnobDeck's uinput device).
//! `toast` and `app_state` need a host that lists the matching feature in `hello`; on an older one
//! `toast` writes to the plugin log and `app_state` returns an error. Focusing apps and listing
//! installed apps happen in the plugin's own process through [`crate::os`].

use crate::error::{Error, Result};
use crate::log::log;
use crate::runtime::global;
use knobdeck_protocol::{features, host_calls, AppState, Notify};
use serde_json::{json, Value};

fn call(method: &str, params: Value) -> Result<Value> {
    global()
        .ok_or_else(|| Error::msg("KnobDeck host bridge is not connected"))?
        .host_call(method, params)
}

/// Ask the host to press a key combination (e.g. `"ctrl+shift+m"`).
pub fn send_keys(keys: &str) -> Result {
    let keys = keys.trim();
    if keys.is_empty() {
        return Err(Error::msg("send_keys requires a non-empty key string"));
    }
    log(format!("host.send_keys({keys})"));
    call(host_calls::SEND_KEYS, json!({"keys": keys})).map(|_| ())
}

/// Show the user a short message from this plugin ("Copied", "Recording started"). On a KnobDeck
/// without the `notify` feature the message goes to the plugin log instead, so calling it is
/// always safe.
pub fn toast(message: &str) -> Result {
    notify(message, None)
}

/// Like [`toast`], styled as an error.
pub fn toast_error(message: &str) -> Result {
    notify(message, Some("error"))
}

fn notify(message: &str, level: Option<&str>) -> Result {
    let rt = global().ok_or_else(|| Error::msg("KnobDeck host bridge is not connected"))?;
    if !rt.has_feature(features::NOTIFY) {
        log(format!("toast ({}): {message}", level.unwrap_or("info")));
        return Ok(());
    }
    let params = Notify { message: message.to_string(), level: level.map(str::to_string) };
    rt.host_call(host_calls::NOTIFY, serde_json::to_value(params).expect("serializes")).map(|_| ())
}

/// KnobDeck's OS, version and whether the dial is open. Errors on a host without `app_state`.
pub fn app_state() -> Result<AppState> {
    let rt = global().ok_or_else(|| Error::msg("KnobDeck host bridge is not connected"))?;
    if !rt.has_feature(features::APP_STATE) {
        return Err(Error::msg("this KnobDeck is too old for app_state; update KnobDeck"));
    }
    let v = rt.host_call(host_calls::APP_STATE, json!({}))?;
    serde_json::from_value(v).map_err(|e| Error::msg(format!("app_state: unexpected answer ({e})")))
}
