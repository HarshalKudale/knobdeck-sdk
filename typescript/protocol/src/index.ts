// Generated from the knobdeck-protocol crate (knobdeck-sdk repo). Do not edit: change the Rust
// types and run `KNOBDECK_BLESS=1 cargo test -p knobdeck-protocol --features schema`.

/** The newest protocol number. It moves only for a break (COMPAT.md). */
export const PROTOCOL = 2;
/** The oldest protocol number a host still loads. */
export const MIN_PROTOCOL = 2;

/** The companion protocol number (layer 2). */
export const COMPANION_PROTOCOL = 1;
/** The hub's discovery file, in KnobDeck's data folder (`Discovery`). */
export const COMPANION_DISCOVERY_FILE = "companion-hub.json";
/** The WebSocket path on the hub's port. */
export const COMPANION_WS_PATH = "/companion";
/** The hub takes the first free port of `COMPANION_PORTS`. */
export const COMPANION_PORTS = [32150, 32151, 32152, 32153, 32154] as const;
/** Ping every this many seconds; the hub drops a link silent for `COMPANION_SILENT_DROP_SECS`. */
export const COMPANION_PING_EVERY_SECS = 20;
export const COMPANION_SILENT_DROP_SECS = 75;

/** The control protocol number (layer 3). */
export const CONTROL_PROTOCOL = 1;
export const CONTROL_UNIX_SOCKET = "control.sock";
export const CONTROL_PIPE_PREFIX = "\\\\.\\pipe\\knobdeck-control-";
export const CONTROL_TOKEN_FILE = "control-token";

/** Commands of the control API. */
export const CONTROL_COMMANDS = ["open_dial", "close_dial", "run_item", "list_profiles", "list_plugins", "subscribe", "list_items"] as const;
export type ControlCommand = (typeof CONTROL_COMMANDS)[number];

/** Optional host behaviour, listed in `hello.features`. */
export const FEATURES = ["state_match", "events", "notify", "app_state", "open_dial", "companion_hub"] as const;
export type Feature = (typeof FEATURES)[number];

/** Host events a plugin can subscribe to. */
export const EVENTS = ["focus_changed", "dial_opened", "dial_closed", "profiles_changed", "companion_connected", "companion_disconnected", "companion_event"] as const;
export type EventName = (typeof EVENTS)[number];

/** Values of `invoke.trigger`. */
export const TRIGGERS = ["press", "hold_start", "hold_end", "adjust"] as const;
export type Trigger = (typeof TRIGGERS)[number];

/** Values of `request.method`. */
export const REQUESTS = ["query", "wheel"] as const;
export type RequestMethod = (typeof REQUESTS)[number];

/** Values of `host_call.method`. */
export const HOST_CALLS = ["send_keys", "notify", "app_state", "open_dial", "companion_request", "companion_links"] as const;
export type HostCallMethod = (typeof HOST_CALLS)[number];

/** The body of a companion's `response`: how an action went. */
export interface ActionResult {
  /** The `request` this answers. */
  id: string;
  ok: boolean;
  /** When `ok`. */
  result?: unknown;
  /** When not `ok`: a message for the user. */
  error?: string;
  /** When not `ok` because the app has no API for this action. */
  unsupported?: boolean;
}

/** The answer to the `app_state` host call. */
export interface AppState {
  /** `windows`, `macos` or `linux`. */
  os: string;
  host_version: string;
  /** Whether the dial is on screen. */
  dial_open: boolean;
}

/** `result`: the answer to a `request` (plugin → host) or a `host_call` (host → plugin). */
export interface CallResult {
  /** The `id` of the `request` or `host_call` this answers. */
  id: string;
  /** Whether it worked: then `value` is set, else `error`. */
  ok: boolean;
  /** Present when `ok`. May be `null`, which is kept apart from "absent". */
  value?: unknown;
  /** Present when not `ok`. Human-readable; not part of the contract. */
  error?: string;
}

/** The body of a companion's `hello`: who it is and which plugin it belongs to. */
export interface CompanionInfo {
  /** The companion protocol the companion speaks. */
  protocol: number;
  /** The KnobDeck plugin this companion belongs to (its manifest id). */
  plugin: string;
  /** Stable per install / profile / window, so a reconnect replaces the old link. */
  instance: string;
  /** The app it lives in (`photoshop`, `blender`, `chrome`…). */
  app: string;
  app_version?: string;
  /** What the user sees ("Photoshop 2026", "Chrome — Work"). */
  name?: string;
  /** The companion's own version. */
  version?: string;
  /** Optional companion behaviour, for the plugin to check. */
  features?: string[];
  /** `Discovery::token`, when the companion could read the discovery file. */
  token?: string;
}

/** Payload of the `dial_opened` event. */
export interface DialOpened {
  /** The profile the dial shows. */
  profile_id: string;
}

/** What the hub writes to `DISCOVERY_FILE`. */
export interface Discovery {
  protocol: number;
  /** The hub's loopback port. */
  port: number;
  /** `WS_PATH`. */
  path: string;
  /**
   * Sent back in `hello.token`. A companion that can't read files (a browser extension)
   * connects without it, from an extension origin.
   */
  token: string;
}

/** The body of an `error` frame: what failed, and who gets it. */
export interface ErrorReport {
  /** One instance. Wins over `action_id`. */
  instance_id?: string;
  /** Every instance of this action. */
  action_id?: string;
  /**
   * With `action_id`: only instances whose inputs hold these values (compared as strings; a
   * missing input counts as `""`). Needs the `state_match` feature.
   */
  match?: Record<string, unknown>;
  /** The invoke this error ends, when there is one. */
  id?: string;
  /** Shown to the user on the action. */
  message: string;
}

/** The body of `hello`: who the plugin is and what the host can do. */
export interface Hello {
  /** The newest protocol number the host speaks. */
  protocol: number;
  /** The plugin's id. Action ids are made from it (`make_full_id` in the SDK). */
  plugin_id: string;
  /** A folder the plugin owns for its settings and caches. */
  data_dir: string;
  /**
   * KnobDeck's version, for logs and bug reports. Check `features`, not this, before using
   * optional behaviour.
   */
  host_version: string;
  /** The `features` this host has. */
  features?: string[];
  /**
   * The host's process id, so a plugin can leave KnobDeck's own windows out of "the target
   * window".
   */
  host_pid?: number;
}

/** One placed instance of an action. */
export interface Instance {
  /** Stable for as long as the action stays where the user put it. */
  instance_id: string;
  /** The full action id (`<plugin>-<action>-<hash>`). */
  action_id: string;
  /** The values the user set in the action's setup form. */
  inputs?: unknown;
}

/** The body of `invoke`: run one placed instance. */
export interface Invoke {
  /** Echoed by `ack` and `done`. */
  id: string;
  /** For a dynamic-wheel child: `"<wheel instance>::<child_id>"`. */
  instance_id: string;
  /** The full action id. */
  action_id: string;
  /** A `triggers` value. */
  trigger: string;
  /** The instance's inputs; for `adjust`, plus `value` (absolute) and/or `delta`. */
  inputs?: unknown;
  /** Set when a dynamic wheel's child button was pressed. */
  child_id?: string;
}

/** One item in `list_items`. */
export interface ItemSummary {
  /** The id `run_item` takes. */
  id: string;
  /** The label the user sees on the dial. */
  label: string;
  /** `button`, `toggle`, `slider`, `dynamic`, `multi` or `folder`. */
  kind: string;
  /** The plugin that runs it; none for KnobDeck's own actions. */
  plugin?: string;
  /** The folder it sits in, by label (`Streaming / Scenes`); none at the top level. */
  folder?: string;
  /** A slider's range and unit: `run_item` takes a `value` in it. */
  min?: number;
  max?: number;
  unit?: string;
}

/** One connected companion, as the plugin sees it (`companion_connected`, `companion_links`). */
export interface Link {
  /** The hub's id for this link; pass it to `companion_request`. */
  link: string;
  instance: string;
  app: string;
  app_version?: string;
  name?: string;
  version?: string;
  features?: string[];
}

/** Payload of `companion_event`. */
export interface LinkEvent {
  link: string;
  /** The companion's event name. */
  event: string;
  payload?: unknown;
}

/** Payload of `companion_disconnected`. */
export interface LinkGone {
  link: string;
}

/** `params` of the `companion_request` host call. */
export interface LinkRequest {
  /**
   * A link from `companion_connected` / `companion_links`; absent: the most recently
   * connected link of this plugin.
   */
  link?: string;
  action: string;
  params?: unknown;
  /** How long to wait for the companion (default 5000, at most 30000). */
  timeout_ms?: number;
}

/** `params` of `list_items`. */
export interface ListItems {
  /** A dial profile id (`list_profiles`). */
  profile_id: string;
}

/** `params` of the `notify` host call. */
export interface Notify {
  /** One short sentence. The host may shorten long text. */
  message: string;
  /** `info` (default) or `error`. Unknown values read as `info`. */
  level?: string;
}

/** `params` of the `open_dial` host call. */
export interface OpenDial {
  /**
   * The dial profile to show. Absent: the one the user's auto-open rules pick, as the dial
   * keybind does.
   */
  profile_id?: string;
}

/** One plugin in `list_plugins`. */
export interface PluginSummary {
  id: string;
  name: string;
  version?: string;
  /** `running`, `stopped`, `crashed`, `setup_required` or `outdated`. */
  status: string;
}

/** One dial profile in `list_profiles`. */
export interface ProfileSummary {
  id: string;
  name: string;
  /** The profile the dial keybind opens when no auto-open rule matches. */
  default?: boolean;
  /** The user lets AI assistants see and run this profile's items. */
  assistant?: boolean;
}

/** `params` of `run_item`. */
export interface RunItem {
  /** A dial profile id (`list_profiles`). */
  profile_id: string;
  /** The item's id in that profile (folders included). */
  item_id: string;
  /** For a slider: the value to set. */
  value?: number;
}

/** The body of a `state` frame: a new state and/or a temporary label or icon, and who gets it. */
export interface StateUpdate {
  /** One instance. Wins over `action_id`. */
  instance_id?: string;
  /** Every instance of this action. */
  action_id?: string;
  /**
   * With `action_id`: only instances whose inputs hold these values (compared as strings; a
   * missing input counts as `""`). Needs the `state_match` feature.
   */
  match?: Record<string, unknown>;
  /** The new state. A frame without it leaves state and error alone. */
  state?: string;
  /** A temporary label shown instead of the configured one. */
  label?: string;
  /** A temporary icon name shown instead of the configured one. */
  icon?: string;
}

/** `params` of `subscribe`. */
export interface Subscribe {
  /** Event names; an empty list unsubscribes from everything. */
  events: string[];
}

/** Always the first line. */
export interface HostHello extends Hello {
  type: "hello";
}

/** The plugin's placed instances, sent after `ready` and whenever they change. */
export interface HostSync {
  type: "sync";
  /** Every placed instance of this plugin's actions; replaces the previous list. */
  instances: Instance[];
}

/** Run one instance. The plugin answers `ack` at once and `done` when the handler returns. */
export interface HostInvoke extends Invoke {
  type: "invoke";
}

/** Ask the plugin for data; answered by a `result` with the same `id`. */
export interface HostRequest {
  type: "request";
  /** Echoed by the answering `result`. */
  id: string;
  /** A `requests` value. */
  method: string;
  /** Method-specific arguments. */
  params?: unknown;
}

/** A host event the plugin subscribed to. */
export interface HostEvent {
  type: "event";
  /** An `events` value. */
  event: string;
  /** Event-specific data. */
  payload?: unknown;
}

/** The answer to a plugin's `host_call`. */
export interface HostResult extends CallResult {
  type: "result";
}

/** Stop now. The host kills the process if it is still alive two seconds later. */
export interface HostShutdown {
  type: "shutdown";
}

/** A frame the host writes to the plugin's stdin. */
export type HostMsg =
  | HostHello
  | HostSync
  | HostInvoke
  | HostRequest
  | HostEvent
  | HostResult
  | HostShutdown;

/** The first line, after `hello`. */
export interface PluginReady {
  type: "ready";
  /** The protocol the plugin was built for. */
  protocol: number;
  /** `events` to receive from the start. */
  subscribe?: string[];
  /** The plugin sends `done` after each invoke's handler returns. Workflows wait for it. */
  done?: boolean;
}

/** An `invoke` arrived (liveness). */
export interface PluginAck {
  type: "ack";
  /** The invoke's `id`. */
  id: string;
}

/** An `invoke`'s handler returned, so its effects have landed. */
export interface PluginDone {
  type: "done";
  /** The invoke's `id`. */
  id: string;
}

/** State and/or a temporary label/icon for one instance, or for every instance of an action. */
export interface PluginState extends StateUpdate {
  type: "state";
}

/**
 * A failure for `instance_id`, else the invoke `id`'s instance, else every instance of
 * `action_id`.
 */
export interface PluginError extends ErrorReport {
  type: "error";
}

/** The answer to a host `request`. */
export interface PluginResult extends CallResult {
  type: "result";
}

/** Ask the host to do something; answered by a `result` with the same `id`. */
export interface PluginHostCall {
  type: "host_call";
  /** Echoed by the host's `result`. */
  id: string;
  /** A `host_calls` value. */
  method: string;
  /** Method-specific arguments. */
  params?: unknown;
}

/** Replace the plugin's event subscriptions. */
export interface PluginSubscribe {
  type: "subscribe";
  /** `events` values; an empty list unsubscribes from everything. */
  events: string[];
}

/** The plugin's setup is no longer valid (an expired token…); the host asks the user again. */
export interface PluginSetupRequired {
  type: "setup_required";
  /** Shown to the user. */
  reason?: string;
}

/** A frame the plugin writes to its stdout. Nothing else may go to stdout; logs go to stderr. */
export type PluginMsg =
  | PluginReady
  | PluginAck
  | PluginDone
  | PluginState
  | PluginError
  | PluginResult
  | PluginHostCall
  | PluginSubscribe
  | PluginSetupRequired;

/** The first frame. */
export interface CompanionHello extends CompanionInfo {
  type: "hello";
}

/** Every `PING_EVERY_SECS`; answered `pong`. */
export interface CompanionPing {
  type: "ping";
}

/** The answer to a hub `request`. */
export interface CompanionResponse extends ActionResult {
  type: "response";
}

/** Live state from the app (the current tool, the active layer…), for the plugin. */
export interface CompanionEvent {
  type: "event";
  /** Companion-specific name. */
  event: string;
  /** Event-specific data. */
  payload?: unknown;
}

/** A frame a companion sends to the hub. */
export type CompanionMsg =
  | CompanionHello
  | CompanionPing
  | CompanionResponse
  | CompanionEvent;

/** `hello` was accepted; the link is up. */
export interface HubReady {
  type: "ready";
  /** The companion protocol the hub speaks. */
  protocol: number;
  /** The id the plugin knows this link by. */
  link: string;
  host_version: string;
}

/** `hello` was refused; the hub closes the socket after this. */
export interface HubRefused {
  type: "refused";
  /** Why, for the companion's log or UI. */
  reason: string;
}

/** The answer to `ping`. */
export interface HubPong {
  type: "pong";
}

/** Run an action in the app; answered by a `response` with the same `id`. */
export interface HubRequest {
  type: "request";
  id: string;
  /** Companion-specific action id. */
  action: string;
  params?: unknown;
}

/** A frame the hub sends to a companion. */
export type HubMsg =
  | HubReady
  | HubRefused
  | HubPong
  | HubRequest;

/** The first line. */
export interface ClientHello {
  type: "hello";
  protocol: number;
  /** The contents of `TOKEN_FILE`, trimmed. */
  token: string;
  /** Who is calling, for KnobDeck's log ("obs-script", "raycast"). */
  client?: string;
  /**
   * This client acts for an AI assistant (KnobDeck's MCP server). KnobDeck then lists,
   * opens and runs only profiles the user opened to assistants
   * (`ProfileSummary::assistant`).
   */
  assistant?: boolean;
}

/** Run a command; answered by a `response` with the same `id`. */
export interface ClientRequest {
  type: "request";
  id: string;
  /** A `commands` value. */
  cmd: string;
  params?: unknown;
}

/** A line a client sends. */
export type ClientMsg =
  | ClientHello
  | ClientRequest;

/** `hello` was accepted. */
export interface ServerReady {
  type: "ready";
  protocol: number;
  host_version: string;
  /** The `commands` this KnobDeck takes. */
  commands: string[];
}

/** `hello` was refused; the connection closes after this. */
export interface ServerRefused {
  type: "refused";
  reason: string;
}

/** The answer to a `request`. */
export interface ServerResponse {
  type: "response";
  id: string;
  ok: boolean;
  result?: unknown;
  error?: string;
}

/** A subscribed host event. */
export interface ServerEvent {
  type: "event";
  event: string;
  payload?: unknown;
}

/** A line KnobDeck sends. */
export type ServerMsg =
  | ServerReady
  | ServerRefused
  | ServerResponse
  | ServerEvent;
