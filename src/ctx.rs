//! Handler contexts. `Ctx` is passed to every action handler; the others are
//! passed to the narrower config-time callbacks (initial state, queries, wheels).

use crate::error::{Error, Result};
use crate::log::log;
use crate::runtime::{value_str, Runtime, Visual};
use serde_json::Value;

/// String value of an input ("" when absent or null).
pub(crate) fn input_str(inputs: &Value, key: &str) -> String {
    match &inputs[key] {
        Value::Null => String::new(),
        v => value_str(v),
    }
}

pub(crate) fn input_bool(inputs: &Value, key: &str) -> bool {
    match &inputs[key] {
        Value::Bool(b) => *b,
        Value::String(s) => matches!(s.as_str(), "true" | "1" | "on"),
        Value::Number(n) => n.as_f64().map_or(false, |f| f != 0.0),
        _ => false,
    }
}

pub(crate) fn input_f64(inputs: &Value, key: &str) -> Option<f64> {
    let v = &inputs[key];
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

/// Adds `input_str` / `input_bool` / `input_f64` to a context that has an `inputs` field.
macro_rules! input_readers {
    ($t:ty) => {
        impl $t {
            pub fn input_str(&self, key: &str) -> String {
                $crate::ctx::input_str(&self.inputs, key)
            }
            pub fn input_bool(&self, key: &str) -> bool {
                $crate::ctx::input_bool(&self.inputs, key)
            }
            pub fn input_f64(&self, key: &str) -> Option<f64> {
                $crate::ctx::input_f64(&self.inputs, key)
            }
        }
    };
}
pub(crate) use input_readers;

/// Context for an action handler (press / hold / adjust / wheel child press).
pub struct Ctx {
    pub instance_id: String,
    /// Short action id as registered by the plugin.
    pub action_id: String,
    pub(crate) full_action_id: String,
    pub inputs: Value,
    pub trigger: String,
    /// Current state of this instance (as last reported).
    pub state: Option<String>,
    /// Set only for a `wheel` child invoke: the id the plugin gave the child in `on_open`.
    pub child_id: Option<String>,
    /// Slider absolute target (already clamped by the host), when driven by a drag.
    pub value: Option<f64>,
    /// Slider relative nudge, when driven by a +/- stepper.
    pub delta: Option<f64>,
    pub(crate) rt: Runtime,
    pub(crate) context: std::sync::Arc<crate::context::Context>,
}

input_readers!(Ctx);

impl Ctx {
    pub fn log(&self, msg: impl AsRef<str>) {
        log(msg)
    }
    /// The runtime handle (clone it to move into your own threads).
    pub fn runtime(&self) -> &Runtime {
        &self.rt
    }
    /// Report new state for THIS instance only.
    pub fn set_state(&self, value: impl ToString) {
        self.rt.set_state(&self.instance_id, value);
    }
    /// Fan-out: report state for ALL instances of this action.
    pub fn fan_out(&self, value: impl ToString) {
        self.rt.send(knobdeck_protocol::PluginMsg::State(knobdeck_protocol::StateUpdate {
            target: knobdeck_protocol::Target::action(self.full_action_id.clone()),
            state: Some(value.to_string()),
            ..Default::default()
        }));
    }
    /// Push ephemeral visuals (any subset of state/label/icon) for THIS instance.
    pub fn set_visual(&self, patch: Visual) {
        self.rt.set_visual(&self.instance_id, patch);
    }
    pub fn set_label(&self, label: impl ToString) {
        self.set_visual(Visual::new().label(label));
    }
    pub fn set_icon(&self, icon: impl ToString) {
        self.set_visual(Visual::new().icon(icon));
    }
    /// Fan-out visuals to ALL instances of this action.
    pub fn fan_out_visual(&self, patch: Visual) {
        self.rt.notify_visual(&self.action_id, patch);
    }
    /// Targeted fan-out: report state for every instance of this action whose
    /// `keys` inputs equal this instance's, e.g. `ctx.fan_out_by(&["device"], v)`:
    /// two sliders on the same device stay in step, one on another device doesn't.
    /// Name only the inputs that pick the target, not ones like a step size.
    pub fn fan_out_by(&self, keys: &[&str], value: impl ToString) {
        self.fan_out_visual_by(keys, Visual::new().state(value));
    }
    /// Targeted fan-out of visuals; see [`Ctx::fan_out_by`].
    pub fn fan_out_visual_by(&self, keys: &[&str], patch: Visual) {
        let filter = Value::Object(crate::runtime::filter_from(&self.inputs, keys));
        self.rt.notify_visual_matching(&self.action_id, &filter, patch);
    }
    /// Report an error for this instance (the handler keeps running).
    pub fn set_error(&self, message: impl ToString) {
        self.rt.set_error(&self.instance_id, message);
    }
    /// Log + report an error and stop the handler: `return ctx.fail("msg");` or `ctx.fail("msg")?;`.
    pub fn fail<T>(&self, message: impl ToString) -> Result<T> {
        let m = message.to_string();
        log(format!("error: {m}"));
        self.set_error(&m);
        Err(Error::Fail(m))
    }
    /// Tell the host setup is needed again; `reason` is shown to the user.
    pub fn mark_setup_required(&self, reason: impl ToString) {
        self.rt.mark_setup_required(reason);
    }
    /// This plugin's persistent data folder (survives updates/reinstalls).
    pub fn data_dir(&self) -> &std::path::Path {
        self.rt.data_dir()
    }
    /// Bring `app`'s frontmost window to the front, in this process ([`crate::os::window::focus_app`]).
    /// `Ok(false)` when it has no open window.
    #[cfg(feature = "os-window")]
    pub fn focus_app(&self, app: &str) -> Result<bool> {
        Ok(crate::os::window::focus_app(app, None)?)
    }
    /// The target window: the frontmost one that isn't KnobDeck's. Read once per press
    /// (prefetched with `.needs(&[Need::Window])`).
    #[cfg(feature = "os-window")]
    pub fn window(&self) -> Option<&crate::os::window::Window> {
        self.context.window()
    }
    /// The target app's focused element (role, editable, password).
    #[cfg(feature = "os-focus")]
    pub fn focus(&self) -> Option<&crate::os::focus::Element> {
        self.context.focus()
    }
    /// The target app's selected text; `None` when nothing is selected, the focus is a password
    /// field, or the app doesn't expose it (see [`Ctx::context_error`]).
    #[cfg(feature = "os-selection")]
    pub fn selection(&self) -> Option<&str> {
        self.context.selection()
    }
    /// The clipboard's text.
    #[cfg(feature = "os-clipboard")]
    pub fn clipboard_text(&self) -> Option<&str> {
        self.context.clip().and_then(|c| c.text.as_deref())
    }
    /// The files on the clipboard (copied in a file manager).
    #[cfg(feature = "os-clipboard")]
    pub fn clipboard_files(&self) -> &[std::path::PathBuf] {
        self.context.clip().map_or(&[], |c| c.files.as_slice())
    }
    /// The files and folders selected in the target file manager (File Explorer, the desktop,
    /// Finder, Dolphin). Empty when nothing is selected or the target isn't a file manager (the
    /// reason is in [`Ctx::context_error`]`(Need::Files)`).
    #[cfg(feature = "os-files")]
    pub fn files(&self) -> &[std::path::PathBuf] {
        self.context.files().unwrap_or(&[])
    }
    /// The folder the target file manager window shows (the selection's folder on Linux).
    #[cfg(feature = "os-files")]
    pub fn folder(&self) -> Option<&std::path::Path> {
        self.context.folder()
    }
    /// The file the target window has open (macOS; elsewhere `None`, with the reason in
    /// [`Ctx::context_error`]).
    #[cfg(feature = "os-document")]
    pub fn document(&self) -> Option<&std::path::Path> {
        self.context.document()
    }
    /// The target browser's current tab (`url` where the OS lets it be read, `title`).
    #[cfg(feature = "os-browser")]
    pub fn browser_tab(&self) -> Option<&crate::os::browser::Tab> {
        self.context.browser()
    }
    /// The pointer's position when the action ran, in the space of window bounds.
    #[cfg(feature = "os-pointer")]
    pub fn pointer(&self) -> Option<(f64, f64)> {
        self.context.pointer()
    }
    /// Replace the placeholders in `text` (`{selection}`, `{files}`, `{file}`, `{clipboard}`,
    /// `{url}`, `{document}`, `{app}`, `{title}`,
    /// `{date}`, `{date:%d %B}`, `{time}`, `{input:Prompt}`, `{{` / `}}` for braces), reading only
    /// what it uses. A value that can't be read is empty. Other text in braces is left alone.
    /// `{input:…}` shows the OS's input box; if the user cancels it, this returns an error that
    /// stops the handler quietly (`?` it).
    #[cfg(feature = "placeholders")]
    pub fn expand(&self, text: &str) -> Result<String> {
        self.expand_as(text, crate::os::placeholders::Escape::Raw)
    }
    /// [`Ctx::expand`], escaping values for a URL or a shell command.
    #[cfg(feature = "placeholders")]
    pub fn expand_as(&self, text: &str, escape: crate::os::placeholders::Escape) -> Result<String> {
        use crate::os::placeholders::{expand, Name};
        let c = &self.context;
        expand(text, escape, |name| {
            Ok(match name {
                Name::Selection => c.selection().unwrap_or_default().to_string(),
                Name::Files | Name::File => {
                    c.files().unwrap_or_default().iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>().join("\n")
                }
                Name::Clipboard => c.clip().and_then(|c| c.text.clone()).unwrap_or_default(),
                Name::App => c.window().map(|w| w.app_name.clone().unwrap_or_else(|| w.app_id.clone())).unwrap_or_default(),
                Name::Title => c.window().and_then(|w| w.title.clone()).unwrap_or_default(),
                Name::Url => c.browser().and_then(|t| t.url.clone()).unwrap_or_default(),
                Name::Document => c.document().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
                Name::Date | Name::Time | Name::Input => String::new(), // filled in by the parser
            })
        })
        .map_err(|e| {
            // A cancelled input box: stop without reporting an error.
            if knobdeck_os::input_box::is_cancelled(&e) {
                Error::Fail(e.message().into())
            } else {
                Error::from(e)
            }
        })
    }
    /// Why a context reader returned `None` ("This app doesn't expose its selected text",
    /// "Reading selection took longer than 300 ms"), if it failed.
    pub fn context_error(&self, need: crate::Need) -> Option<String> {
        self.context.error(need.as_str())
    }
    /// Ask KnobDeck to press a key combination. Last resort: see the keystroke rule in the docs.
    pub fn send_keys(&self, keys: &str) -> Result {
        crate::host::send_keys(keys)
    }
    /// Show the user a short message ("Copied"). Goes to the plugin log on an older KnobDeck.
    pub fn toast(&self, message: impl AsRef<str>) -> Result {
        crate::host::toast(message.as_ref())
    }
    /// KnobDeck's OS, version and dial state (needs a KnobDeck with `app_state`).
    pub fn app_state(&self) -> Result<crate::protocol::AppState> {
        crate::host::app_state()
    }
    /// The installed apps (scanned once per process; [`QueryCtx::installed_apps`] rescans).
    #[cfg(feature = "os-apps")]
    pub fn installed_apps(&self) -> Vec<crate::os::apps::InstalledApp> {
        crate::os::apps::installed()
    }
}

/// Context handed to an action's `initial` callback.
pub struct InitialCtx {
    pub instance_id: String,
    pub action_id: String,
    pub inputs: Value,
}

input_readers!(InitialCtx);

/// Context for a config-time query provider (side-effect free).
pub struct QueryCtx {
    pub inputs: Value,
}

input_readers!(QueryCtx);

impl QueryCtx {
    pub fn log(&self, msg: impl AsRef<str>) {
        log(msg)
    }
    /// Report a query failure to the config UI.
    pub fn fail<T>(&self, message: impl ToString) -> Result<T> {
        let m = message.to_string();
        log(format!("query failed: {m}"));
        Err(Error::Fail(m))
    }
    /// The installed apps, scanned again (a picker opening is when a new install should show).
    #[cfg(feature = "os-apps")]
    pub fn installed_apps(&self) -> Vec<crate::os::apps::InstalledApp> {
        crate::os::apps::rescan()
    }
}

/// Context for a `wheel` action's `on_open` (builds the dynamic wheel contents).
pub struct OpenCtx {
    pub inputs: Value,
}

input_readers!(OpenCtx);

impl OpenCtx {
    pub fn log(&self, msg: impl AsRef<str>) {
        log(msg)
    }
    /// The files selected in the target file manager, read now (a wheel that fits its slices to
    /// the selection). An error when the target isn't a file manager.
    #[cfg(feature = "os-files")]
    pub fn files(&self) -> Result<Vec<std::path::PathBuf>> {
        Ok(crate::os::files::selected()?)
    }
    pub fn fail<T>(&self, message: impl ToString) -> Result<T> {
        let m = message.to_string();
        log(format!("wheel open failed: {m}"));
        Err(Error::Fail(m))
    }
}

/// A subscribed host event delivered to `Plugin::on_event`.
pub struct Event {
    pub name: String,
    pub payload: Value,
}
