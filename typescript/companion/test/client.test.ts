import { test } from "node:test";
import assert from "node:assert/strict";
import { Companion, Unsupported } from "../src/index.ts";
import type { SocketLike, Status } from "../src/index.ts";
import { dataDir, readDiscovery } from "../src/node.ts";

/** An in-memory hub end: records what the companion sends, lets the test answer. */
class FakeSocket implements SocketLike {
  static all: FakeSocket[] = [];
  readyState = 0;
  sent: any[] = [];
  onopen: ((ev: unknown) => void) | null = null;
  onmessage: ((ev: { data: unknown }) => void) | null = null;
  onclose: ((ev: unknown) => void) | null = null;
  onerror: ((ev: unknown) => void) | null = null;
  readonly url: string;
  constructor(url: string) {
    this.url = url;
    FakeSocket.all.push(this);
  }
  open() {
    this.readyState = 1;
    this.onopen?.({});
  }
  hub(msg: unknown) {
    this.onmessage?.({ data: JSON.stringify(msg) });
  }
  send(data: string) {
    this.sent.push(JSON.parse(data));
  }
  close() {
    this.readyState = 3;
  }
  drop() {
    this.readyState = 3;
    this.onclose?.({});
  }
}

const tick = () => new Promise((r) => setTimeout(r, 0));

function companion(statuses: Status[] = [], ports = [41000]) {
  FakeSocket.all = [];
  return new Companion({
    plugin: "photoshop",
    instance: "inst-1",
    app: "photoshop",
    appVersion: "27.1",
    token: "tok",
    ports,
    socket: (u) => new FakeSocket(u),
    onStatus: (s) => statuses.push(s),
    retryMinMs: 5,
    retryMaxMs: 20,
  });
}

test("hello carries who the companion is, then ready links it", () => {
  const statuses: Status[] = [];
  const c = companion(statuses).start();
  const ws = FakeSocket.all[0];
  assert.equal(ws.url, "ws://127.0.0.1:41000/companion");
  ws.open();
  assert.deepEqual(ws.sent[0], {
    type: "hello", protocol: 1, plugin: "photoshop", instance: "inst-1", app: "photoshop",
    app_version: "27.1", features: [], token: "tok",
  });
  ws.hub({ type: "ready", protocol: 1, link: "photoshop-3", host_version: "1.0" });
  assert.equal(c.link, "photoshop-3");
  assert.equal(c.connected, true);
  assert.deepEqual(statuses.at(-1), { state: "connected", link: "photoshop-3", port: 41000 });
  c.stop();
});

test("requests run the registered action; unknown and Unsupported answer unsupported; errors carry the message", async () => {
  const c = companion()
    .action("set-tool", (p: any) => ({ tool: p.tool }))
    .action("slow", async () => { await tick(); return 7; })
    .action("no-api", () => { throw new Unsupported(); })
    .action("fails", () => { throw new Error("No document is open"); })
    .action("nothing", () => undefined)
    .start();
  const ws = FakeSocket.all[0];
  ws.open();
  ws.hub({ type: "ready", protocol: 1, link: "l", host_version: "1" });
  for (const [id, action] of [["1", "set-tool"], ["2", "slow"], ["3", "no-api"], ["4", "fails"], ["5", "missing"], ["6", "nothing"]]) {
    ws.hub({ type: "request", id, action, params: { tool: "brush" } });
  }
  await tick(); await tick();
  const byId = Object.fromEntries(ws.sent.filter((m) => m.type === "response").map((m) => [m.id, m]));
  assert.deepEqual(byId["1"], { type: "response", id: "1", ok: true, result: { tool: "brush" } });
  assert.deepEqual(byId["2"], { type: "response", id: "2", ok: true, result: 7 });
  assert.deepEqual(byId["3"], { type: "response", id: "3", ok: false, unsupported: true });
  assert.deepEqual(byId["4"], { type: "response", id: "4", ok: false, error: "No document is open" });
  assert.deepEqual(byId["5"], { type: "response", id: "5", ok: false, unsupported: true });
  assert.deepEqual(byId["6"], { type: "response", id: "6", ok: true, result: null });
  c.stop();
});

test("fallback handles actions without their own handler; own handlers win", async () => {
  const c = companion()
    .action("own", () => "mine")
    .fallback((action, params) => ({ action, params }))
    .start();
  const ws = FakeSocket.all[0];
  ws.open();
  ws.hub({ type: "ready", protocol: 1, link: "l", host_version: "1" });
  ws.hub({ type: "request", id: "1", action: "own", params: {} });
  ws.hub({ type: "request", id: "2", action: "editor.action.formatDocument", params: { x: 1 } });
  await tick(); await tick();
  const byId = Object.fromEntries(ws.sent.filter((m) => m.type === "response").map((m) => [m.id, m]));
  assert.equal(byId["1"].result, "mine");
  assert.deepEqual(byId["2"].result, { action: "editor.action.formatDocument", params: { x: 1 } });
  c.stop();
});

test("emit sends events only while linked; unknown hub frames are ignored", () => {
  const c = companion().start();
  const ws = FakeSocket.all[0];
  ws.open();
  assert.equal(c.emit("tool_changed", { tool: "eraser" }), false, "not linked yet");
  ws.hub({ type: "ready", protocol: 1, link: "l", host_version: "1" });
  ws.hub({ type: "from_a_newer_hub", x: 1 });
  assert.equal(c.emit("tool_changed", { tool: "eraser" }), true);
  assert.deepEqual(ws.sent.at(-1), { type: "event", event: "tool_changed", payload: { tool: "eraser" } });
  c.stop();
});

test("a refusal is reported and retried on the same port after a delay; a dead port moves to the next", async () => {
  const statuses: Status[] = [];
  const c = companion(statuses, [41000, 41001]).start();
  // Port 41000 is closed: the next port is tried at once.
  FakeSocket.all[0].drop();
  assert.equal(FakeSocket.all[1].url, "ws://127.0.0.1:41001/companion");
  // 41001 refuses (wrong token): reported, then retried later, not immediately.
  const ws = FakeSocket.all[1];
  ws.open();
  ws.hub({ type: "refused", reason: "wrong token" });
  ws.drop();
  assert.deepEqual(statuses.find((s) => s.state === "refused"), { state: "refused", reason: "wrong token" });
  assert.equal(FakeSocket.all.length, 2, "no immediate retry after a refusal");
  await new Promise((r) => setTimeout(r, 30));
  assert.ok(FakeSocket.all.length >= 3, "retried after the delay");
  c.stop();
  const n = FakeSocket.all.length;
  await new Promise((r) => setTimeout(r, 40));
  assert.equal(FakeSocket.all.length, n, "stop() ends the retries");
});

test("a lost link reports disconnected and reconnects", async () => {
  const statuses: Status[] = [];
  const c = companion(statuses).start();
  const ws = FakeSocket.all[0];
  ws.open();
  ws.hub({ type: "ready", protocol: 1, link: "l", host_version: "1" });
  ws.drop();
  assert.equal(c.connected, false);
  assert.deepEqual(statuses.at(-1), { state: "disconnected" });
  await new Promise((r) => setTimeout(r, 15));
  assert.ok(FakeSocket.all.length >= 2);
  c.stop();
});

test("token and ports are read again on every attempt, so a KnobDeck restart is picked up", async () => {
  FakeSocket.all = [];
  let file = { token: "old", port: 41000 };
  const c = new Companion({
    plugin: "p", instance: "i", app: "a",
    token: () => file.token, ports: () => [file.port],
    socket: (u) => new FakeSocket(u), retryMinMs: 5, retryMaxMs: 10,
  }).start();
  FakeSocket.all[0].open();
  assert.equal(FakeSocket.all[0].sent[0].token, "old");
  // KnobDeck restarts: new token, new port; the old link drops.
  file = { token: "new", port: 41003 };
  FakeSocket.all[0].hub({ type: "refused", reason: "wrong token" });
  FakeSocket.all[0].drop();
  await new Promise((r) => setTimeout(r, 20));
  const next = FakeSocket.all.at(-1)!;
  assert.equal(next.url, "ws://127.0.0.1:41003/companion");
  next.open();
  assert.equal(next.sent[0].token, "new");
  c.stop();
});

test("fromDiscovery reads the file on each call", async () => {
  const { mkdtempSync, writeFileSync } = await import("node:fs");
  const { tmpdir } = await import("node:os");
  const { fromDiscovery } = await import("../src/node.ts");
  const path = mkdtempSync(tmpdir() + "/kd-") + "/companion-hub.json";
  const d = fromDiscovery(path);
  assert.equal(d.token(), undefined);
  assert.equal(d.ports(), undefined);
  writeFileSync(path, JSON.stringify({ protocol: 1, port: 32152, path: "/companion", token: "t1" }));
  assert.equal(d.token(), "t1");
  assert.deepEqual(d.ports(), [32152]);
  writeFileSync(path, JSON.stringify({ protocol: 1, port: 32150, path: "/companion", token: "t2" }));
  assert.equal(d.token(), "t2");
});

test("node helpers find the data folder per OS and read the discovery file", async () => {
  assert.equal(dataDir("win32", { APPDATA: "C:\\Users\\u\\AppData\\Roaming" }).replaceAll("\\", "/"), "C:/Users/u/AppData/Roaming/KnobDeck");
  assert.ok(dataDir("darwin", {}).endsWith("Library/Application Support/KnobDeck"));
  assert.equal(dataDir("linux", { XDG_CONFIG_HOME: "/x" }), "/x/KnobDeck");
  const { mkdtempSync, writeFileSync } = await import("node:fs");
  const { tmpdir } = await import("node:os");
  const dir = mkdtempSync(tmpdir() + "/kd-");
  writeFileSync(dir + "/companion-hub.json", JSON.stringify({ protocol: 1, port: 32151, path: "/companion", token: "t" }));
  assert.deepEqual(readDiscovery(dir + "/companion-hub.json"), { protocol: 1, port: 32151, path: "/companion", token: "t" });
  assert.equal(readDiscovery(dir + "/missing.json"), null);
});
