# @knobdeck/protocol

TypeScript types and constants for every frame of KnobDeck's protocols, **generated** from the Rust
definitions in the `knobdeck-protocol` crate (don't edit `src/index.ts` by hand):

- layer 1, plugin ↔ KnobDeck (`HostMsg`, `PluginMsg`, `PROTOCOL`, `FEATURES`, `EVENTS`, …);
- layer 2, companion ↔ KnobDeck's companion hub (`CompanionMsg`, `HubMsg`, `COMPANION_*`);
- layer 3, the external control API (`ClientMsg`, `ServerMsg`, `CONTROL_*`).

The written spec is [`knobdeck-protocol/REFERENCE.md`](https://github.com/Knobdeck/knobdeck-sdk/blob/master/knobdeck-protocol/REFERENCE.md);
what may change between versions is in [`COMPAT.md`](https://github.com/Knobdeck/knobdeck-sdk/blob/master/COMPAT.md).
Most people want `@knobdeck/sdk` (plugins) or `@knobdeck/companion` (companions) instead.
