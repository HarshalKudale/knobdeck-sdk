// Node-only helpers (Blender-over-JS, Electron apps, scripts): read the hub's discovery file.

import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { COMPANION_DISCOVERY_FILE } from "../../../knobdeck-protocol/typescript/protocol.ts";
import type { Discovery } from "../../../knobdeck-protocol/typescript/protocol.ts";

export type { Discovery };

/** KnobDeck's data folder: where the app writes `companion-hub.json`. */
export function dataDir(platform: string = process.platform, env: Record<string, string | undefined> = process.env): string {
  if (platform === "win32") return join(env.APPDATA ?? join(homedir(), "AppData", "Roaming"), "KnobDeck");
  if (platform === "darwin") return join(homedir(), "Library", "Application Support", "KnobDeck");
  return join(env.XDG_CONFIG_HOME ?? join(homedir(), ".config"), "KnobDeck");
}

/** The hub's port and token, or `null` when KnobDeck hasn't written the file (not running yet). */
export function readDiscovery(path: string = join(dataDir(), COMPANION_DISCOVERY_FILE)): Discovery | null {
  try {
    return JSON.parse(readFileSync(path, "utf8")) as Discovery;
  } catch {
    return null;
  }
}
