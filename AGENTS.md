# knobdeck-sdk: working notes for agents

- To **build a plugin**, follow `skills/knobdeck-plugin/SKILL.md`; start with `knobdeck-plugin new`.
- To **change the SDK**: the protocol is `src/runtime.rs` (v2, one JSON object per line), the
  builders `src/plugin.rs`, the generated manifest/actions `src/codegen.rs`, the CLI
  `src/bin/knobdeck-plugin.rs` (+ `knobdeck-plugin/scaffold.rs`, `knobdeck-plugin/drive.rs`).
- `knobdeck-os/` is the OS crate (windows, focus, the window watcher, installed apps) shared by
  the KnobDeck host and plugins (`knobdeck_sdk::os`, features `os-window` / `os-apps`). It owns the
  KWin scripts and the GNOME Shell extension (`knobdeck-os/src/linux/`): keep `kwin.js` and the
  extension's commands in step, and bump the extension's `version` (metadata.json and `VERSION`)
  when it changes. Type-check every OS: `cargo check --target x86_64-pc-windows-msvc` and
  `--target aarch64-apple-darwin` from Linux.
- `knobdeck-plugin new` embeds `templates/plugin/*` and `skills/knobdeck-plugin/**` with
  `include_str!`: when you add a file to either, add it to `scaffold.rs` too.
- Keep the skill true: every API it names must exist, and `references/examples.md` must compile
  (scaffold a crate per example and `cargo build` it before changing the examples).
- First-party plugins (`../knobdeck-plugins`) depend on this repo's git master: push SDK changes
  before relying on them there.
- Before finishing: `cargo test` (includes `tests/protocol.rs`, which drives the demo over pipes)
  with no warnings.
