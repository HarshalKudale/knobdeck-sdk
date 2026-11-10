# knobdeck-sdk

Declarative Rust SDK for [KnobDeck](https://knobdeck.harshalkudale.com) plugins. A plugin is a
native executable: you declare actions with builders and write handlers; the SDK speaks the stdio
protocol, and the `knobdeck-plugin` CLI creates, builds, tests and packages it.

## Quick start

```bash
cargo install --git https://github.com/HarshalKudale/knobdeck-sdk --bin knobdeck-plugin   # once

knobdeck-plugin new acme-player --name "Acme Player"   # a working crate with three example actions
cd acme-player
knobdeck-plugin dev        # debug build -> dist/acme-player/ (use "Load Unpacked" in KnobDeck)
knobdeck-plugin drive      # run it over the real protocol, without KnobDeck
knobdeck-plugin dev --watch   # rebuild on change; KnobDeck restarts the plugin
knobdeck-plugin build      # release build -> self-contained dist/<id>/ (universal on macOS)
knobdeck-plugin package    # dist/<id>.knobdeck
```

`new` takes `--os windows|macos|linux` for a one-OS plugin, and `--id`, `--author`,
`--description`. Inside a Cargo workspace whose `[workspace.dependencies]` has `knobdeck-sdk`, the
new crate uses it (add the folder to `members`). Otherwise it depends on this repo:

```toml
[dependencies]
knobdeck-sdk = { git = "https://github.com/HarshalKudale/knobdeck-sdk.git" }
```

```rust
use knobdeck_sdk::{Action, Plugin};

fn main() {
    Plugin::new("demo", "Demo")
        .version(env!("CARGO_PKG_VERSION"))
        .action(Action::button("ping", "Ping").on_press(|ctx| {
            ctx.log("pong");
            Ok(())
        }))
        .run()
}
```

## Build a plugin with an AI agent

Every crate made by `knobdeck-plugin new` contains the **knobdeck-plugin skill**
(`.claude/skills/knobdeck-plugin/`, plain Markdown) plus an `AGENTS.md` pointing at it, so a
coding agent opened in that folder (Claude Code, Codex, Cursor…) knows how to build, test and
package a plugin, and how to reach the target app without faking keystrokes. Ask it, e.g.,
*"Make this a plugin that controls Home Assistant lights"*.

To have the skill everywhere instead: `knobdeck-plugin skill ~/.claude/skills/knobdeck-plugin`.
The source is [`skills/knobdeck-plugin/`](skills/knobdeck-plugin/SKILL.md), with an API reference and
three complete example plugins (a web API, a local app with a token, a macOS AppleScript app).

## Testing with `drive`

`knobdeck-plugin drive` starts `dist/<id>/` the way KnobDeck does (hello → ready → sync one
instance of every action), prints every message the plugin sends, and shuts it down. Give it a
steps file to exercise actions:

```json
[
  {"press": "greet", "inputs": {"name": "Ada"}},
  {"adjust": "volume", "value": 40},
  {"hold": "push-to-talk", "ms": 300},
  {"query": "playlists"},
  {"open": "recent", "inputs": {}},
  {"press": "recent", "child": "3"},
  {"wait": 1500}
]
```

Host calls (`send_keys`, `focus_app`, `installed_apps`) get harmless stub answers and are printed;
a `send_keys` is flagged, since it means the action fell back to keystrokes. It fails when the
plugin writes anything but protocol messages to stdout.

## Action types

| Builder | Handlers | Notes |
|---|---|---|
| `Action::button(id, label)` | `on_press` | |
| `Action::toggle(id, label)` | `on_press` | `.state(value, label, icon)` ×N; first is the default; `.initial(..)` / `.initial_with(..)` override |
| `Action::hold(id, label)` | `on_hold_start`, `on_hold_end` | `.released(label, icon)`, `.held(label, icon)` |
| `Action::slider(id, label, min, max)` | `on_adjust` | `ctx.value` (absolute) or `ctx.delta`; `.step()`, `.unit()` |
| `Action::wheel(id, label)` | `on_open` → `Vec<slice>`, `on_child_press` | max 16 buttons (extra are truncated) |

`.volatile()` re-computes the initial state on every sync / focus change. `.icon()` and `.inputs()` apply to all.

`.live(every, |ic| Ok(Visual::new().label(..).icon(..)))` gives any action Stream Deck-style live content: it runs every `every` for each placed instance (`ic.input_str(..)` reads its inputs) and pushes the label/icon when they change (a newly placed instance within a second). Set `.state(..)` too to keep a toggle or slider in step with something changed outside KnobDeck. The action and its press handler stay as they are, and while the plugin is not running the tile shows its configured icon and label.

An icon is a [Lucide](https://lucide.dev/icons) name (`"play"`), an image file in the plugin's `icons/` folder (`"mute.svg"`), or a `data:` URI. Emoji and `icon:` pack refs are not rendered by the host.

## Handlers and context

Handlers are `Fn(&Ctx) -> knobdeck_sdk::Result`. `?` works on any `std::error::Error`; an error that
escapes a handler is logged and sent to the host as an error frame. Invokes for the same instance
run in order on that instance's worker thread (after the host ack is sent); different instances,
queries and events run in parallel, so blocking I/O in a handler is fine. When a handler returns
the runtime sends `done`, and a workflow runs its next step only then, so finish the action's
effect (keys sent, window moved) before returning.

```rust
ctx.set_state("on");            // this instance        ctx.fan_out("on");  // every instance
ctx.set_label("CPU 20%");       // live visuals          ctx.set_visual(Visual::new().label("x").icon("y"));
ctx.set_error("msg");           // report, keep going    return ctx.fail("msg");  // report + stop
ctx.input_str("url"); ctx.input_bool("on"); ctx.input_f64("n");
ctx.state; ctx.fg_process; ctx.fg_title; ctx.trigger; ctx.child_id; ctx.value; ctx.delta;
ctx.send_keys("ctrl+m")?; ctx.focus_app("code.exe")?; ctx.installed_apps()?;
ctx.mark_setup_required("Discord token expired");   // host stops the plugin, asks for setup
ctx.data_dir();                 // plugin-data/<id> — survives updates (see Data and logs)
```

## Windows, macOS and Linux

The host runs on Windows, macOS and Linux and spawns the binary built for its OS.

```rust
Plugin::new("mac-tools", "Mac Tools").platforms(&["macos"])        // whole plugin: one OS
Action::button("lock", "Lock").platforms(&["windows"])              // one action: one OS
knobdeck_sdk::platform();  knobdeck_sdk::is_mac();  knobdeck_sdk::is_linux(); // "windows" | "macos" | "linux"
ctx.send_keys("mod t")?;   // `mod` is ⌘ on macOS and Ctrl on Windows/Linux; `cmd`/`win` is ⌘ / Win / Super
```

Without `platforms` a plugin or action is offered on every OS. Elsewhere the host hides it from
pickers, and won't start a plugin whose `platforms` exclude its OS. `fg_process` is the exe name on
Windows (`code.exe`), the bundle id on macOS (`com.microsoft.VSCode`) and the window class /
Wayland app id on Linux (`code`, `org.gnome.Nautilus`). `focus_app` takes an exe
name/path on Windows and a bundle id, `.app` path or app name on macOS.

`build` writes `runtime.executables` (and `setup.executables`) keyed by OS. Building on Windows
and then on macOS into the same `dist/<id>/` keeps both entries and both binaries, so
`package` produces one `.knobdeck` that runs on both.

**Keystrokes are a last resort.** `send_keys` is for actions that *are* typing, or when no OS API,
app interface (local API, scripting, Accessibility menu press) or system app exists. Shortcuts can
be captured by the app in focus, remapped or turned off by the user. If you must use one, prefer
the user's configured system binding and comment why no API fits.

## Inputs, queries

```rust
use knobdeck_sdk::input::{self, InputExt};
Action::button("open", "Open").inputs(vec![
    input::text("url", "URL").placeholder("https://…"),
    input::checkbox("new_tab", "New tab").default_value(true),
    input::select("mode", "Mode", &[("a", "A"), ("b", "B")]),
    input::query_select("scene", "Scene", "scenes", &[]),   // live options from a query
])
// Any raw `json!({...})` descriptor works too (showIf, etc.).

Plugin::new(..).query("scenes", |ctx| Ok(json!([{"value": "1", "label": "Main"}])))
```

## Background work

`on_ready` fires once after the first sync with a cloneable, `Send` [`Runtime`]; move it into your
own threads to push `set_state` / `notify` / `notify_visual` / `set_error`. `subscribe(&["focus_changed"])`
+ `on_event` receive host events. `on_shutdown` runs when the host stops the plugin (stdin closes
or `shutdown` arrives); the host kills it 2 s later.

## Data and logs

Keep tokens and settings in `knobdeck_sdk::data_dir()` (`%APPDATA%\KnobDeck\plugin-data\<id>` on
Windows, `~/Library/Application Support/KnobDeck/plugin-data/<id>` on macOS) (or `knobdeck_sdk::data_file("token.json")`,
which also moves a file left in the plugin folder by an older version). The folder survives
updates and reinstalls. `log()` and anything printed (even `println!`) go to the plugin's
`log.txt` in that folder; stdout is reserved for the protocol.

## Setup program

A second Cargo `[[bin]]` that calls `run_setup`; declare it on the plugin with `.setup(bin, page)`:

```rust
fn main() {
    knobdeck_sdk::run_setup(|ctx| {
        let host = ctx.input_str("host");
        if host.is_empty() { return ctx.fail("Host cannot be empty"); }
        std::fs::write(ctx.data_dir().join(".env"), format!("HOST={host}\n"))?;
        Ok(())
    })
}
```

At runtime the plugin calls `knobdeck_sdk::load_env()` to read that `.env`. Declare collected fields with
`.setup_inputs(vec![..])` on the plugin and pass `None` as the page: the host draws the form.
`knobdeck_sdk::open_url(url)` opens a link in the default browser.

## Signing in to a web service (feature `oauth`)

`knobdeck-sdk = { ..., features = ["oauth"] }` adds `knobdeck_sdk::oauth::OAuth`:

- `authorize_pkce(auth_url, scopes, port, extra)`: browser sign-in with PKCE, answered on
  `http://127.0.0.1:{port}/callback` (register that redirect URI). No client secret is shipped.
- `device_flow(device_url, form)`: for services without PKCE for desktop apps (Twitch); opens
  the activation page and waits for approval.
- `access_token()`: the saved token, refreshed when it is about to expire. A refused refresh calls
  `mark_setup_required`, so the host asks the user to sign in again.

Run the sign-in in the setup program; the token is kept as `oauth-token.json` in the data folder.

## Layout

`src/runtime.rs` (stdio protocol v2), `src/plugin.rs` (builders), `src/codegen.rs` (manifest/actions),
`src/bin/knobdeck-plugin.rs` (CLI; `knobdeck-plugin/scaffold.rs` for `new`/`skill`,
`knobdeck-plugin/drive.rs` for `drive`), `templates/plugin/` (what `new` writes),
`skills/knobdeck-plugin/` (the agent skill), `tests/protocol.rs` (drives the demo over pipes).

## Protocol v2

One JSON object per line. Host → plugin on stdin: `hello` (first), `sync`, `invoke`, `request`
(`query` / `wheel`), `event`, `result` (answer to `host_call`), `shutdown`. Plugin → host on stdout:
`ready` (first), `ack`, `done`, `state`, `error`, `result` (answer to `request`), `host_call`,
`subscribe`, `setup_required`. Unknown types are ignored. `ready` with `"done": true` promises a
`done {id}` after each invoke's handler returns; the host then holds a workflow step until it. The manifest declares `"protocol": 2`.
