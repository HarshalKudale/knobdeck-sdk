//! Helpers for building action input descriptors. They return plain JSON
//! objects, so add any extra field (`placeholder`, `default`, `showIf`, `query`,
//! `options`…) with `json!` or by mutating the returned value.

use serde_json::{json, Value};

fn field(key: &str, label: &str, ty: &str) -> Value {
    json!({"key": key, "label": label, "type": ty})
}

pub fn text(key: &str, label: &str) -> Value {
    field(key, label, "text")
}
pub fn textarea(key: &str, label: &str) -> Value {
    field(key, label, "textarea")
}
pub fn checkbox(key: &str, label: &str) -> Value {
    field(key, label, "checkbox")
}
pub fn keybind(key: &str, label: &str) -> Value {
    field(key, label, "keybind")
}
/// Static select; `options` are `(value, label)` pairs.
pub fn select(key: &str, label: &str, options: &[(&str, &str)]) -> Value {
    let mut v = field(key, label, "select");
    v["options"] = options.iter().map(|(value, label)| json!({"value": value, "label": label})).collect();
    v
}
/// Select fed live by `Plugin::query(query_id, …)`; `args` are sibling input keys it depends on.
pub fn query_select(key: &str, label: &str, query_id: &str, args: &[&str]) -> Value {
    let mut v = field(key, label, "select");
    v["query"] = if args.is_empty() { json!(query_id) } else { json!({"id": query_id, "args": args}) };
    v
}

/// Searchable select fed live by `Plugin::query(query_id, …)`.
pub fn query_combobox(key: &str, label: &str, query_id: &str) -> Value {
    let mut v = field(key, label, "combobox");
    v["query"] = json!(query_id);
    v
}

/// Builder-style extras on any input value.
pub trait InputExt {
    fn placeholder(self, p: &str) -> Value;
    fn default_value(self, d: impl Into<Value>) -> Value;
    fn with(self, key: &str, v: impl Into<Value>) -> Value;
    /// The field takes placeholders (`{selection}`, `{date}`…): KnobDeck shows an "Insert
    /// placeholder" menu next to it. Expand the value with `ctx.expand(..)`.
    fn accepts_placeholders(self) -> Value;
}

impl InputExt for Value {
    fn placeholder(self, p: &str) -> Value {
        self.with("placeholder", p)
    }
    fn default_value(self, d: impl Into<Value>) -> Value {
        self.with("default", d)
    }
    fn with(mut self, key: &str, v: impl Into<Value>) -> Value {
        self[key] = v.into();
        self
    }
    fn accepts_placeholders(self) -> Value {
        self.with("placeholders", true)
    }
}
