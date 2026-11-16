//! The KnobDeck plugin protocol, defined once.
//!
//! A plugin is a process the host talks to over stdio, one JSON object per line. [`HostMsg`] is
//! every frame the host writes to the plugin's stdin; [`PluginMsg`] is every frame the plugin
//! writes to its stdout. The host and the Rust SDK both use these types, and the JSON Schema in
//! `schema/` (and the TypeScript types made from it) are generated from them, so there is one
//! source of truth for the wire format.
//!
//! What may change without breaking plugins is in `COMPAT.md` at the root of the SDK repo. In
//! short: fields are only ever added, as optional; receivers ignore unknown fields and frame types
//! (both enums end in an `Unknown` catch-all); behaviour only some hosts have is announced in
//! [`Hello::features`]. The example frames in [`fixtures::V2`] pin the format: the tests here
//! parse every one and write it back byte for byte.

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

#[cfg(feature = "schema")]
use schemars::JsonSchema;

/// The newest protocol number. It names a major version and moves only for a break.
pub const PROTOCOL: u32 = 2;

/// The oldest protocol number a host still loads. It moves up only after a public deprecation
/// notice of at least 12 months (`COMPAT.md`).
pub const MIN_PROTOCOL: u32 = 2;

const _: () = assert!(MIN_PROTOCOL <= PROTOCOL);

/// Whether a host speaking this crate's protocol can run a plugin built for protocol `p`.
pub fn protocol_supported(p: u32) -> bool {
    (MIN_PROTOCOL..=PROTOCOL).contains(&p)
}

/// Optional host behaviour, listed in [`Hello::features`]. A plugin uses one only when the host
/// lists it. Adding a feature is never a break.
pub mod features {
    /// A `state`/`error` frame sent to an `action_id` may carry `match`; only instances whose
    /// inputs hold those values get it. Without it, the SDK sends one frame per instance.
    pub const STATE_MATCH: &str = "state_match";

    /// The host sends the [`events`](crate::events) marked "needs `events`" to plugins that
    /// subscribe to them.
    pub const EVENTS: &str = "events";

    /// The host takes the [`notify`](crate::host_calls::NOTIFY) host call.
    pub const NOTIFY: &str = "notify";

    /// The host takes the [`app_state`](crate::host_calls::APP_STATE) host call.
    pub const APP_STATE: &str = "app_state";

    /// The host takes the [`open_dial`](crate::host_calls::OPEN_DIAL) host call.
    pub const OPEN_DIAL: &str = "open_dial";

    /// Every feature defined by this version of the protocol.
    pub const ALL: &[&str] = &[STATE_MATCH, EVENTS, NOTIFY, APP_STATE, OPEN_DIAL];
}

/// Host events a plugin can subscribe to (`ready.subscribe`, `subscribe`).
pub mod events {
    /// The foreground app changed. Payload: `{"app": "<process or bundle name>"}`. Current hosts
    /// don't send it: a plugin that follows the foreground app marks its actions `.volatile()`
    /// in the SDK. Subscribing is still allowed.
    pub const FOCUS_CHANGED: &str = "focus_changed";

    /// The dial opened. Payload: [`DialOpened`](crate::DialOpened). Needs `events`.
    pub const DIAL_OPENED: &str = "dial_opened";

    /// The dial closed. Payload: `{}`. Needs `events`.
    pub const DIAL_CLOSED: &str = "dial_closed";

    /// The user's profiles were edited, added or removed (placed instances arrive separately, as
    /// `sync`). Payload: `{}`. Needs `events`.
    pub const PROFILES_CHANGED: &str = "profiles_changed";

    pub const ALL: &[&str] = &[FOCUS_CHANGED, DIAL_OPENED, DIAL_CLOSED, PROFILES_CHANGED];
}

/// Values of [`Invoke::trigger`].
pub mod triggers {
    pub const PRESS: &str = "press";
    pub const HOLD_START: &str = "hold_start";
    pub const HOLD_END: &str = "hold_end";
    /// A slider moved: `inputs.value` (absolute) and/or `inputs.delta`.
    pub const ADJUST: &str = "adjust";

    pub const ALL: &[&str] = &[PRESS, HOLD_START, HOLD_END, ADJUST];
}

/// Values of [`HostMsg::Request`] `method`.
pub mod requests {
    /// `params: {query_id, inputs}`; the plugin answers with a list for a setup-form field.
    pub const QUERY: &str = "query";
    /// `params: {instance_id, action_id, inputs}`; the plugin answers with a dynamic wheel's buttons.
    pub const WHEEL: &str = "wheel";

    pub const ALL: &[&str] = &[QUERY, WHEEL];
}

/// Values of [`PluginMsg::HostCall`] `method`.
pub mod host_calls {
    /// `params: {keys}`. Last resort: plugins should drive apps through their APIs (Rule 1).
    pub const SEND_KEYS: &str = "send_keys";

    /// `params`: [`Notify`](crate::Notify). Show the user a short message from the plugin.
    /// Answers `null`. Needs the `notify` feature.
    pub const NOTIFY: &str = "notify";

    /// `params`: `{}`. Answers [`AppState`](crate::AppState). Needs the `app_state` feature.
    pub const APP_STATE: &str = "app_state";

    /// `params`: [`OpenDial`](crate::OpenDial). Open the dial, e.g. when the app a plugin watches
    /// starts streaming. Answers `true` when it opened, `false` when it was already open. Refused
    /// for an unknown profile and when called more than once every few seconds. Needs the
    /// `open_dial` feature.
    pub const OPEN_DIAL: &str = "open_dial";

    pub const ALL: &[&str] = &[SEND_KEYS, NOTIFY, APP_STATE, OPEN_DIAL];
}

#[cfg(feature = "schema")]
pub mod generate;

/// A `match` filter: the inputs an instance must hold to receive a frame.
pub type Match = Map<String, Value>;

// ── Host → plugin ────────────────────────────────────────────────────────────

/// A frame the host writes to the plugin's stdin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostMsg {
    /// Always the first line.
    Hello(Hello),
    /// The plugin's placed instances, sent after `ready` and whenever they change.
    Sync {
        /// Every placed instance of this plugin's actions; replaces the previous list.
        instances: Vec<Instance>,
    },
    /// Run one instance. The plugin answers `ack` at once and `done` when the handler returns.
    Invoke(Invoke),
    /// Ask the plugin for data; answered by a `result` with the same `id`.
    Request {
        /// Echoed by the answering `result`.
        id: String,
        /// A [`requests`] value.
        method: String,
        /// Method-specific arguments.
        #[serde(default)]
        params: Value,
    },
    /// A host event the plugin subscribed to.
    Event {
        /// An [`events`] value.
        event: String,
        /// Event-specific data.
        #[serde(default)]
        payload: Value,
    },
    /// The answer to a plugin's `host_call`.
    Result(CallResult),
    /// Stop now. The host kills the process if it is still alive two seconds later.
    Shutdown,
    /// A frame type this version doesn't know. Receivers ignore it. Never sent.
    #[serde(other)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    Unknown,
}

/// The body of `hello`: who the plugin is and what the host can do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Hello {
    /// The newest protocol number the host speaks.
    pub protocol: u32,
    /// The plugin's id. Action ids are made from it (`make_full_id` in the SDK).
    pub plugin_id: String,
    /// A folder the plugin owns for its settings and caches.
    pub data_dir: String,
    /// KnobDeck's version, for logs and bug reports. Check `features`, not this, before using
    /// optional behaviour.
    pub host_version: String,
    /// The [`features`] this host has.
    #[serde(default)]
    pub features: Vec<String>,
    /// The host's process id, so a plugin can leave KnobDeck's own windows out of "the target
    /// window".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_pid: Option<u32>,
}

impl Hello {
    pub fn has_feature(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }
}

/// One placed instance of an action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Instance {
    /// Stable for as long as the action stays where the user put it.
    pub instance_id: String,
    /// The full action id (`<plugin>-<action>-<hash>`).
    pub action_id: String,
    /// The values the user set in the action's setup form.
    #[serde(default)]
    pub inputs: Value,
}

/// The body of `invoke`: run one placed instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Invoke {
    /// Echoed by `ack` and `done`.
    pub id: String,
    /// For a dynamic-wheel child: `"<wheel instance>::<child_id>"`.
    pub instance_id: String,
    /// The full action id.
    pub action_id: String,
    /// A [`triggers`] value.
    pub trigger: String,
    /// The instance's inputs; for `adjust`, plus `value` (absolute) and/or `delta`.
    #[serde(default)]
    pub inputs: Value,
    /// Set when a dynamic wheel's child button was pressed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_id: Option<String>,
}

/// `result`: the answer to a `request` (plugin → host) or a `host_call` (host → plugin).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CallResult {
    /// The `id` of the `request` or `host_call` this answers.
    pub id: String,
    /// Whether it worked: then `value` is set, else `error`.
    pub ok: bool,
    /// Present when `ok`. May be `null`, which is kept apart from "absent".
    #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    /// Present when not `ok`. Human-readable; not part of the contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl CallResult {
    pub fn ok(id: impl Into<String>, value: Value) -> Self {
        CallResult { id: id.into(), ok: true, value: Some(value), error: None }
    }
    pub fn err(id: impl Into<String>, error: impl Into<String>) -> Self {
        CallResult { id: id.into(), ok: false, value: None, error: Some(error.into()) }
    }
    /// The value, or the error.
    pub fn into_result(self) -> Result<Value, String> {
        if self.ok {
            Ok(self.value.unwrap_or(Value::Null))
        } else {
            Err(self.error.unwrap_or_else(|| "call failed".into()))
        }
    }
}

/// `Some` whenever the key is present, `null` included (plain `Option<Value>` reads `null` as
/// `None`, which would drop it when the frame is written back).
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

// ── Plugin → host ────────────────────────────────────────────────────────────

/// A frame the plugin writes to its stdout. Nothing else may go to stdout; logs go to stderr.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginMsg {
    /// The first line, after `hello`.
    Ready {
        /// The protocol the plugin was built for.
        protocol: u32,
        /// [`events`] to receive from the start.
        #[serde(default)]
        subscribe: Vec<String>,
        /// The plugin sends `done` after each invoke's handler returns. Workflows wait for it.
        #[serde(default)]
        done: bool,
    },
    /// An `invoke` arrived (liveness).
    Ack {
        /// The invoke's `id`.
        id: String,
    },
    /// An `invoke`'s handler returned, so its effects have landed.
    Done {
        /// The invoke's `id`.
        id: String,
    },
    /// State and/or a temporary label/icon for one instance, or for every instance of an action.
    State(StateUpdate),
    /// A failure for `instance_id`, else the invoke `id`'s instance, else every instance of
    /// `action_id`.
    Error(ErrorReport),
    /// The answer to a host `request`.
    Result(CallResult),
    /// Ask the host to do something; answered by a `result` with the same `id`.
    HostCall {
        /// Echoed by the host's `result`.
        id: String,
        /// A [`host_calls`] value.
        method: String,
        /// Method-specific arguments.
        #[serde(default)]
        params: Value,
    },
    /// Replace the plugin's event subscriptions.
    Subscribe {
        /// [`events`] values; an empty list unsubscribes from everything.
        events: Vec<String>,
    },
    /// The plugin's setup is no longer valid (an expired token…); the host asks the user again.
    SetupRequired {
        /// Shown to the user.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// A frame type this version doesn't know. Receivers ignore it. Never sent.
    #[serde(other)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    Unknown,
}

/// Who a `state` or `error` frame is for: one instance, or every instance of an action
/// (narrowed by `match` when the host has [`features::STATE_MATCH`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Target {
    /// One instance. Wins over `action_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    /// Every instance of this action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_id: Option<String>,
    /// With `action_id`: only instances whose inputs hold these values (compared as strings; a
    /// missing input counts as `""`). Needs the `state_match` feature.
    #[serde(default, rename = "match", skip_serializing_if = "Option::is_none")]
    pub filter: Option<Match>,
}

impl Target {
    pub fn instance(id: impl Into<String>) -> Self {
        Target { instance_id: Some(id.into()), ..Default::default() }
    }
    pub fn action(id: impl Into<String>) -> Self {
        Target { action_id: Some(id.into()), ..Default::default() }
    }
    pub fn matching(mut self, filter: Match) -> Self {
        self.filter = Some(filter);
        self
    }
}

/// The body of a `state` frame: a new state and/or a temporary label or icon, and who gets it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct StateUpdate {
    #[serde(flatten)]
    pub target: Target,
    /// The new state. A frame without it leaves state and error alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// A temporary label shown instead of the configured one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// A temporary icon name shown instead of the configured one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// The body of an `error` frame: what failed, and who gets it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ErrorReport {
    #[serde(flatten)]
    pub target: Target,
    /// The invoke this error ends, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Shown to the user on the action.
    pub message: String,
}

// ── Payloads (`event.payload`, `host_call.params` and answers) ────────────────

/// Payload of the `dial_opened` event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct DialOpened {
    /// The profile the dial shows.
    pub profile_id: String,
}

/// `params` of the `notify` host call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Notify {
    /// One short sentence. The host may shorten long text.
    pub message: String,
    /// `info` (default) or `error`. Unknown values read as `info`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
}

/// `params` of the `open_dial` host call.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct OpenDial {
    /// The dial profile to show. Absent: the one the user's auto-open rules pick, as the dial
    /// keybind does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

/// The answer to the `app_state` host call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct AppState {
    /// `windows`, `macos` or `linux`.
    pub os: String,
    pub host_version: String,
    /// Whether the dial is on screen.
    pub dial_open: bool,
}

/// The pinned example frames (`fixtures/`), one JSON object per line:
/// `{"dir": "to_plugin" | "to_host", "frame": {…}}`.
pub mod fixtures {
    /// Protocol 2.
    pub const V2: &str = include_str!("../fixtures/protocol-v2.jsonl");

    /// The frames of `fixture` going in `dir` (`"to_plugin"` or `"to_host"`), in file order.
    pub fn frames(fixture: &str, dir: &str) -> Vec<serde_json::Value> {
        fixture
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str::<serde_json::Value>(l).expect("fixture line is JSON"))
            .filter(|e| e["dir"] == dir)
            .map(|e| e["frame"].clone())
            .collect()
    }
}
