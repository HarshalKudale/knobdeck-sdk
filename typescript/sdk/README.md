# @knobdeck/sdk

Write KnobDeck plugins in TypeScript. A thin layer over the generated protocol types
(`knobdeck-protocol/typescript/protocol.ts`), with the same behaviour as the Rust SDK: the parity
test drives the same plugin written with both and requires identical frames, and
`knobdeck-plugin conform` checks it like any plugin.

```ts
import { Action, Plugin } from "@knobdeck/sdk";

new Plugin({ id: "hello", name: "Hello", version: "1.0.0" })
  .action(Action.button("ping", "Ping").onPress((ctx) => ctx.setLabel("pong")))
  .action(
    Action.toggle("mute", "Mute")
      .state("unmuted", "Unmuted", "mic")
      .state("muted", "Muted", "mic-off")
      .onPress((ctx) => ctx.fanOut(ctx.state === "muted" ? "unmuted" : "muted")),
  )
  .action(Action.slider("volume", "Volume", 0, 100).onAdjust((ctx) => ctx.setState(ctx.value ?? 0)))
  .query("items", (q) => ["a", q.input("group")])
  .run();
```

- Handlers may be async; invokes for one instance run in order, `ack` is sent at once and `done`
  when the handler returns.
- `ctx.fail(msg)` reports an error and stops; a thrown error is reported the same way.
- `ctx.toast()`, `ctx.appState()`, `ctx.openDial()` need a KnobDeck with the matching feature
  (`toast` falls back to the log). `ctx.sendKeys()` is the last resort.
- No OS code here: anything native goes through host calls. Plugins that need OS readers use the
  Rust SDK.
- `KNOBDECK_CODEGEN=1 node plugin.ts` prints the manifest and actions; the manifest declares
  `runtime: {language: "node", entry}`.

**Status:** the host doesn't run `language: "node"` plugins yet (PLATFORM_PLAN.md §3 C: one shared,
host-managed Node process). Until then, check plugins with
`knobdeck-plugin conform --dir <folder> -- node --experimental-strip-types plugin.ts`.

```sh
npm install && npm run check && npm test
```
