//! Layer 2: an app-side companion (a Photoshop panel, a Blender add-on, a browser extension…) and
//! KnobDeck's companion hub.
//!
//! The companion dials the hub over a WebSocket on loopback (the port is in the discovery file,
//! [`DISCOVERY_FILE`]) and sends JSON text frames. It names the plugin it belongs to in `hello`; the
//! hub checks it, then relays: a plugin's `companion_request` host call becomes a [`HubMsg::Request`],
//! the companion's [`CompanionMsg::Response`] answers it, and a [`CompanionMsg::Event`] reaches the
//! plugin as a `companion_event` host event. The plugin never opens a port.
//!
//! Same compatibility rules as layer 1 (`COMPAT.md`): fields are added as optional, unknown frames
//! and fields are ignored, [`COMPANION_PROTOCOL`] moves only for a break.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(feature = "schema")]
use schemars::JsonSchema;

/// The companion protocol number (independent of the plugin protocol's).
pub const COMPANION_PROTOCOL: u32 = 1;

/// The discovery file, in KnobDeck's data folder: [`Discovery`] as JSON, readable only by the
/// user. A companion that can read files takes the port and token from it.
pub const DISCOVERY_FILE: &str = "companion-hub.json";

/// The WebSocket path on the hub's port.
pub const WS_PATH: &str = "/companion";

/// The first port the hub tries; it takes the next free one of [`PORT_RANGE`] ports.
pub const DEFAULT_PORT: u16 = 32150;
pub const PORT_RANGE: u16 = 5;

/// How often a companion pings, and after how long the hub drops a silent link.
pub const PING_EVERY_SECS: u64 = 20;
pub const SILENT_DROP_SECS: u64 = 75;

/// What the hub writes to [`DISCOVERY_FILE`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Discovery {
    pub protocol: u32,
    /// The hub's loopback port.
    pub port: u16,
    /// [`WS_PATH`].
    pub path: String,
    /// Sent back in `hello.token`. A companion that can't read files (a browser extension)
    /// connects without it, from an extension origin.
    pub token: String,
}

/// A frame a companion sends to the hub.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CompanionMsg {
    /// The first frame.
    Hello(CompanionInfo),
    /// Every [`PING_EVERY_SECS`]; answered `pong`.
    Ping,
    /// The answer to a hub `request`.
    Response(ActionResult),
    /// Live state from the app (the current tool, the active layer…), for the plugin.
    Event {
        /// Companion-specific name.
        event: String,
        /// Event-specific data.
        #[serde(default)]
        payload: Value,
    },
    /// A frame type this version doesn't know. Receivers ignore it. Never sent.
    #[serde(other)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    Unknown,
}

/// The body of a companion's `hello`: who it is and which plugin it belongs to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct CompanionInfo {
    /// The companion protocol the companion speaks.
    pub protocol: u32,
    /// The KnobDeck plugin this companion belongs to (its manifest id).
    pub plugin: String,
    /// Stable per install / profile / window, so a reconnect replaces the old link.
    pub instance: String,
    /// The app it lives in (`photoshop`, `blender`, `chrome`…).
    pub app: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
    /// What the user sees ("Photoshop 2026", "Chrome — Work").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The companion's own version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Optional companion behaviour, for the plugin to check.
    #[serde(default)]
    pub features: Vec<String>,
    /// [`Discovery::token`], when the companion could read the discovery file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// The body of a companion's `response`: how an action went.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ActionResult {
    /// The `request` this answers.
    pub id: String,
    pub ok: bool,
    /// When `ok`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// When not `ok`: a message for the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// When not `ok` because the app has no API for this action.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unsupported: bool,
}

/// A frame the hub sends to a companion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HubMsg {
    /// `hello` was accepted; the link is up.
    Ready {
        /// The companion protocol the hub speaks.
        protocol: u32,
        /// The id the plugin knows this link by.
        link: String,
        host_version: String,
    },
    /// `hello` was refused; the hub closes the socket after this.
    Refused {
        /// Why, for the companion's log or UI.
        reason: String,
    },
    /// The answer to `ping`.
    Pong,
    /// Run an action in the app; answered by a `response` with the same `id`.
    Request {
        id: String,
        /// Companion-specific action id.
        action: String,
        #[serde(default)]
        params: Value,
    },
    /// A frame type this version doesn't know. Receivers ignore it. Never sent.
    #[serde(other)]
    #[cfg_attr(feature = "schema", schemars(skip))]
    Unknown,
}

// ── What the plugin sees (layer 1, feature `companion_hub`) ──────────────────

/// One connected companion, as the plugin sees it (`companion_connected`, `companion_links`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct Link {
    /// The hub's id for this link; pass it to `companion_request`.
    pub link: String,
    pub instance: String,
    pub app: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub features: Vec<String>,
}

/// Payload of `companion_disconnected`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct LinkGone {
    pub link: String,
}

/// Payload of `companion_event`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct LinkEvent {
    pub link: String,
    /// The companion's event name.
    pub event: String,
    #[serde(default)]
    pub payload: Value,
}

/// `params` of the `companion_request` host call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct LinkRequest {
    /// A link from `companion_connected` / `companion_links`; absent: the most recently
    /// connected link of this plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    pub action: String,
    #[serde(default)]
    pub params: Value,
    /// How long to wait for the companion (default 5000, at most 30000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// The fixed example frames for layer 2 (`fixtures/companion-v1.jsonl`), as for layer 1:
/// `{"dir": "to_hub" | "to_companion", "frame": {…}}`.
pub const FIXTURES_V1: &str = include_str!("../fixtures/companion-v1.jsonl");
