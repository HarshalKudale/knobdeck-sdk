# Compatibility rules

What may change in KnobDeck's plugin protocol, package format and SDK without breaking plugins that
already exist. Written 2026-11-16 for protocol v2. The frames themselves are pinned in
[`knobdeck-protocol/fixtures/protocol-v2.jsonl`](knobdeck-protocol/fixtures/protocol-v2.jsonl), and
every frame is defined once, as a Rust type, in the [`knobdeck-protocol`](knobdeck-protocol) crate.

## The protocol number

`PROTOCOL` (SDK, and `knobdeck_core::plugin::PROTOCOL` in the host) names a **major** version of the
stdio protocol, and a plugin's `manifest.json` declares the one it was built for. It changes only for a
break (below). Everything else ships inside the same number.

- The host loads plugins built for **any number from `MIN_PROTOCOL` to `PROTOCOL`**. Today both are 2.
  When a break is unavoidable, the host moves `PROTOCOL` up by one and keeps accepting the previous
  number.
- A number is dropped from the supported range only after a public deprecation notice at least **12
  months** before the release that drops it. The notice goes in the SDK release notes, the plugin
  registry and the app's "update this plugin" message.
- When `PROTOCOL` moves, the host talks to each plugin in the dialect that plugin declared (its manifest
  `protocol`), so a plugin built for the previous number sees the frames it expects.
- A plugin may send a `ready.protocol` the host supports and nothing else; a number outside the range
  is refused with "update this plugin" (older) or "update KnobDeck" (newer).

## What is not a break (do these freely)

- Adding an **optional field** to a frame, in either direction.
- Adding a **new frame type**, as long as the sender only uses it when the other side said it can
  receive it (see "Features").
- Adding an optional field to `manifest.json` or `actions.json`.
- Adding a new `host_call` method or a new subscribable event.
- Tightening nothing: error messages and log text are not part of the contract.

## What is a break (needs a new number)

- Removing or renaming a field or frame type.
- Changing a field's JSON type or the meaning of a value.
- Making an optional field required.
- Changing the order guarantees the docs promise (for example "invokes for one instance run in order,
  and `done` follows the handler").
- Starting to send a frame to a plugin that did not ask for it.

## Rules for every sender and receiver

1. **Ignore what you don't know.** Unknown frame types and unknown fields are skipped, never an error.
   (The host's `InMsg` has an `Unknown` catch-all; the SDK ignores unknown host frames. Don't add
   `#[serde(deny_unknown_fields)]` anywhere on the wire.)
2. **Missing optional fields mean "the old behaviour".** A new field defaults to what the protocol did
   before it existed.
3. **Print nothing but frames on stdout.** Logs go to stderr. `knobdeck-plugin drive` fails a plugin
   that breaks this.

## Features

Behaviour that only some hosts have is announced in `hello.features` (a list of strings) and used only
when present. Today: `state_match` (a `state`/`error` frame may carry `match`). A plugin that wants a
feature checks the list and falls back when it is absent; the SDK does this for you. Features are
additive and don't count toward the protocol number. New host calls and events follow the same rule.

## The contract crate and the fixtures

`knobdeck-protocol` defines every frame: `HostMsg` (host → plugin) and `PluginMsg` (plugin → host),
plus the constants both sides need (`PROTOCOL`, `MIN_PROTOCOL`, `features`, `events`, triggers, request
and host-call methods). The host and this SDK both use it, so a change there changes both at once.
Generated from it and checked in: `knobdeck-protocol/schema/*.json` (JSON Schema), `typescript/protocol/` (the `@knobdeck/protocol` package)
(TypeScript types and constants) and `REFERENCE.md` (the written spec). CI fails when one is stale, and
`tsc --strict` checks every fixture frame against the TypeScript types.

`knobdeck-protocol/fixtures/protocol-v2.jsonl` holds example frames for every message type, both
directions (`to_plugin`, `to_host`). `cargo test --workspace --all-features` checks that:

- every fixture frame parses into the typed messages and is written back **byte for byte**;
- every frame this SDK emits has a known type, only known fields, and matching JSON types;
- the plugin survives every host frame in the file;
- the generated schema, TypeScript and reference files are current.

So a rename, a new field or a type change fails the tests until the fixture (and the schema, with
`KNOBDECK_BLESS=1 cargo test -p knobdeck-protocol --features schema`) is updated in the same change,
which makes the change visible in review. **Adding a field means adding it to the fixture** and a line
to the SDK release notes. Changing or removing a field means a new protocol number (above). Fields
added later are `Option`/`#[serde(default)]`, so a frame from an older peer still parses.

## Other versioned surfaces

| Surface | Today | Rule |
|---|---|---|
| `manifest.json` `protocol` | integer, required by the host | same range check as the wire |
| `.knobdeck` package | zip of `dist/<id>/`; no format version recorded | a missing `format` in `manifest.json` means layout 1; add `"format": 2` in the same change that alters the layout, and the host reads both |
| `actions.json` | no version recorded | additive only; a missing version means 1, and a version field arrives with the first non-additive change |
| Companion protocols (`knobdeck-companion/PROTOCOL.md`) | each companion documents its own; the browser one carries `protocol: 1` | additive only; the VS Code and IntelliJ HTTP dialects stay as they are |
| SDK crate (`knobdeck-sdk`) | semver `0.x` | a public API removal or rename is a minor bump with a note in the release notes until `1.0` |
