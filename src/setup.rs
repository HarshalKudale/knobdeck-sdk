//! Standalone setup program support. KnobDeck runs the setup binary once when the
//! user clicks Setup, with the collected fields in `KNOBDECK_SETUP_INPUTS` (JSON).
//! Returning `Ok` → exit 0 → the host marks setup complete; `Err` → exit 1.
//! Save settings under [`crate::data_dir`] (`KNOBDECK_PLUGIN_DATA`) and passwords or tokens with
//! `secrets::set` (feature `secrets`), which hands them to the plugin for the OS credential store.

use crate::ctx::input_readers;
use crate::error::{Error, Result};
use crate::log::log;
use serde_json::{json, Value};

pub struct SetupCtx {
    pub inputs: Value,
}

input_readers!(SetupCtx);

impl SetupCtx {
    pub fn log(&self, msg: impl AsRef<str>) {
        log(msg)
    }
    /// The plugin's persistent data folder (same one the plugin sees).
    pub fn data_dir(&self) -> std::path::PathBuf {
        crate::data_dir()
    }
    /// Print `message` to stderr (shown by the host) and fail setup.
    pub fn fail<T>(&self, message: impl ToString) -> Result<T> {
        let m = message.to_string();
        eprintln!("{m}");
        Err(Error::Fail(m))
    }
}

/// Run a setup program: `fn main() { knobdeck_sdk::run_setup(|ctx| { …; Ok(()) }) }`.
/// A no-op under `KNOBDECK_CODEGEN=1` so the CLI can probe every binary safely.
pub fn run_setup(run: impl FnOnce(&SetupCtx) -> Result) -> ! {
    if std::env::var("KNOBDECK_CODEGEN").as_deref() == Ok("1") {
        std::process::exit(0);
    }
    #[cfg(feature = "secrets")]
    crate::secrets::mark_setup_process();
    let inputs = match std::env::var("KNOBDECK_SETUP_INPUTS") {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|e| {
            eprintln!("setup: warning: could not parse KNOBDECK_SETUP_INPUTS: {e}");
            json!({})
        }),
        Err(_) => json!({}),
    };
    match run(&SetupCtx { inputs }) {
        Ok(()) => {
            log("setup: complete");
            std::process::exit(0)
        }
        Err(Error::Fail(_)) => std::process::exit(1),
        Err(Error::Other(m)) => {
            eprintln!("{m}");
            log(format!("setup error: {m}"));
            std::process::exit(1)
        }
    }
}
