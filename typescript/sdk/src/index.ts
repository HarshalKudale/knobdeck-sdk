// @knobdeck/sdk: KnobDeck plugins in TypeScript.
//
// A thin layer over the generated protocol types (@knobdeck/protocol), with
// the same behaviour as the Rust SDK: `knobdeck-plugin conform` and the parity test check both
// against one transcript. No OS code here: anything native goes through host calls.

import { createHash } from "node:crypto";
import { basename } from "node:path";
import { createInterface } from "node:readline";
import { parentPort, workerData } from "node:worker_threads";
import { PROTOCOL } from "@knobdeck/protocol";
import type {
  AppState,
  HostMsg,
  Instance,
  PluginMsg,
} from "@knobdeck/protocol";

export { PROTOCOL };
export type { AppState, HostMsg, PluginMsg };

/** `<plugin>-<action>-<8 hex>`: the ids placed actions are stored under. Never change them. */
export function makeFullId(pluginId: string, actionId: string): string {
  if (!pluginId) return actionId;
  const hash = createHash("sha1").update(`${pluginId}:${actionId}`).digest("hex").slice(0, 8);
  return `${pluginId}-${actionId}-${hash}`;
}

/** Throw from a handler (or `return ctx.fail(msg)`) to report an error and stop. */
export class Fail extends Error {}

type Kind = "button" | "toggle" | "hold" | "slider";
type Visual = { state?: string; label?: string; icon?: string };
type Handler = (ctx: Ctx) => void | Promise<void>;

/** A float the way the Rust SDK writes one (serde: `50.0`, `0.5`). */
function floatStr(n: number): string {
  return Number.isInteger(n) ? n.toFixed(1) : String(n);
}

/** Visual fields in the order the Rust SDK writes them. */
function ordered(v: Visual): Visual {
  const o: Visual = {};
  if (v.state !== undefined) o.state = v.state;
  if (v.label !== undefined) o.label = v.label;
  if (v.icon !== undefined) o.icon = v.icon;
  return o;
}

function valueStr(v: unknown): string {
  return typeof v === "string" ? v : JSON.stringify(v);
}

export class ActionDef {
  readonly id: string;
  readonly label: string;
  readonly kind: Kind;
  _icon?: string;
  _inputs: unknown[] = [];
  _states: { value: string; label: string; icon: string }[] = [];
  _released?: { label: string; icon: string };
  _held?: { label: string; icon: string };
  _min = 0;
  _max = 0;
  _step?: number;
  _unit?: string;
  _initial?: string;
  _platforms: string[] = [];
  _onPress?: Handler;
  _onAdjust?: Handler;
  _onHoldStart?: Handler;
  _onHoldEnd?: Handler;

  constructor(id: string, label: string, kind: Kind) {
    this.id = id;
    this.label = label;
    this.kind = kind;
  }
  icon(v: string): this { this._icon = v; return this; }
  inputs(list: unknown[]): this { this._inputs = list; return this; }
  /** A toggle state; the first is the initial one. */
  state(value: string, label: string, icon: string): this { this._states.push({ value, label, icon }); return this; }
  released(label: string, icon: string): this { this._released = { label, icon }; return this; }
  held(label: string, icon: string): this { this._held = { label, icon }; return this; }
  step(v: number): this { this._step = v; return this; }
  unit(v: string): this { this._unit = v; return this; }
  initial(v: string): this { this._initial = v; return this; }
  platforms(list: string[]): this { this._platforms = list; return this; }
  onPress(f: Handler): this { this._onPress = f; return this; }
  onAdjust(f: Handler): this { this._onAdjust = f; return this; }
  onHoldStart(f: Handler): this { this._onHoldStart = f; return this; }
  onHoldEnd(f: Handler): this { this._onHoldEnd = f; return this; }
}

export const Action = {
  button: (id: string, label: string) => new ActionDef(id, label, "button"),
  toggle: (id: string, label: string) => new ActionDef(id, label, "toggle"),
  hold: (id: string, label: string) => new ActionDef(id, label, "hold"),
  slider: (id: string, label: string, min: number, max: number) => {
    const a = new ActionDef(id, label, "slider");
    a._min = min;
    a._max = max;
    return a;
  },
};

export interface PluginMeta {
  id: string;
  name: string;
  version?: string;
  author?: string;
  description?: string;
  icon?: string;
  platforms?: string[];
}

export interface HostEvent {
  name: string;
  payload: unknown;
}

/** Where frames come from and go to: stdio for a plugin run on its own, the parent thread when
 * KnobDeck's shared Node runner loads it in a worker. */
interface Transport {
  send(line: string): void;
  onLine(f: (line: string) => void): void;
  onClose(f: () => void): void;
}

function stdioTransport(): Transport {
  const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
  return {
    send: (line) => void process.stdout.write(line + "\n"),
    onLine: (f) => void lines.on("line", f),
    onClose: (f) => void lines.on("close", f),
  };
}

/** In a runner worker: lines arrive as string messages; `null` means the host went away. */
function workerTransport(port: NonNullable<typeof parentPort>): Transport {
  let onLine: (l: string) => void = () => {};
  let onClose: () => void = () => {};
  port.on("message", (m: unknown) => (typeof m === "string" ? onLine(m) : onClose()));
  return {
    send: (line) => port.postMessage(line),
    onLine: (f) => void (onLine = f),
    onClose: (f) => void (onClose = f),
  };
}

function transport(): Transport {
  const inRunner = parentPort && (workerData as { knobdeck?: boolean } | undefined)?.knobdeck;
  return inRunner ? workerTransport(parentPort!) : stdioTransport();
}

function log(msg: string): void {
  process.stderr.write(`[${new Date().toISOString()}] ${msg}\n`);
}

/** What handlers, timers and event callbacks use to talk to KnobDeck. */
export class Runtime {
  readonly pluginId: string;
  readonly dataDir: string;
  readonly features: string[];
  /** @internal */ states = new Map<string, string>();
  /** @internal */ actionOf = new Map<string, string>();
  /** @internal */ inputsOf = new Map<string, Record<string, unknown>>();
  #pending = new Map<string, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();
  #next = 1;

  #write: (line: string) => void;

  constructor(pluginId: string, dataDir: string, features: string[], write: (line: string) => void = (l) => void process.stdout.write(l + "\n")) {
    this.pluginId = pluginId;
    this.dataDir = dataDir;
    this.features = features;
    this.#write = write;
  }

  /** @internal */ send(msg: PluginMsg): void {
    this.#write(JSON.stringify(msg));
    if (msg.type === "state" && msg.state !== undefined) {
      if (msg.instance_id) this.states.set(msg.instance_id, msg.state);
      else if (msg.action_id) {
        for (const [iid, a] of this.actionOf) if (makeFullId(this.pluginId, a) === msg.action_id) this.states.set(iid, msg.state);
      }
    }
  }

  log(msg: string): void { log(msg); }
  hasFeature(f: string): boolean { return this.features.includes(f); }
  fullId(action: string): string { return makeFullId(this.pluginId, action); }

  setState(instanceId: string, value: string | number): void {
    this.send({ type: "state", instance_id: instanceId, state: String(value) });
  }
  setVisual(instanceId: string, v: Visual): void {
    this.send({ type: "state", instance_id: instanceId, ...ordered(v) });
  }
  /** Every instance of `action` (short id). */
  notify(action: string, value: string | number): void {
    this.send({ type: "state", action_id: this.fullId(action), state: String(value) });
  }
  notifyVisual(action: string, v: Visual): void {
    this.send({ type: "state", action_id: this.fullId(action), ...ordered(v) });
  }
  setError(instanceId: string, message: string): void {
    this.send({ type: "error", instance_id: instanceId, message });
  }
  markSetupRequired(reason: string): void {
    this.send({ type: "setup_required", reason });
  }

  /** @internal */ hostCall(method: string, params: unknown, timeoutMs = 30_000): Promise<unknown> {
    const id = `host-${this.#next++}`;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        reject(new Error(`host call ${method} timed out`));
      }, timeoutMs);
      this.#pending.set(id, {
        resolve: (v) => { clearTimeout(timer); resolve(v); },
        reject: (e) => { clearTimeout(timer); reject(e); },
      });
      this.send({ type: "host_call", id, method, params });
    });
  }
  /** @internal */ answer(id: string, ok: boolean, value: unknown, error?: string): void {
    const p = this.#pending.get(id);
    if (!p) return;
    this.#pending.delete(id);
    if (ok) p.resolve(value ?? null);
    else p.reject(new Error(error ?? "host call failed"));
  }

  /** A short message to the user; the plugin log on a KnobDeck without `notify`. */
  async toast(message: string, level?: "info" | "error"): Promise<void> {
    if (!this.hasFeature("notify")) {
      log(`toast (${level ?? "info"}): ${message}`);
      return;
    }
    await this.hostCall("notify", level ? { message, level } : { message });
  }
  async appState(): Promise<AppState> {
    if (!this.hasFeature("app_state")) throw new Error("this KnobDeck is too old for app_state; update KnobDeck");
    return (await this.hostCall("app_state", {})) as AppState;
  }
  async openDial(profileId?: string): Promise<boolean> {
    if (!this.hasFeature("open_dial")) throw new Error("this KnobDeck is too old to open the dial for a plugin; update KnobDeck");
    return Boolean(await this.hostCall("open_dial", profileId ? { profile_id: profileId } : {}));
  }
  /** Last resort: see the keystroke rule in the docs. */
  async sendKeys(keys: string): Promise<void> {
    if (!keys.trim()) throw new Error("send_keys requires a non-empty key string");
    await this.hostCall("send_keys", { keys: keys.trim() });
  }
}

/** One invoke. */
export class Ctx {
  readonly instanceId: string;
  readonly actionId: string;
  readonly trigger: string;
  readonly state: string | undefined;
  readonly inputs: Record<string, unknown>;
  /** A slider's absolute value / change, on `adjust`. */
  readonly value: number | undefined;
  readonly delta: number | undefined;
  readonly rt: Runtime;

  constructor(rt: Runtime, instanceId: string, actionId: string, trigger: string, inputs: Record<string, unknown>, adjust: boolean) {
    this.rt = rt;
    this.instanceId = instanceId;
    this.actionId = actionId;
    this.trigger = trigger;
    this.state = rt.states.get(instanceId);
    this.inputs = inputs;
    const num = (v: unknown) => (typeof v === "number" ? v : typeof v === "string" && v.trim() !== "" && !isNaN(Number(v)) ? Number(v) : undefined);
    this.value = adjust ? num(inputs.value) : undefined;
    this.delta = adjust ? num(inputs.delta) : undefined;
  }
  log(msg: string): void { log(msg); }
  setState(v: string | number): void { this.rt.setState(this.instanceId, v); }
  setVisual(v: Visual): void { this.rt.setVisual(this.instanceId, v); }
  setLabel(label: string): void { this.setVisual({ label }); }
  setIcon(icon: string): void { this.setVisual({ icon }); }
  /** Every instance of this action. */
  fanOut(v: string | number): void { this.rt.send({ type: "state", action_id: this.rt.fullId(this.actionId), state: String(v) }); }
  setError(message: string): void { this.rt.setError(this.instanceId, message); }
  /** Report an error and stop: `return ctx.fail("…")` or `throw`. */
  fail(message: string): never {
    log(`error: ${message}`);
    this.setError(message);
    throw new Fail(message);
  }
  markSetupRequired(reason: string): void { this.rt.markSetupRequired(reason); }
  toast(message: string): Promise<void> { return this.rt.toast(message); }
  appState(): Promise<AppState> { return this.rt.appState(); }
  openDial(profileId?: string): Promise<boolean> { return this.rt.openDial(profileId); }
  sendKeys(keys: string): Promise<void> { return this.rt.sendKeys(keys); }
}

export interface QueryCtx {
  inputs: Record<string, unknown>;
  input(key: string): string;
}

export class Plugin {
  readonly meta: PluginMeta;
  #actions = new Map<string, ActionDef>();
  #queries = new Map<string, (q: QueryCtx) => unknown | Promise<unknown>>();
  #subscriptions: string[] = [];
  #onReady?: (rt: Runtime) => void | Promise<void>;
  #onEvent?: (ev: HostEvent, rt: Runtime) => void | Promise<void>;
  #onShutdown?: (rt: Runtime) => void;

  constructor(meta: PluginMeta) {
    this.meta = meta;
  }
  action(a: ActionDef): this {
    if (this.#actions.has(a.id)) throw new Error(`duplicate action id "${a.id}"`);
    this.#actions.set(a.id, a);
    return this;
  }
  query(id: string, f: (q: QueryCtx) => unknown | Promise<unknown>): this { this.#queries.set(id, f); return this; }
  subscribe(events: string[]): this { this.#subscriptions.push(...events); return this; }
  /** Accept companions (KnobDeck's companion hub) whose `hello.plugin` is this plugin's id. */
  companions(): this { return this.subscribe(["companion_connected", "companion_disconnected", "companion_event"]); }
  onReady(f: (rt: Runtime) => void | Promise<void>): this { this.#onReady = f; return this; }
  onEvent(f: (ev: HostEvent, rt: Runtime) => void | Promise<void>): this { this.#onEvent = f; return this; }
  onShutdown(f: (rt: Runtime) => void): this { this.#onShutdown = f; return this; }

  /** `manifest.json` and `actions.json`, as the Rust SDK writes them. */
  codegen(entry: string): { manifest: Record<string, unknown>; actions: { actions: unknown[] } } {
    const m = this.meta;
    const manifest: Record<string, unknown> = {
      id: m.id,
      name: m.name,
      protocol: PROTOCOL,
      // Run by KnobDeck's shared Node runtime, not as a native executable.
      runtime: { language: "node", entry },
    };
    for (const k of ["icon", "version", "author", "description"] as const) if (m[k] !== undefined) manifest[k] = m[k];
    if (m.platforms?.length) manifest.platforms = m.platforms;
    const actions = [...this.#actions.values()].map((a) => {
      if (a.kind === "toggle" && a._states.length === 0) throw new Error(`Toggle action "${a.id}" must declare a non-empty "states" list`);
      if (a.kind === "hold" && (!a._released || !a._held)) throw new Error(`Hold action "${a.id}" must declare both "released" and "held"`);
      if (a.kind === "slider" && a._step !== undefined && a._step <= 0) throw new Error(`Slider action "${a.id}" "step" must be a positive number`);
      const e: Record<string, unknown> = { id: makeFullId(m.id, a.id), label: a.label, type: a.kind };
      if (a._icon) e.icon = a._icon;
      e.inputs = a._inputs;
      const platforms = a._platforms.length ? a._platforms : m.platforms ?? [];
      if (platforms.length) e.platforms = platforms;
      if (a.kind === "toggle") e.states = a._states;
      if (a.kind === "hold") { e.released = a._released; e.held = a._held; }
      if (a.kind === "slider") {
        e.min = a._min; e.max = a._max; e.step = a._step ?? 1;
        if (a._unit) e.unit = a._unit;
      }
      return e;
    });
    return { manifest, actions: { actions } };
  }

  #initial(a: ActionDef | undefined): string {
    if (!a) return "off";
    if (a.kind === "hold") return "released";
    if (a._initial !== undefined) return a._initial;
    if (a._states.length) return a._states[0].value;
    if (a.kind === "slider") return floatStr(a._min);
    return "off";
  }

  #short(full: string): string {
    for (const id of this.#actions.keys()) if (makeFullId(this.meta.id, id) === full) return id;
    return full;
  }

  /** Start: codegen when KNOBDECK_CODEGEN=1, else the protocol until shutdown (over stdio, or
   * over the parent thread inside KnobDeck's shared Node runner). */
  run(): void {
    if (process.env.KNOBDECK_CODEGEN === "1") {
      const out = this.codegen(basename(process.argv[1] ?? "plugin.ts"));
      process.stdout.write(JSON.stringify({ ...out, meta: { setup_page: null, setup_assets_dir: null } }) + "\n");
      process.exit(0);
    }
    // stdout carries frames only.
    console.log = console.info = console.debug = (...a: unknown[]) => log(a.map(String).join(" "));
    let rt: Runtime | undefined;
    const queues = new Map<string, Promise<void>>();
    const io = transport();
    // In a runner worker, process.exit() ends only this worker.
    const finish = () => {
      if (rt) this.#onShutdown?.(rt);
      log("shutdown");
      process.exit(0);
    };
    io.onClose(finish);
    io.onLine((line) => {
      let msg: HostMsg;
      try {
        msg = JSON.parse(line) as HostMsg;
      } catch {
        log(`bad line from host: ${line}`);
        return;
      }
      if (!rt) {
        if (msg.type !== "hello") return log(`expected hello, got: ${line}`);
        if (msg.protocol !== PROTOCOL) log(`host speaks protocol ${msg.protocol}, this SDK speaks ${PROTOCOL}`);
        rt = new Runtime(msg.plugin_id || this.meta.id, msg.data_dir, msg.features ?? [], io.send);
        const subscribe: string[] = [];
        for (const e of this.#subscriptions.map((s) => s.trim())) if (e && !subscribe.includes(e)) subscribe.push(e);
        rt.send({ type: "ready", protocol: PROTOCOL, subscribe, done: true });
        log("ready");
        return;
      }
      this.#dispatch(rt, msg, queues, finish);
    });
  }

  #dispatch(rt: Runtime, msg: HostMsg, queues: Map<string, Promise<void>>, finish: () => void): void {
    switch (msg.type) {
      case "sync": {
        log(`sync: ${msg.instances.length} instance(s)`);
        const keep = new Set(msg.instances.map((i: Instance) => i.instance_id));
        for (const m of [rt.actionOf, rt.inputsOf, rt.states]) for (const k of [...m.keys()]) if (!keep.has(k)) m.delete(k);
        for (const inst of msg.instances) {
          const short = this.#short(inst.action_id);
          rt.actionOf.set(inst.instance_id, short);
          rt.inputsOf.set(inst.instance_id, (inst.inputs ?? {}) as Record<string, unknown>);
          if (!rt.states.has(inst.instance_id)) rt.states.set(inst.instance_id, this.#initial(this.#actions.get(short)));
          rt.send({ type: "state", instance_id: inst.instance_id, state: rt.states.get(inst.instance_id)! });
        }
        if (this.#onReady) {
          const f = this.#onReady;
          this.#onReady = undefined; // once, after the first sync
          Promise.resolve().then(() => f(rt)).catch((e) => log(`on_ready error: ${e}`));
        }
        return;
      }
      case "invoke": {
        rt.send({ type: "ack", id: msg.id });
        const key = msg.instance_id;
        const run = async () => {
          try {
            await this.#invoke(rt, msg);
          } finally {
            rt.send({ type: "done", id: msg.id });
          }
        };
        const next = (queues.get(key) ?? Promise.resolve()).then(run, run);
        queues.set(key, next);
        void next.finally(() => { if (queues.get(key) === next) queues.delete(key); });
        return;
      }
      case "request": {
        const p = (msg.params ?? {}) as Record<string, unknown>;
        if (msg.method === "query") {
          const qid = String(p.query_id ?? "");
          const f = this.#queries.get(qid);
          const inputs = (p.inputs ?? {}) as Record<string, unknown>;
          const q: QueryCtx = { inputs, input: (k) => (inputs[k] === undefined || inputs[k] === null ? "" : valueStr(inputs[k])) };
          Promise.resolve()
            .then(() => {
              if (!f) throw new Error(`unknown query "${qid}"`);
              return f(q);
            })
            .then(
              (value) => rt.send({ type: "result", id: msg.id, ok: true, value: value ?? null }),
              (e) => rt.send({ type: "result", id: msg.id, ok: false, error: e instanceof Error ? e.message : String(e) }),
            );
        } else {
          rt.send({ type: "result", id: msg.id, ok: false, error: `unknown request method "${msg.method}"` });
        }
        return;
      }
      case "event": {
        const f = this.#onEvent;
        if (f) Promise.resolve().then(() => f({ name: msg.event, payload: msg.payload ?? {} }, rt)).catch((e) => log(`on_event error (${msg.event}): ${e}`));
        return;
      }
      case "result":
        rt.answer(msg.id, msg.ok, msg.value, msg.error);
        return;
      case "shutdown":
        finish();
        return;
      default:
        return; // frame types from a newer host
    }
  }

  async #invoke(rt: Runtime, msg: Extract<HostMsg, { type: "invoke" }>): Promise<void> {
    const short = this.#short(msg.action_id);
    rt.actionOf.set(msg.instance_id, short);
    const a = this.#actions.get(short);
    if (!a) return log(`unknown action: ${short}`);
    const adjust = a.kind === "slider" && msg.trigger === "adjust";
    const ctx = new Ctx(rt, msg.instance_id, short, msg.trigger, (msg.inputs ?? {}) as Record<string, unknown>, adjust);
    const h =
      (adjust && a._onAdjust) ||
      (msg.trigger === "hold_start" && a._onHoldStart) ||
      (msg.trigger === "hold_end" && a._onHoldEnd) ||
      a._onPress;
    if (!h) return;
    try {
      await h(ctx);
    } catch (e) {
      if (e instanceof Fail) return; // already reported
      const m = e instanceof Error ? e.message : String(e);
      log(`handler (${short}): ${m}`);
      rt.setError(msg.instance_id, m);
    }
  }
}
