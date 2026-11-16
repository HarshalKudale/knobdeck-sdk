# KnobDeck plugin protocol reference (v2)

<!-- Generated from the knobdeck-protocol crate (knobdeck-sdk repo). Do not edit: change the Rust types and run `KNOBDECK_BLESS=1 cargo test -p knobdeck-protocol --features schema`. -->

A plugin is a process KnobDeck starts. The two talk over the plugin's stdin and stdout, one JSON object per line (UTF-8, `\n`-terminated). Logs go to stderr; stdout carries frames only. Every frame has a `type`. Unknown types and unknown fields must be ignored, so new optional fields and new frame types can appear without a new protocol number; what may change is in `COMPAT.md`.

A session: the host sends `hello`; the plugin answers `ready`; the host sends `sync` with the placed instances, then `invoke`, `request` and `event` frames as the user works; `shutdown` ends it. A plugin built for protocol `p` runs on hosts that support `2..=2` today.

Machine-readable forms: `schema/*.json` (JSON Schema for each direction of both layers, and `payloads.json`) and `typescript/protocol.ts`.

## Names

- **Features (`hello.features`):** `state_match`, `events`, `notify`, `app_state`, `open_dial`, `companion_hub`
- **Events (`ready.subscribe`, `subscribe`, `event.event`):** `focus_changed`, `dial_opened`, `dial_closed`, `profiles_changed`, `companion_connected`, `companion_disconnected`, `companion_event`
- **Triggers (`invoke.trigger`):** `press`, `hold_start`, `hold_end`, `adjust`
- **Request methods (`request.method`):** `query`, `wheel`
- **Host calls (`host_call.method`):** `send_keys`, `notify`, `app_state`, `open_dial`, `companion_request`, `companion_links`

## Host → plugin

### `hello`

Always the first line.

The body of `hello`: who the plugin is and what the host can do.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `protocol` | `number` | yes | The newest protocol number the host speaks. |
| `plugin_id` | `string` | yes | The plugin's id. Action ids are made from it (`make_full_id` in the SDK). |
| `data_dir` | `string` | yes | A folder the plugin owns for its settings and caches. |
| `host_version` | `string` | yes | KnobDeck's version, for logs and bug reports. Check `features`, not this, before using optional behaviour. |
| `features` | `string[]` | no | The `features` this host has. |
| `host_pid` | `number` | no | The host's process id, so a plugin can leave KnobDeck's own windows out of "the target window". |

```json
{"type":"hello","protocol":2,"plugin_id":"example-rust","data_dir":"/tmp/knobdeck/plugin-data/example-rust","host_version":"1.0.0","features":["state_match","events","notify","app_state","open_dial","companion_hub"],"host_pid":4242}
```

### `sync`

The plugin's placed instances, sent after `ready` and whenever they change.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `instances` | `Instance[]` | yes | Every placed instance of this plugin's actions; replaces the previous list. |

```json
{"type":"sync","instances":[{"instance_id":"i1","action_id":"example-rust-mute-0a1b2c3d","inputs":{}}]}
```

### `invoke`

Run one instance. The plugin answers `ack` at once and `done` when the handler returns.

The body of `invoke`: run one placed instance.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | `string` | yes | Echoed by `ack` and `done`. |
| `instance_id` | `string` | yes | For a dynamic-wheel child: `"<wheel instance>::<child_id>"`. |
| `action_id` | `string` | yes | The full action id. |
| `trigger` | `string` | yes | A `triggers` value. |
| `inputs` | `unknown` | no | The instance's inputs; for `adjust`, plus `value` (absolute) and/or `delta`. |
| `child_id` | `string` | no | Set when a dynamic wheel's child button was pressed. |

```json
{"type":"invoke","id":"r1","instance_id":"i1","action_id":"example-rust-mute-0a1b2c3d","trigger":"press","inputs":{}}
{"type":"invoke","id":"r2","instance_id":"v1","action_id":"example-rust-volume-4e5f6a7b","trigger":"adjust","inputs":{"value":40}}
{"type":"invoke","id":"r3","instance_id":"w1::3","action_id":"example-rust-recent-8c9d0e1f","trigger":"press","inputs":{},"child_id":"3"}
```

### `request`

Ask the plugin for data; answered by a `result` with the same `id`.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | `string` | yes | Echoed by the answering `result`. |
| `method` | `string` | yes | A `requests` value. |
| `params` | `unknown` | no | Method-specific arguments. |

```json
{"type":"request","id":"q1","method":"query","params":{"query_id":"items","inputs":{"group":"fruit"}}}
```

### `event`

A host event the plugin subscribed to.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `event` | `string` | yes | An `events` value. |
| `payload` | `unknown` | no | Event-specific data. |

```json
{"type":"event","event":"focus_changed","payload":{"app":"code.exe"}}
{"type":"event","event":"dial_opened","payload":{"profile_id":"default"}}
{"type":"event","event":"dial_closed","payload":{}}
{"type":"event","event":"companion_connected","payload":{"link":"photoshop-1","instance":"6f1c2b9e-ps-2026","app":"photoshop","app_version":"27.1.0","name":"Photoshop 2026","version":"1.0.0","features":["layers"]}}
{"type":"event","event":"companion_event","payload":{"link":"photoshop-1","event":"tool_changed","payload":{"tool":"eraser"}}}
{"type":"event","event":"companion_disconnected","payload":{"link":"photoshop-1"}}
```

### `result`

The answer to a plugin's `host_call`.

`result`: the answer to a `request` (plugin → host) or a `host_call` (host → plugin).

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | `string` | yes | The `id` of the `request` or `host_call` this answers. |
| `ok` | `boolean` | yes | Whether it worked: then `value` is set, else `error`. |
| `value` | `unknown` | no | Present when `ok`. May be `null`, which is kept apart from "absent". |
| `error` | `string` | no | Present when not `ok`. Human-readable; not part of the contract. |

```json
{"type":"result","id":"h1","ok":true,"value":null}
{"type":"result","id":"h2","ok":false,"error":"not possible"}
{"type":"result","id":"c2","ok":true,"value":{"os":"linux","host_version":"1.0.0","dial_open":false}}
{"type":"result","id":"c5","ok":true,"value":{"id":"7","ok":true,"result":{"tool":"brush"}}}
```

### `shutdown`

Stop now. The host kills the process if it is still alive two seconds later.

No fields besides `type`.

```json
{"type":"shutdown"}
```

## Plugin → host

### `ready`

The first line, after `hello`.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `protocol` | `number` | yes | The protocol the plugin was built for. |
| `subscribe` | `string[]` | no | `events` to receive from the start. |
| `done` | `boolean` | no | The plugin sends `done` after each invoke's handler returns. Workflows wait for it. |

```json
{"type":"ready","protocol":2,"subscribe":["focus_changed"],"done":true}
```

### `ack`

An `invoke` arrived (liveness).

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | `string` | yes | The invoke's `id`. |

```json
{"type":"ack","id":"r1"}
```

### `done`

An `invoke`'s handler returned, so its effects have landed.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | `string` | yes | The invoke's `id`. |

```json
{"type":"done","id":"r1"}
```

### `state`

State and/or a temporary label/icon for one instance, or for every instance of an action.

The body of a `state` frame: a new state and/or a temporary label or icon, and who gets it.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `instance_id` | `string` | no | One instance. Wins over `action_id`. |
| `action_id` | `string` | no | Every instance of this action. |
| `match` | `Record<string, unknown>` | no | With `action_id`: only instances whose inputs hold these values (compared as strings; a missing input counts as `""`). Needs the `state_match` feature. |
| `state` | `string` | no | The new state. A frame without it leaves state and error alone. |
| `label` | `string` | no | A temporary label shown instead of the configured one. |
| `icon` | `string` | no | A temporary icon name shown instead of the configured one. |

```json
{"type":"state","instance_id":"i1","state":"muted"}
{"type":"state","action_id":"example-rust-mute-0a1b2c3d","state":"muted","label":"Muted","icon":"mic-off"}
{"type":"state","action_id":"example-rust-device-volume-2b3c4d5e","match":{"device":"a"},"state":"40"}
```

### `error`

A failure for `instance_id`, else the invoke `id`'s instance, else every instance of
`action_id`.

The body of an `error` frame: what failed, and who gets it.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `instance_id` | `string` | no | One instance. Wins over `action_id`. |
| `action_id` | `string` | no | Every instance of this action. |
| `match` | `Record<string, unknown>` | no | With `action_id`: only instances whose inputs hold these values (compared as strings; a missing input counts as `""`). Needs the `state_match` feature. |
| `id` | `string` | no | The invoke this error ends, when there is one. |
| `message` | `string` | yes | Shown to the user on the action. |

```json
{"type":"error","instance_id":"i1","message":"demo error"}
{"type":"error","action_id":"example-rust-mute-0a1b2c3d","match":{"device":"a"},"id":"r1","message":"demo error"}
```

### `result`

The answer to a host `request`.

`result`: the answer to a `request` (plugin → host) or a `host_call` (host → plugin).

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | `string` | yes | The `id` of the `request` or `host_call` this answers. |
| `ok` | `boolean` | yes | Whether it worked: then `value` is set, else `error`. |
| `value` | `unknown` | no | Present when `ok`. May be `null`, which is kept apart from "absent". |
| `error` | `string` | no | Present when not `ok`. Human-readable; not part of the contract. |

```json
{"type":"result","id":"q1","ok":true,"value":["apple","pear"]}
{"type":"result","id":"q2","ok":false,"error":"unknown query \"nope\""}
```

### `host_call`

Ask the host to do something; answered by a `result` with the same `id`.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | `string` | yes | Echoed by the host's `result`. |
| `method` | `string` | yes | A `host_calls` value. |
| `params` | `unknown` | no | Method-specific arguments. |

```json
{"type":"host_call","id":"c1","method":"send_keys","params":{"keys":"ctrl+m"}}
{"type":"host_call","id":"c2","method":"notify","params":{"message":"Copied","level":"info"}}
{"type":"host_call","id":"c3","method":"app_state","params":{}}
{"type":"host_call","id":"c4","method":"open_dial","params":{"profile_id":"streaming"}}
{"type":"host_call","id":"c5","method":"companion_request","params":{"link":"photoshop-1","action":"set-tool","params":{"tool":"brush"},"timeout_ms":5000}}
{"type":"host_call","id":"c6","method":"companion_links","params":{}}
```

### `subscribe`

Replace the plugin's event subscriptions.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `events` | `string[]` | yes | `events` values; an empty list unsubscribes from everything. |

```json
{"type":"subscribe","events":["focus_changed"]}
{"type":"subscribe","events":["dial_opened","dial_closed","profiles_changed"]}
```

### `setup_required`

The plugin's setup is no longer valid (an expired token…); the host asks the user again.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `reason` | `string` | no | Shown to the user. |

```json
{"type":"setup_required","reason":"demo token expired"}
```

## Companion → hub (layer 2)

A companion (an add-on inside another app) dials KnobDeck's hub over a WebSocket on loopback: `ws://127.0.0.1:<port>/companion`, the port and a token in `companion-hub.json` in KnobDeck's data folder (ports 32150–32154). Its `hello` names the plugin it belongs to; the plugin sees it through the `companion_*` events and host calls. Companion protocol 1; same compatibility rules as the plugin protocol.

### `hello`

The first frame.

The body of a companion's `hello`: who it is and which plugin it belongs to.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `protocol` | `number` | yes | The companion protocol the companion speaks. |
| `plugin` | `string` | yes | The KnobDeck plugin this companion belongs to (its manifest id). |
| `instance` | `string` | yes | Stable per install / profile / window, so a reconnect replaces the old link. |
| `app` | `string` | yes | The app it lives in (`photoshop`, `blender`, `chrome`…). |
| `app_version` | `string` | no |  |
| `name` | `string` | no | What the user sees ("Photoshop 2026", "Chrome — Work"). |
| `version` | `string` | no | The companion's own version. |
| `features` | `string[]` | no | Optional companion behaviour, for the plugin to check. |
| `token` | `string` | no | `Discovery::token`, when the companion could read the discovery file. |

```json
{"type":"hello","protocol":1,"plugin":"photoshop","instance":"6f1c2b9e-ps-2026","app":"photoshop","app_version":"27.1.0","name":"Photoshop 2026","version":"1.0.0","features":["layers"],"token":"3b6e…"}
{"type":"hello","protocol":1,"plugin":"browser-plugin","instance":"a4d0-chrome-work","app":"chrome","features":[]}
```

### `ping`

Every `PING_EVERY_SECS`; answered `pong`.

No fields besides `type`.

```json
{"type":"ping"}
```

### `response`

The answer to a hub `request`.

The body of a companion's `response`: how an action went.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | `string` | yes | The `request` this answers. |
| `ok` | `boolean` | yes |  |
| `result` | `unknown` | no | When `ok`. |
| `error` | `string` | no | When not `ok`: a message for the user. |
| `unsupported` | `boolean` | no | When not `ok` because the app has no API for this action. |

```json
{"type":"response","id":"7","ok":true,"result":{"tool":"brush"}}
{"type":"response","id":"8","ok":false,"error":"No document is open"}
{"type":"response","id":"9","ok":false,"unsupported":true}
```

### `event`

Live state from the app (the current tool, the active layer…), for the plugin.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `event` | `string` | yes | Companion-specific name. |
| `payload` | `unknown` | no | Event-specific data. |

```json
{"type":"event","event":"tool_changed","payload":{"tool":"eraser"}}
```

## Hub → companion (layer 2)

### `ready`

`hello` was accepted; the link is up.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `protocol` | `number` | yes | The companion protocol the hub speaks. |
| `link` | `string` | yes | The id the plugin knows this link by. |
| `host_version` | `string` | yes |  |

```json
{"type":"ready","protocol":1,"link":"photoshop-1","host_version":"1.0.0"}
```

### `refused`

`hello` was refused; the hub closes the socket after this.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `reason` | `string` | yes | Why, for the companion's log or UI. |

```json
{"type":"refused","reason":"wrong token"}
```

### `pong`

The answer to `ping`.

No fields besides `type`.

```json
{"type":"pong"}
```

### `request`

Run an action in the app; answered by a `response` with the same `id`.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | `string` | yes |  |
| `action` | `string` | yes | Companion-specific action id. |
| `params` | `unknown` | no |  |

```json
{"type":"request","id":"7","action":"set-tool","params":{"tool":"brush"}}
```

## Payloads

Typed contents of `event.payload`, `host_call.params` and `result.value`; see the event and host call names above.

### `ActionResult`

The body of a companion's `response`: how an action went.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `id` | `string` | yes | The `request` this answers. |
| `ok` | `boolean` | yes |  |
| `result` | `unknown` | no | When `ok`. |
| `error` | `string` | no | When not `ok`: a message for the user. |
| `unsupported` | `boolean` | no | When not `ok` because the app has no API for this action. |

### `AppState`

The answer to the `app_state` host call.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `os` | `string` | yes | `windows`, `macos` or `linux`. |
| `host_version` | `string` | yes |  |
| `dial_open` | `boolean` | yes | Whether the dial is on screen. |

### `DialOpened`

Payload of the `dial_opened` event.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `profile_id` | `string` | yes | The profile the dial shows. |

### `Discovery`

What the hub writes to `DISCOVERY_FILE`.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `protocol` | `number` | yes |  |
| `port` | `number` | yes | The hub's loopback port. |
| `path` | `string` | yes | `WS_PATH`. |
| `token` | `string` | yes | Sent back in `hello.token`. A companion that can't read files (a browser extension) connects without it, from an extension origin. |

### `Link`

One connected companion, as the plugin sees it (`companion_connected`, `companion_links`).

| Field | Type | Required | Meaning |
|---|---|---|---|
| `link` | `string` | yes | The hub's id for this link; pass it to `companion_request`. |
| `instance` | `string` | yes |  |
| `app` | `string` | yes |  |
| `app_version` | `string` | no |  |
| `name` | `string` | no |  |
| `version` | `string` | no |  |
| `features` | `string[]` | no |  |

### `LinkEvent`

Payload of `companion_event`.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `link` | `string` | yes |  |
| `event` | `string` | yes | The companion's event name. |
| `payload` | `unknown` | no |  |

### `LinkGone`

Payload of `companion_disconnected`.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `link` | `string` | yes |  |

### `LinkRequest`

`params` of the `companion_request` host call.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `link` | `string` | no | A link from `companion_connected` / `companion_links`; absent: the most recently connected link of this plugin. |
| `action` | `string` | yes |  |
| `params` | `unknown` | no |  |
| `timeout_ms` | `number` | no | How long to wait for the companion (default 5000, at most 30000). |

### `Notify`

`params` of the `notify` host call.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `message` | `string` | yes | One short sentence. The host may shorten long text. |
| `level` | `string` | no | `info` (default) or `error`. Unknown values read as `info`. |

### `OpenDial`

`params` of the `open_dial` host call.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `profile_id` | `string` | no | The dial profile to show. Absent: the one the user's auto-open rules pick, as the dial keybind does. |
