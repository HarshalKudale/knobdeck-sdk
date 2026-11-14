//! Host calls: what only KnobDeck itself can do, asked for over the protocol (`host_call`).
//!
//! The one left is `send_keys`: keystrokes must come from the app (on Linux, from KnobDeck's
//! uinput device). Focusing apps and listing installed apps happen in the plugin's own process
//! through [`crate::os`].

use crate::error::{Error, Result};
use crate::log::log;
use crate::runtime::global;
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
    call("send_keys", json!({"keys": keys})).map(|_| ())
}
