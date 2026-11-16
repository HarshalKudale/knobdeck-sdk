// KnobDeck's shared Node runner: ONE Node process for every TypeScript plugin. Plain JavaScript, so
// any Node LTS runs it as is; KnobDeck ships it (knobdeck_protocol::node::RUNNER_JS) and starts it
// on the first TypeScript plugin.
//
// It listens on a local socket (a Unix socket path, or `\\.\pipe\…` on Windows). Each connection is
// one plugin. The first line is `{"token": "<KNOBDECK_RUNNER_TOKEN>", "entry": "<plugin JS>"}`, then
// raw protocol lines both ways. The token keeps other users on the machine from loading code into
// this user's runner. The plugin runs in a worker_thread (~10 MiB each instead of ~45 MiB for its
// own Node); @knobdeck/sdk notices the worker and talks over the parent port. When the connection
// closes the worker is told to stop and terminated 2 s later; when the worker ends, the connection
// closes, so KnobDeck sees the plugin exit.
//
// Usage: KNOBDECK_RUNNER_TOKEN=… node runner.mjs <socket path or pipe name> [--exit-with-stdin]
// `--exit-with-stdin`: exit when stdin closes; KnobDeck holds it, so the runner ends with the app.

import { createServer } from "node:net";
import { Worker } from "node:worker_threads";
import { existsSync, unlinkSync } from "node:fs";
import { timingSafeEqual } from "node:crypto";

const address = process.argv[2];
const token = process.env.KNOBDECK_RUNNER_TOKEN ?? "";
if (!address || !token) {
  process.stderr.write("usage: KNOBDECK_RUNNER_TOKEN=… runner <socket path or \\\\.\\pipe\\name>\n");
  process.exit(2);
}

/** @param {string} msg */
function log(msg) {
  process.stderr.write(`[runner ${new Date().toISOString()}] ${msg}\n`);
}

/** @param {string} given */
function tokenOk(given) {
  const a = Buffer.from(given);
  const b = Buffer.from(token);
  return a.length === b.length && timingSafeEqual(a, b);
}

/** @param {import("node:net").Socket} sock */
function serve(sock) {
  let buf = "";
  /** @type {Worker | undefined} */
  let worker;
  sock.setEncoding("utf8");
  sock.on("data", (/** @type {string} */ chunk) => {
    buf += chunk;
    let i;
    while ((i = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, i).replace(/\r$/, "");
      buf = buf.slice(i + 1);
      if (worker) {
        worker.postMessage(line);
        continue;
      }
      /** @type {{token?: unknown, entry?: unknown}} */
      let first = {};
      try {
        first = JSON.parse(line);
      } catch {}
      if (!tokenOk(String(first.token ?? ""))) {
        log("refused a connection: wrong token");
        sock.destroy();
        return;
      }
      const entry = String(first.entry ?? "");
      if (!entry) {
        log('first line must carry "entry"; closing');
        sock.destroy();
        return;
      }
      log(`loading ${entry}`);
      const w = new Worker(entry, { workerData: { knobdeck: true } });
      worker = w;
      w.on("message", (l) => {
        if (typeof l === "string" && !sock.destroyed) sock.write(l + "\n");
      });
      w.on("error", (e) => log(`${entry}: ${e instanceof Error ? e.stack ?? e.message : e}`));
      w.on("exit", (code) => {
        log(`${entry} ended (${code})`);
        sock.end();
      });
    }
  });
  sock.on("close", () => {
    if (!worker) return;
    worker.postMessage(null); // like stdin closing
    const w = worker;
    setTimeout(() => void w.terminate(), 2000).unref();
  });
  sock.on("error", () => {});
}

if (process.argv.includes("--exit-with-stdin")) {
  process.stdin.on("end", () => process.exit(0));
  process.stdin.on("error", () => process.exit(0));
  process.stdin.resume();
}

if (!address.startsWith("\\\\") && existsSync(address)) unlinkSync(address);
createServer(serve).listen(address, () => log(`listening on ${address}`));
