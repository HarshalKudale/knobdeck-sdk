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
    pub fg_process: Option<String>,
    pub fg_title: Option<String>,
    /// Set only for a `wheel` child invoke: the id the plugin gave the child in `on_open`.
    pub child_id: Option<String>,
    /// Slider absolute target (already clamped by the host), when driven by a drag.
    pub value: Option<f64>,
    /// Slider relative nudge, when driven by a +/- stepper.
    pub delta: Option<f64>,
    pub(crate) rt: Runtime,
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
        self.rt.send(serde_json::json!({"type": "state", "action_id": self.full_action_id, "state": value.to_string()}));
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
    pub fn focus_app(&self, target: &str) -> Result<bool> {
        crate::host::focus_app(target)
    }
    pub fn send_keys(&self, keys: &str) -> Result {
        crate::host::send_keys(keys)
    }
    pub fn installed_apps(&self) -> Result<Vec<Value>> {
        crate::host::installed_apps()
    }
}

/// Context handed to an action's `initial` callback.
pub struct InitialCtx {
    pub instance_id: String,
    pub action_id: String,
    pub inputs: Value,
    pub fg_process: Option<String>,
    pub fg_title: Option<String>,
}

input_readers!(InitialCtx);

/// Context for a config-time query provider (side-effect free).
pub struct QueryCtx {
    pub inputs: Value,
    pub fg_process: Option<String>,
    pub fg_title: Option<String>,
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
    pub fn installed_apps(&self) -> Result<Vec<Value>> {
        crate::host::installed_apps()
    }
}

/// Context for a `wheel` action's `on_open` (builds the dynamic wheel contents).
pub struct OpenCtx {
    pub inputs: Value,
    pub fg_process: Option<String>,
    pub fg_title: Option<String>,
}

input_readers!(OpenCtx);

impl OpenCtx {
    pub fn log(&self, msg: impl AsRef<str>) {
        log(msg)
    }
    pub fn fail<T>(&self, message: impl ToString) -> Result<T> {
        let m = message.to_string();
        log(format!("wheel open failed: {m}"));
        Err(Error::Fail(m))
    }
    pub fn installed_apps(&self) -> Result<Vec<Value>> {
        crate::host::installed_apps()
    }
}

/// A subscribed host event delivered to `Plugin::on_event`.
pub struct Event {
    pub name: String,
    pub payload: Value,
    pub fg_process: Option<String>,
    pub fg_title: Option<String>,
}
