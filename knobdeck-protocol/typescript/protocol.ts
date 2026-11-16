// Generated from the knobdeck-protocol crate (knobdeck-sdk repo). Do not edit: change the Rust
// types and run `KNOBDECK_BLESS=1 cargo test -p knobdeck-protocol --features schema`.

/** The newest protocol number. It moves only for a break (COMPAT.md). */
export const PROTOCOL = 2;
/** The oldest protocol number a host still loads. */
export const MIN_PROTOCOL = 2;

/** Optional host behaviour, listed in `hello.features`. */
export const FEATURES = ["state_match", "events", "notify", "app_state", "open_dial"] as const;
export type Feature = (typeof FEATURES)[number];

/** Host events a plugin can subscribe to. */
export const EVENTS = ["focus_changed", "dial_opened", "dial_closed", "profiles_changed"] as const;
export type EventName = (typeof EVENTS)[number];

/** Values of `invoke.trigger`. */
export const TRIGGERS = ["press", "hold_start", "hold_end", "adjust"] as const;
export type Trigger = (typeof TRIGGERS)[number];

/** Values of `request.method`. */
export const REQUESTS = ["query", "wheel"] as const;
export type RequestMethod = (typeof REQUESTS)[number];

/** Values of `host_call.method`. */
export const HOST_CALLS = ["send_keys", "notify", "app_state", "open_dial"] as const;
export type HostCallMethod = (typeof HOST_CALLS)[number];

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

/** Payload of the `dial_opened` event. */
export interface DialOpened {
  /** The profile the dial shows. */
  profile_id: string;
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
