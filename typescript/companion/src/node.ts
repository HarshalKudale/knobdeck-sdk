// Node-only helpers (Blender-over-JS, Electron apps, scripts): read the hub's discovery file.

import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { COMPANION_DISCOVERY_FILE } from "@knobdeck/protocol";
import type { Discovery } from "@knobdeck/protocol";

export type { Discovery };

/** KnobDeck's data folder: where the app writes `companion-hub.json`. */
export function dataDir(platform: string = process.platform, env: Record<string, string | undefined> = process.env): string {
  if (platform === "win32") return join(env.APPDATA ?? join(homedir(), "AppData", "Roaming"), "KnobDeck");
  if (platform === "darwin") return join(homedir(), "Library", "Application Support", "KnobDeck");
  return join(env.XDG_CONFIG_HOME ?? join(homedir(), ".config"), "KnobDeck");
}

/** `token` and `ports` options that read the discovery file again on every connection attempt,
 * so a companion keeps working across KnobDeck restarts (each start writes a new token, and may
 * pick another port):
 *
 * ```ts
 * new Companion({ plugin, instance, app, ...fromDiscovery() }).start();
 * ```
 */
export function fromDiscovery(path?: string): { token: () => string | undefined; ports: () => readonly number[] | undefined } {
  return {
    token: () => readDiscovery(path)?.token,
    ports: () => {
      const d = readDiscovery(path);
      return d ? [d.port] : undefined;
    },
  };
}

/** The hub's port and token, or `null` when KnobDeck hasn't written the file (not running yet). */
export function readDiscovery(path: string = join(dataDir(), COMPANION_DISCOVERY_FILE)): Discovery | null {
  try {
    return JSON.parse(readFileSync(path, "utf8")) as Discovery;
  } catch {
    return null;
  }
}
