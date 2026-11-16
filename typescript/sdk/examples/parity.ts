// The parity plugin: the same plugin as knobdeck-sdk/examples/parity.rs. tests/parity.rs drives both
// through one host script and requires identical frames.

import { Action, Plugin } from "../src/index.ts";

new Plugin({ id: "parity", name: "Parity", version: "1.0.0", author: "KnobDeck" })
  .action(Action.button("ping", "Ping").onPress((ctx) => ctx.setLabel("pong")))
  .action(
    Action.toggle("mute", "Mute")
      .state("unmuted", "Unmuted", "mic")
      .state("muted", "Muted", "mic-off")
      .onPress((ctx) => ctx.fanOut(ctx.state === "muted" ? "unmuted" : "muted")),
  )
  .action(
    Action.hold("ptt", "Push to talk")
      .released("Released", "mic-off")
      .held("Talking", "mic")
      .onHoldStart((ctx) => ctx.setState("held"))
      .onHoldEnd((ctx) => ctx.setState("released")),
  )
  .action(Action.slider("volume", "Volume", 0, 100).unit("%").onAdjust((ctx) => ctx.setState(ctx.value ?? 0)))
  .action(Action.button("fail", "Fail").onPress((ctx) => ctx.fail("nope")))
  .action(Action.button("toast", "Toast").onPress((ctx) => ctx.toast("hi")))
  .query("items", (q) => ["a", q.input("group")])
  .run();
