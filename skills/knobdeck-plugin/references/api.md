# knobdeck-sdk API reference

`use knobdeck_sdk::{json, Action, Error, Plugin, Result, Value, Visual};`
`use knobdeck_sdk::input::{self, InputExt};`

## Plugin

| Method | |
|---|---|
| `Plugin::new(id, name)` | id: lowercase `a-z0-9-`; never change it after release |
| `.version(v)` `.author(v)` `.description(v)` `.icon(v)` | metadata (`icon`: Lucide name or `icons/<file>`) |
| `.platforms(&["windows", "macos", "linux"])` | OSes it runs on (default: all) |
| `.action(Action)` | add an action (duplicate ids panic) |
| `.query(id, \|q: &QueryCtx\| Ok(json!([{"value": .., "label": ..}])))` | options for `query_select` inputs; `q.inputs`, `q.installed_apps()` (os-apps, rescans) |
| `.setup(bin, page)` `.setup_inputs(vec![..])` `.setup_assets_dir(dir)` | a setup program (second `[[bin]]` calling `run_setup`) |
| `.on_ready(\|rt: &Runtime\| ..)` | once after the first sync; clone `rt` into your threads |
| `.subscribe(&[..])` `.on_event(\|e: &Event, rt\| ..)` | host events (none defined now) |
| `.on_shutdown(\|rt\| ..)` | the host kills the process 2 s later |
| `.ack_timeout_ms(n)` `.query_timeout_ms(n)` | raise host timeouts for slow handlers |
| `.run()` | start (never returns) |

## Action

| Constructor | Handlers |
|---|---|
| `Action::button(id, label)` | `.on_press(\|ctx\| ..)` |
| `Action::toggle(id, label)` | `.on_press`; `.state(value, label, icon)` ×N (first = default) |
| `Action::hold(id, label)` | `.on_hold_start`, `.on_hold_end`; `.released(label, icon)`, `.held(label, icon)` |
| `Action::slider(id, label, min, max)` | `.on_adjust` (`ctx.value` absolute or `ctx.delta`); `.step(n)`, `.unit("%")` |
| `Action::wheel(id, label)` | `.on_open(\|o: &OpenCtx\| Ok(vec![json!({"id": "1", "label": "One", "icon": "star"})]))` (max 16), `.on_child_press(\|ctx\| ..)` with `ctx.child_id` |

Common: `.icon(name)`, `.inputs(vec![..])`, `.platforms(..)`, `.volatile()` (re-run `initial` on sync and, with
`os-window`, on target-window changes), `.initial(state)`,
`.initial_with(|ic: &InitialCtx| Some(state))`, `.live(Duration, |ic| Ok(Visual))`.

## Inputs

`input::text(key, label)`, `textarea`, `checkbox`, `keybind`, `select(key, label, &[(value, label)])`,
`query_select(key, label, query_id, &[dependent input keys])`, `query_combobox(key, label, query_id)`.
Chain `.placeholder("…")`, `.default_value(v)`, `.with("showIf", json!({"key": "mode", "equals": "a"}))`.

## Ctx (handlers)

Fields: `instance_id`, `action_id`, `trigger` (`press`, `adjust`, `hold_start`, `hold_end`),
`state: Option<String>`, `child_id`, `value`, `delta`, `inputs`.

| Method | |
|---|---|
| `input_str(k)` `input_bool(k)` `input_f64(k)` | read inputs (also on `InitialCtx`, `QueryCtx`, `OpenCtx`, `SetupCtx`) |
| `set_state(v)` / `fan_out(v)` | this key / every key of this action |
| `fan_out_by(&["device"], v)` / `fan_out_visual_by(keys, Visual)` | every key of this action whose `keys` inputs equal this key's |
| `set_label(s)` `set_icon(s)` `set_visual(Visual)` `fan_out_visual(Visual)` | live visuals |
| `set_error(msg)` | show an error, keep running |
| `fail(msg)` | return this: report the error and stop the handler |
| `mark_setup_required(reason)` | the host stops the plugin and asks for setup |
| `log(msg)` `data_dir()` `runtime()` | |
| `send_keys(keys)` | the only host call; last resort |
| `focus_app(app)` | os-window: the app's frontmost window to the front (in this process) |
| `installed_apps()` | os-apps: cached `Vec<InstalledApp>` (`label`, `value`, `executable`, `working_dir`) |

## os (features `os-window`, `os-apps`)

`os::window::{target, foreground, list, focus, focus_app, watch, wait_for}`; `Window { id, pid,
app_id, app_name, title, bounds, monitor, state: WindowState { maximized, minimized, fullscreen,
topmost }, desktop_id }`. `os::apps::{installed, rescan}`. Linux: `os::linux::{bridge, kwin, gnome,
desktop, bus}`. `knobdeck_sdk::host_process_id()` is KnobDeck's pid.

`Visual::new().label(..).icon(..).state(..)`

## Runtime (background threads)

`rt.set_state(instance_id, v)`, `rt.notify(action_id, v)` (every key of an action),
`rt.notify_matching(action_id, &json!({"device": id}), v)` (every key of an action whose inputs hold
those values), `rt.set_visual(instance_id, Visual)`, `rt.notify_visual(action_id, Visual)`,
`rt.notify_visual_matching(action_id, &filter, Visual)`, `rt.set_error(..)`,
`rt.instances_of(action_id)`, `rt.instances_matching(action_id, &filter_map)`, `rt.inputs_of(instance_id)`, `rt.has_instances()`, `rt.subscribe(..)`,
`rt.mark_setup_required(..)`, `rt.log(..)`, `rt.data_dir()`.

## Free functions

`data_dir()`, `data_file(name)`, `load_env()`, `save_env(&[(k, v)])`, `env_or(k, default)`,
`open_url(url)`, `platform()` / `is_mac()` / `is_linux()`, `mark_setup_required(reason)`,
`run_setup(|ctx: &SetupCtx| ..)`, `log(msg)`.

## Sign-in (feature `oauth`)

`knobdeck_sdk::oauth::OAuth::new(token_url, client_id)`: `.authorize_pkce(auth_url, scopes, port, extra)`
(browser + `http://127.0.0.1:{port}/callback`), `.device_flow(device_url, form)`, `.access_token()`
(refreshed automatically; a refused refresh marks setup required). Token kept in the OS credential
store under `oauth-token.json` (via `secrets`).

## Secrets (feature `secrets`, on with `oauth`)

`knobdeck_sdk::secrets::{get(name) -> Option<String>, set(name, value) -> Result, delete(name)}`.
The plugin binary keeps them in the OS credential store (item "KnobDeck plugin <id>" / name); a setup
program writes a user-only hand-off file `name` in the data folder that the plugin moves into the store.
`env_or_stored(key)`: env var `key`, else the stored secret. `migrate_env(&[keys])` (plugin start-up,
after `load_env()`): moves those keys out of an old `.env` into the store.

## Files

`dist/<id>/manifest.json` and `actions.json` are generated by `knobdeck-plugin dev|build`; never
edit them by hand. `icons/` is copied into the package. Logs: `log.txt` in the data folder
(`~/Library/Application Support/KnobDeck/plugin-data/<id>` on macOS,
`%APPDATA%\KnobDeck\plugin-data\<id>` on Windows, `~/.config/KnobDeck/plugin-data/<id>` on Linux).
