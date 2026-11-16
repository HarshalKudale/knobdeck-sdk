import { test } from "node:test";
import assert from "node:assert/strict";
import { Action, Plugin, makeFullId } from "../src/index.ts";

test("full ids match the Rust SDK's pinned values", () => {
  // knobdeck-sdk/src/codegen.rs: full_ids_are_pinned / full_id_matches_js_sdk
  assert.equal(makeFullId("example-rust", "mute"), "example-rust-mute-b1c2189c");
  assert.equal(makeFullId("win-tools", "always-on-top"), "win-tools-always-on-top-7ee2f192");
  assert.equal(makeFullId("discord", "mute"), "discord-mute-b094f5e1");
  assert.equal(makeFullId("", "mute"), "mute");
});

test("codegen validates like the Rust SDK", () => {
  const p = (a: ReturnType<typeof Action.button>) => new Plugin({ id: "x", name: "X" }).action(a).codegen("x.ts");
  assert.throws(() => p(Action.toggle("t", "T")), /non-empty "states"/);
  assert.throws(() => p(Action.hold("h", "H").released("r", "i")), /both "released" and "held"/);
  assert.throws(() => p(Action.slider("s", "S", 0, 1).step(0)), /positive number/);
  assert.throws(() => new Plugin({ id: "x", name: "X" }).action(Action.button("a", "A")).action(Action.button("a", "B")), /duplicate/);
});

test("platforms are inherited by actions", () => {
  const out = new Plugin({ id: "mac-tools", name: "Mac Tools", platforms: ["macos"] })
    .action(Action.button("a", "A"))
    .action(Action.button("b", "B").platforms(["windows"]))
    .codegen("p.ts");
  assert.deepEqual(out.manifest.platforms, ["macos"]);
  assert.deepEqual((out.actions.actions[0] as any).platforms, ["macos"]);
  assert.deepEqual((out.actions.actions[1] as any).platforms, ["windows"]);
  assert.deepEqual(out.manifest.runtime, { language: "node", entry: "p.ts" });
});
