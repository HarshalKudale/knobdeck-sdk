// @knobdeck/companion: connect an add-on inside another app (a Photoshop UXP panel, a Figma plugin,
// a browser extension, a Node-based add-on) to its KnobDeck plugin through KnobDeck's companion hub.
//
// The frames are the generated layer-2 types of knobdeck-protocol; nothing here is hand-kept
// protocol. See knobdeck-protocol/REFERENCE.md ("Companion → hub").

import {
  COMPANION_PING_EVERY_SECS,
  COMPANION_PORTS,
  COMPANION_PROTOCOL,
  COMPANION_WS_PATH,
} from "@knobdeck/protocol";
import type {
  ActionResult,
  CompanionInfo,
  CompanionMsg,
  HubMsg,
} from "@knobdeck/protocol";

export type { ActionResult, CompanionInfo, CompanionMsg, HubMsg };
export { COMPANION_PORTS, COMPANION_PROTOCOL, COMPANION_WS_PATH };

/** Throw this from an action handler when the app has no API for the action. */
export class Unsupported extends Error {
  constructor(message = "unsupported") {
    super(message);
    this.name = "Unsupported";
  }
}

/** Runs one action; its return value is the `result` the plugin gets. */
export type ActionHandler = (params: unknown) => unknown | Promise<unknown>;

export type Status =
  | { state: "connecting"; port: number }
  | { state: "connected"; link: string; port: number }
  | { state: "refused"; reason: string }
  | { state: "disconnected" };

/** The subset of the WebSocket API the client uses (the browser's, Node's, UXP's). */
export interface SocketLike {
  readyState: number;
  send(data: string): void;
  close(): void;
  onopen: ((ev: unknown) => void) | null;
  onmessage: ((ev: { data: unknown }) => void) | null;
  onclose: ((ev: unknown) => void) | null;
  onerror: ((ev: unknown) => void) | null;
}
export type SocketFactory = (url: string) => SocketLike;

export interface CompanionOptions {
  /** The KnobDeck plugin this companion belongs to (its manifest id). */
  plugin: string;
  /** Stable per install / profile / window (store a random id once). */
  instance: string;
  /** The app it lives in: `photoshop`, `figma`, `chrome`… */
  app: string;
  appVersion?: string;
  /** What the user sees ("Photoshop 2026"). */
  name?: string;
  /** This companion's version. */
  version?: string;
  features?: string[];
  /** The hub token from the discovery file (`@knobdeck/companion/node` reads it). Browser
   * extensions connect without one. */
  token?: string;
  /** Ports to try, in order (default: the discovery file's port, else all hub ports). */
  ports?: readonly number[];
  host?: string;
  /** Defaults to the global `WebSocket`. */
  socket?: SocketFactory;
  onStatus?: (s: Status) => void;
  /** Reconnect delays grow from `retryMinMs` to `retryMaxMs` (defaults 1 s and 30 s). */
  retryMinMs?: number;
  retryMaxMs?: number;
}

const OPEN = 1;

export class Companion {
  readonly #opts: CompanionOptions;
  readonly #handlers = new Map<string, ActionHandler>();
  #ws: SocketLike | null = null;
  #link: string | undefined;
  #port = 0;
  #portIndex = 0;
  #retryMs: number;
  #stopped = true;
  #pingTimer: ReturnType<typeof setInterval> | undefined;
  #retryTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(opts: CompanionOptions) {
    this.#opts = opts;
    this.#retryMs = opts.retryMinMs ?? 1000;
  }

  /** Register what `action` does in the app. Unregistered actions answer `unsupported`. */
  action(name: string, handler: ActionHandler): this {
    this.#handlers.set(name, handler);
    return this;
  }

  /** The hub's id for this link while connected. */
  get link(): string | undefined {
    return this.#link;
  }

  get connected(): boolean {
    return this.#link !== undefined && this.#ws?.readyState === OPEN;
  }

  /** Send live app state to the plugin (dropped while disconnected). */
  emit(event: string, payload: unknown = null): boolean {
    return this.#send({ type: "event", event, payload });
  }

  /** Connect, and keep reconnecting until `stop()`. */
  start(): this {
    if (!this.#stopped) return this;
    this.#stopped = false;
    this.#connect();
    return this;
  }

  stop(): void {
    this.#stopped = true;
    clearTimeout(this.#retryTimer);
    this.#teardown();
  }

  #ports(): readonly number[] {
    return this.#opts.ports?.length ? this.#opts.ports : COMPANION_PORTS;
  }

  #connect(): void {
    if (this.#stopped) return;
    const ports = this.#ports();
    this.#port = ports[this.#portIndex % ports.length];
    const url = `ws://${this.#opts.host ?? "127.0.0.1"}:${this.#port}${COMPANION_WS_PATH}`;
    this.#opts.onStatus?.({ state: "connecting", port: this.#port });
    const factory = this.#opts.socket ?? ((u: string) => new WebSocket(u) as unknown as SocketLike);
    let ws: SocketLike;
    try {
      ws = factory(url);
    } catch {
      this.#retry(true);
      return;
    }
    this.#ws = ws;
    let refused = false;
    ws.onopen = () => {
      const hello: CompanionMsg = {
        type: "hello",
        protocol: COMPANION_PROTOCOL,
        plugin: this.#opts.plugin,
        instance: this.#opts.instance,
        app: this.#opts.app,
        ...(this.#opts.appVersion ? { app_version: this.#opts.appVersion } : {}),
        ...(this.#opts.name ? { name: this.#opts.name } : {}),
        ...(this.#opts.version ? { version: this.#opts.version } : {}),
        features: this.#opts.features ?? [],
        ...(this.#opts.token ? { token: this.#opts.token } : {}),
      };
      ws.send(JSON.stringify(hello));
    };
    ws.onmessage = (ev) => {
      let msg: HubMsg;
      try {
        msg = JSON.parse(String(ev.data)) as HubMsg;
      } catch {
        return;
      }
      switch (msg.type) {
        case "ready":
          this.#link = msg.link;
          this.#retryMs = this.#opts.retryMinMs ?? 1000;
          this.#startPing();
          this.#opts.onStatus?.({ state: "connected", link: msg.link, port: this.#port });
          break;
        case "refused":
          refused = true;
          this.#opts.onStatus?.({ state: "refused", reason: msg.reason });
          break;
        case "request":
          void this.#run(msg.id, msg.action, msg.params);
          break;
        default:
          break; // pong, and frame types from a newer hub
      }
    };
    ws.onerror = () => {};
    ws.onclose = () => {
      const wasLinked = this.#link !== undefined;
      this.#teardown();
      if (wasLinked) this.#opts.onStatus?.({ state: "disconnected" });
      // Never linked and not refused: probably the wrong port; try the next one at once.
      this.#retry(!wasLinked && !refused);
    };
  }

  async #run(id: string, action: string, params: unknown): Promise<void> {
    const handler = this.#handlers.get(action);
    let answer: ActionResult;
    if (!handler) {
      answer = { id, ok: false, unsupported: true };
    } else {
      try {
        const result = await handler(params);
        answer = { id, ok: true, result: result === undefined ? null : result };
      } catch (e) {
        answer = e instanceof Unsupported
          ? { id, ok: false, unsupported: true }
          : { id, ok: false, error: e instanceof Error ? e.message : String(e) };
      }
    }
    this.#send({ type: "response", ...answer });
  }

  #send(msg: CompanionMsg): boolean {
    if (!this.connected) return false;
    this.#ws!.send(JSON.stringify(msg));
    return true;
  }

  #startPing(): void {
    clearInterval(this.#pingTimer);
    this.#pingTimer = setInterval(() => this.#send({ type: "ping" }), COMPANION_PING_EVERY_SECS * 1000);
  }

  #teardown(): void {
    clearInterval(this.#pingTimer);
    this.#link = undefined;
    const ws = this.#ws;
    this.#ws = null;
    if (ws) {
      ws.onopen = ws.onmessage = ws.onclose = ws.onerror = null;
      try {
        ws.close();
      } catch {
        /* already closed */
      }
    }
  }

  #retry(nextPort: boolean): void {
    if (this.#stopped) return;
    const ports = this.#ports();
    if (nextPort && (this.#portIndex + 1) % ports.length !== 0) {
      this.#portIndex++;
      this.#connect();
      return;
    }
    if (nextPort) this.#portIndex = 0;
    const wait = this.#retryMs;
    this.#retryMs = Math.min(wait * 2, this.#opts.retryMaxMs ?? 30_000);
    this.#retryTimer = setTimeout(() => this.#connect(), wait);
  }
}
