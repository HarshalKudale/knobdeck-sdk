// KnobDeck's shared Node runner: ONE Node process for every TypeScript plugin.
//
// It listens on a local socket (a Unix socket path, or `\\.\pipe\…` on Windows). Each connection is
// one plugin: the first line is `{"entry": "<absolute path to the plugin's JS>"}`, then raw protocol
// lines both ways. The plugin runs in a worker_thread (~10 MiB each instead of ~45 MiB for its own
// Node; PLATFORM_PLAN.md §7); @knobdeck/sdk notices the worker and talks over the parent port.
// When the connection closes the worker is told to stop, and terminated 2 s later; when the worker
// ends, the connection closes, so KnobDeck sees the plugin exit.
//
// Usage: node runner.js <socket path or pipe name>

import { createServer, type Socket } from "node:net";
import { Worker } from "node:worker_threads";
import { existsSync, unlinkSync } from "node:fs";

const address = process.argv[2];
if (!address) {
  process.stderr.write("usage: runner <socket path or \\\\.\\pipe\\name>\n");
  process.exit(2);
}

function log(msg: string): void {
  process.stderr.write(`[runner ${new Date().toISOString()}] ${msg}\n`);
}

function serve(sock: Socket): void {
  let buf = "";
  let worker: Worker | undefined;
  let entry = "";
  const toWorker = (line: string) => worker?.postMessage(line);
  sock.setEncoding("utf8");
  sock.on("data", (chunk: string) => {
    buf += chunk;
    let i: number;
    while ((i = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, i).replace(/\r$/, "");
      buf = buf.slice(i + 1);
      if (worker) {
        toWorker(line);
        continue;
      }
      try {
        entry = String(JSON.parse(line).entry ?? "");
      } catch {
        entry = "";
      }
      if (!entry) {
        log("first line must be {\"entry\": …}; closing");
        sock.destroy();
        return;
      }
      log(`loading ${entry}`);
      worker = new Worker(entry, { workerData: { knobdeck: true } });
      worker.on("message", (l: unknown) => {
        if (typeof l === "string" && !sock.destroyed) sock.write(l + "\n");
      });
      worker.on("error", (e) => log(`${entry}: ${e instanceof Error ? e.stack ?? e.message : e}`));
      worker.on("exit", (code) => {
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

if (!address.startsWith("\\\\") && existsSync(address)) unlinkSync(address);
createServer(serve).listen(address, () => log(`listening on ${address}`));
