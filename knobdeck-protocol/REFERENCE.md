# KnobDeck plugin protocol reference (v2)

<!-- Generated from the knobdeck-protocol crate (knobdeck-sdk repo). Do not edit: change the Rust types and run `KNOBDECK_BLESS=1 cargo test -p knobdeck-protocol --features schema`. -->

A plugin is a process KnobDeck starts. The two talk over the plugin's stdin and stdout, one JSON object per line (UTF-8, `\n`-terminated). Logs go to stderr; stdout carries frames only. Every frame has a `type`. Unknown types and unknown fields must be ignored, so new optional fields and new frame types can appear without a new protocol number; what may change is in `COMPAT.md`.

A session: the host sends `hello`; the plugin answers `ready`; the host sends `sync` with the placed instances, then `invoke`, `request` and `event` frames as the user works; `shutdown` ends it. A plugin built for protocol `p` runs on hosts that support `2..=2` today.

Machine-readable forms: `schema/host-to-plugin.json`, `schema/plugin-to-host.json` (JSON Schema) and `typescript/protocol.ts`.

## Names

- **Features (`hello.features`):** `state_match`
- **Events (`ready.subscribe`, `subscribe`, `event.event`):** `focus_changed`
- **Triggers (`invoke.trigger`):** `press`, `hold_start`, `hold_end`, `adjust`
- **Request methods (`request.method`):** `query`, `wheel`
- **Host calls (`host_call.method`):** `send_keys`

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
{"type":"hello","protocol":2,"plugin_id":"example-rust","data_dir":"/tmp/knobdeck/plugin-data/example-rust","host_version":"1.0.0","features":["state_match"],"host_pid":4242}
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
```

### `subscribe`

Replace the plugin's event subscriptions.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `events` | `string[]` | yes | `events` values; an empty list unsubscribes from everything. |

```json
{"type":"subscribe","events":["focus_changed"]}
```

### `setup_required`

The plugin's setup is no longer valid (an expired token…); the host asks the user again.

| Field | Type | Required | Meaning |
|---|---|---|---|
| `reason` | `string` | no | Shown to the user. |

```json
{"type":"setup_required","reason":"demo token expired"}
```
