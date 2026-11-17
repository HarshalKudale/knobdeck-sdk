//! Layer 3: the external control API. A local program (a script, a stream tool, a launcher)
//! drives KnobDeck: open the dial, run an item the user placed, list profiles and plugins, follow
//! events. Off by default (`controlApi.enabled` in settings).
//!
//! Transport: newline-delimited JSON over a Unix socket ([`UNIX_SOCKET`] under the user's runtime
//! folder) on macOS and Linux, a named pipe ([`PIPE_PREFIX`]`<user name>`) on Windows. Never TCP.
//! The first line is [`ClientMsg::Hello`] with the token from [`TOKEN_FILE`] in KnobDeck's data
//! folder (readable by the user only).
//!
//! Same compatibility rules as layer 1 (`COMPAT.md`); [`CONTROL_PROTOCOL`] moves only for a break.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(feature = "schema")]
use schemars::JsonSchema;

pub const CONTROL_PROTOCOL: u32 = 1;

/// `<runtime folder>/knobdeck/control.sock`: `$XDG_RUNTIME_DIR` on Linux, the per-user temp
/// folder on macOS.
pub const UNIX_SOCKET: &str = "control.sock";

/// `\\.\pipe\knobdeck-control-<user name>` on Windows.
pub const PIPE_PREFIX: &str = r"\\.\pipe\knobdeck-control-";

/// The token file in KnobDeck's data folder.
pub const TOKEN_FILE: &str = "control-token";

/// Command names ([`ClientMsg::Request`] `cmd`).
pub mod commands {
    /// `params`: [`OpenDial`](crate::OpenDial). Answers `true` when it opened.
    pub const OPEN_DIAL: &str = "open_dial";
    /// `params`: `{}`. Answers `null`.
    pub const CLOSE_DIAL: &str = "close_dial";
    /// `params`: [`RunItem`](super::RunItem). Run an item the user placed on a dial profile, as a
    /// press (or a slider move with `value`). Answers `null` once it was dispatched.
    pub const RUN_ITEM: &str = "run_item";
    /// `params`: `{}`. Answers a list of [`ProfileSummary`](super::ProfileSummary).
    pub const LIST_PROFILES: &str = "list_profiles";
    /// `params`: `{}`. Answers a list of [`PluginSummary`](super::PluginSummary).
    pub const LIST_PLUGINS: &str = "list_plugins";
    /// `params`: [`ListItems`](super::ListItems). Answers a list of
    /// [`ItemSummary`](super::ItemSummary): every item on that profile, folders' items included.
    pub const LIST_ITEMS: &str = "list_items";
    /// `params`: [`Subscribe`](super::Subscribe). Replaces this connection's subscriptions to the
    /// host events (`dial_opened`, `dial_closed`, `profiles_changed`); they arrive as `event`.
    pub const SUBSCRIBE: &str = "subscribe";

    pub const ALL: &[&str] = &[OPEN_DIAL, CLOSE_DIAL, RUN_ITEM, LIST_PROFILES, LIST_PLUGINS, SUBSCRIBE, LIST_ITEMS];
}

/// A line a client sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// The first line.
    Hello {
        protocol: u32,
        /// The contents of [`TOKEN_FILE`], trimmed.
        token: String,
        /// Who is calling, for KnobDeck's log ("obs-script", "raycast").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client: Option<String>,
        /// This client acts for an AI assistant (KnobDeck's MCP server). KnobDeck then lists,
        /// opens and runs only profiles the user opened to assistants
        /// ([`ProfileSummary::assistant`]).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        assistant: bool,
    },
    /// Run a command; answered by a `response` with the same `id`.
    Request {
        id: String,
        /// A [`commands`] value.
        cmd: String,
        #[serde(default)]
        params: Value,
    },
    /// A line type this version doesn't know. Ignored. Never sent.
    #[serde(other)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    Unknown,
}

/// A line KnobDeck sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    /// `hello` was accepted.
    Ready {
        protocol: u32,
        host_version: String,
        /// The [`commands`] this KnobDeck takes.
        commands: Vec<String>,
    },
    /// `hello` was refused; the connection closes after this.
    Refused { reason: String },
    /// The answer to a `request`.
    Response {
        id: String,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// A subscribed host event.
    Event {
        event: String,
        #[serde(default)]
        payload: Value,
    },
    /// A line type this version doesn't know. Ignored. Never sent.
    #[serde(other)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    Unknown,
}

/// `params` of `run_item`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RunItem {
    /// A dial profile id (`list_profiles`).
    pub profile_id: String,
    /// The item's id in that profile (folders included).
    pub item_id: String,
    /// For a slider: the value to set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
}

/// `params` of `subscribe`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Subscribe {
    /// Event names; an empty list unsubscribes from everything.
    pub events: Vec<String>,
}

/// One dial profile in `list_profiles`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ProfileSummary {
    pub id: String,
    pub name: String,
    /// The profile the dial keybind opens when no auto-open rule matches.
    #[serde(default)]
    pub default: bool,
    /// The user lets AI assistants see and run this profile's items.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub assistant: bool,
}

/// `params` of `list_items`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ListItems {
    /// A dial profile id (`list_profiles`).
    pub profile_id: String,
}

/// One item in `list_items`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ItemSummary {
    /// The id `run_item` takes.
    pub id: String,
    /// The label the user sees on the dial.
    pub label: String,
    /// `button`, `toggle`, `slider`, `dynamic`, `multi` or `folder`.
    pub kind: String,
    /// The plugin that runs it; none for KnobDeck's own actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    /// The folder it sits in, by label (`Streaming / Scenes`); none at the top level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// A slider's range and unit: `run_item` takes a `value` in it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// One plugin in `list_plugins`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct PluginSummary {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// `running`, `stopped`, `crashed`, `setup_required` or `outdated`.
    pub status: String,
}

/// Example lines (`fixtures/control-v1.jsonl`): `{"dir": "to_server" | "to_client", "frame": {…}}`.
pub const FIXTURES_V1: &str = include_str!("../fixtures/control-v1.jsonl");
