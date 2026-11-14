---
name: knobdeck-plugin
description: Build, test and package a KnobDeck plugin in Rust with knobdeck-sdk. Use when the user wants KnobDeck (the radial dial and button deck app) to control an app, device or web service, wants a new dial/deck action, or asks to create, extend, debug or package a KnobDeck plugin.
---

# Building a KnobDeck plugin

A KnobDeck plugin is a native executable that KnobDeck starts and talks to over stdin/stdout
(one JSON message per line). `knobdeck-sdk` hides the protocol: you declare actions with builders
and write handlers. The `knobdeck-plugin` CLI builds, assembles, tests and packages it.

## Workflow

1. **Find the route first.** Before writing code, decide how each action will reach the target
   app (see "Choosing the route"). Read the app's docs for a local API, CLI, scripting dictionary
   or socket. Tell the user which route you picked and why.
2. **Scaffold:** `knobdeck-plugin new <folder> --name "<Display Name>"` (add `--os macos` etc. for a
   one-OS plugin). It writes a working crate: `Cargo.toml`, `src/main.rs` with three example
   actions, `readme.md`, `.gitignore`, and this skill. Install the CLI first if `knobdeck-plugin`
   isn't on PATH: `cargo install --git https://github.com/HarshalKudale/knobdeck-sdk --bin knobdeck-plugin`.
3. **Replace the examples** in `src/main.rs` with the real actions.
4. **Build:** `knobdeck-plugin dev` (debug build into `dist/<id>/`). Fix every compiler warning.
5. **Test over the protocol:** `knobdeck-plugin drive` starts the built plugin as KnobDeck would,
   places one instance of every action and prints what comes back (initial states, live labels,
   errors). Then write a steps file to press keys, turn sliders, run queries and open wheels
   (see "Testing"). Only drive actions that are safe to run on this machine; ask before anything
   that changes system state (locking the screen, sending messages, deleting things).
6. **Try it in KnobDeck:** Plugins → Load Unpacked → `dist/<id>/`. `knobdeck-plugin dev --watch`
   rebuilds on save and KnobDeck restarts the plugin.
7. **Ship:** `knobdeck-plugin build` (release; universal arm64 + x86_64 on macOS) then
   `knobdeck-plugin package` → `dist/<id>.knobdeck`, which KnobDeck installs by double-click or
   drag-and-drop.

## Choosing the route (most important rule)

Never implement an action as a keyboard shortcut when a stable route exists. The app in focus can
swallow the chord, users remap shortcuts, keyboard layouts move keys, and the dial or deck can
send the chord to the wrong window. Use, in this order:

1. **An OS API.** Windows: Win32 / COM / WinRT (`windows` crate). macOS: public frameworks
   (CoreAudio, CoreGraphics, AppKit, Accessibility `AXUIElement`). Linux: D-Bus services (MPRIS,
   logind, NetworkManager, portals) and the kernel (evdev, uinput).
2. **A long-stable private OS entry point**, resolved with `dlsym` / `GetProcAddress` at call time
   so its absence is an error, never a crash at launch.
3. **The app's own interface:** a local HTTP/websocket API (OBS, VLC, Elgato lights), an IPC
   pipe or socket (Discord RPC), a CLI, its AppleScript dictionary (`osascript`), a companion
   extension, or pressing its menu item through Accessibility.
4. **Launching a system tool or URL** (`open -b <bundle id>`, `ms-settings:`, an app's URL scheme).
5. **A keystroke**, only when none of the above exists. Comment why next to it, prefer chords
   the OS reserves, and use `mod` (⌘ on macOS, Ctrl elsewhere) for cross-platform app shortcuts.

Keystrokes are right when the action *is* typing (a text snippet) or for media/volume keys.

## Plugin skeleton

```rust
use knobdeck_sdk::input::{self, InputExt};
use knobdeck_sdk::{json, Action, Error, Plugin, Result, Value, Visual};

fn main() {
    Plugin::new("acme-player", "Acme Player")          // id: lowercase a-z0-9 and "-", never changed later
        .version(env!("CARGO_PKG_VERSION"))
        .author("Your Name")
        .description("Play, pause and rate tracks in Acme Player.")
        .icon("music")                                   // Lucide icon name, or a file in icons/
        .action(Action::button("play-pause", "Play / Pause").icon("play").on_press(|ctx| {
            acme_api("POST", "/player/toggle")?;
            Ok(())
        }))
        .run()
}
```

Action ids (`"play-pause"`) are saved in users' layouts: never rename one after release.

## Action types

| Builder | Handler(s) | Notes |
|---|---|---|
| `Action::button(id, label)` | `on_press` | |
| `Action::toggle(id, label)` | `on_press` | `.state(value, label, icon)` ×N, first is default; `ctx.set_state(..)` switches |
| `Action::hold(id, label)` | `on_hold_start`, `on_hold_end` | `.released(label, icon)`, `.held(label, icon)` |
| `Action::slider(id, label, min, max)` | `on_adjust` | `ctx.value` (absolute) or `ctx.delta`; `.step()`, `.unit()` |
| `Action::wheel(id, label)` | `on_open` → `Vec<Value>` slices, `on_child_press` | max 16 slices; `ctx.child_id` says which |

On any action: `.icon(..)`, `.inputs(vec![..])`, `.platforms(&["macos"])`, `.volatile()` (re-run
the initial state on every sync, and with the `os-window` feature whenever the target window changes), `.initial(..)` / `.initial_with(|ic| ..)`, and
`.live(every, |ic| Ok(Visual::new().label(..).icon(..).state(..)))` for content that updates
(now playing, a counter, a gauge). `references/api.md` has the full API.

## Handlers

Handlers are `Fn(&Ctx) -> knobdeck_sdk::Result`. `?` works on any `std::error::Error`; return
`ctx.fail("message")` (or `Err(Error::msg(..))`) for an error the user should see on the key.
Blocking I/O is fine: invokes for the same key run in order on their own worker thread.

```rust
ctx.input_str("url"); ctx.input_bool("new_tab"); ctx.input_f64("step");
ctx.state; ctx.trigger; ctx.child_id; ctx.value; ctx.delta;
ctx.set_state("on"); ctx.fan_out("on");            // this key / every key of the action
ctx.fan_out_by(&["device"], v);                     // every key of the action on the same device
ctx.set_label("CPU 20%"); ctx.set_icon("cpu"); ctx.set_visual(Visual::new().label("x"));
ctx.set_error("Acme isn't running");                // show an error, keep going
ctx.mark_setup_required("Sign in again");           // host asks the user to run setup
ctx.log("…");                                       // to the plugin's log.txt
```

**Which instances get a state.** The same action can sit on the deck and the dial at once, so pick
by what the state belongs to: the whole app or account (Discord mute) → `fan_out` / `rt.notify`;
one target picked in the inputs (a device, a user, a light) → `fan_out_by(&["device"], v)` /
`rt.notify_matching`, naming only the inputs that pick the target (not a step size); only the
pressed key (a hold's own phase) → `set_state`.

**stdout is the protocol.** Never print to it yourself; `println!` is redirected to the log, but
don't rely on writing protocol lines by hand.

## Inputs and queries

```rust
.inputs(vec![
    input::text("url", "URL").placeholder("https://…"),
    input::checkbox("new_tab", "New tab").default_value(true),
    input::select("mode", "Mode", &[("a", "A"), ("b", "B")]),
    input::query_select("playlist", "Playlist", "playlists", &[]), // options from a query
])
// on the Plugin:
.query("playlists", |q| Ok(json!([{"value": "1", "label": "Favourites"}])))
```

Prefer live queries (pick a playlist from a list) over asking users to paste ids.

## Settings, tokens and sign-in

- Data that must survive updates goes in `knobdeck_sdk::data_dir()` / `data_file("settings.json")`.
- Passwords, API keys and tokens go in the OS credential store with the `secrets` feature:
  `secrets::set(name, value)` in setup, `secrets::get(name)` in the plugin. Never in `.env` or a file.
- A one-time setup (host, API key) is a second binary calling `knobdeck_sdk::run_setup`, declared
  with `.setup("<bin>", None)` + `.setup_inputs(vec![..])`; the host draws the form. The plugin reads
  it with `knobdeck_sdk::load_env()` and `env_or("KEY", "")` (non-secret settings only).
- Web sign-in: the `oauth` feature (`OAuth::authorize_pkce`, `device_flow`, `access_token`). Never
  ship a client secret in the binary.

## Background work and events

`.on_ready(|rt| { let rt = rt.clone(); std::thread::spawn(move || loop { … rt.notify("action-id", "on"); }); Ok(()) })`
for pushing state from your own thread (a websocket listener, a poller). `.subscribe(&[..])`
+ `.on_event(..)` for host events. `.on_shutdown(..)` to clean up; the host kills the process 2 s
after asking it to stop.

## Windows, macOS and Linux

Without `.platforms(..)` a plugin or action is offered everywhere. Put OS differences behind one
`#[cfg(target_os = "…")]` module per concern so the action set stays the same on every OS. Build
on each OS into the same `dist/<id>/` and `package` produces one `.knobdeck` holding every OS's
binary.

## OS context (the window, installed apps)

The host sends no OS context. Read it in the plugin, only when an action needs it, through
`knobdeck_sdk::os` with the SDK features `os-window` / `os-apps`:
`os::window::target()` (the frontmost window that isn't KnobDeck's: `app_id`, `title`, `state`,
`bounds`), `os::window::list()`, `os::window::focus(&w.id)`, `os::window::focus_app(app, hint)`,
`os::apps::installed()`. `app_id` is the exe name on Windows, the bundle id on macOS and the
window class / app id on Linux. Don't send keystrokes to bring an app forward: use `focus_app`.

## Testing

`knobdeck-plugin drive` with no argument is a smoke test: hello → ready → sync one instance of
each action → wait → shutdown. With a steps file it runs your steps after the sync:

```json
[
  {"press": "play-pause"},
  {"press": "open-url", "inputs": {"url": "https://example.com"}},
  {"adjust": "volume", "value": 40},
  {"hold": "push-to-talk", "ms": 300},
  {"query": "playlists", "inputs": {}},
  {"open": "recent-tracks"},
  {"press": "recent-tracks", "child": "3"},
  {"wait": 1500}
]
```

`knobdeck-plugin drive steps.json`. Every message the plugin sends is printed (`state`, `error`,
`result`, `host_call`); `send_keys`, the only host call, is answered with a harmless stub and
printed, so you can see when a plugin falls back to keystrokes. A non-JSON line
on stdout fails the run. Check that each action produced the state or label you expect, and that
errors are clear sentences a user can act on ("Acme isn't running", not "connection refused (os
error 61)").

## Before you finish

- `cargo build` has no warnings; `knobdeck-plugin drive` shows no unexpected errors.
- Every action works when the target app isn't running: a clear error, never a hang or panic.
- No keystroke without a comment saying why no API fits.
- `readme.md` says what each action does and what the user must set up.
- Bump `version` in Cargo.toml for every change you ship.

See `references/examples.md` for three complete plugins: a web API, a local HTTP/websocket app
and a macOS AppleScript app.
