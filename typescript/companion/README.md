# @knobdeck/companion

Connect an add-on inside another app (a Photoshop UXP panel, a Figma plugin, a browser
extension, a Node-based add-on) to its KnobDeck plugin through KnobDeck's companion hub. No
dependencies; works wherever there is a `WebSocket`.

```ts
import { Companion, Unsupported } from "@knobdeck/companion";
import { readDiscovery } from "@knobdeck/companion/node"; // Node only

const hub = readDiscovery(); // port + token; browser extensions skip this
const companion = new Companion({
  plugin: "photoshop",            // the KnobDeck plugin's id
  instance: storedRandomId,       // stable per install / window
  app: "photoshop",
  appVersion: host.version,
  token: hub?.token,
  ports: hub ? [hub.port] : undefined,
  onStatus: (s) => console.log("knobdeck", s),
})
  .action("set-tool", ({ tool }: any) => app.setTool(tool))   // its return value is the result
  .action("flatten", () => { throw new Unsupported(); })      // the app has no API for it
  .start();

app.on("toolChanged", (tool) => companion.emit("tool_changed", { tool }));
```

On the plugin side (Rust): `Plugin::companions()` and `knobdeck_sdk::companion::request(None,
"set-tool", json!({"tool": "brush"}))`. The frames are the generated layer-2 types
(`@knobdeck/protocol`); the spec is `knobdeck-protocol/REFERENCE.md`.

Behaviour: tries each hub port until one answers, reconnects with a growing delay (1 s → 30 s),
pings every 20 s, answers `unsupported` for actions it doesn't know, and drops `emit` calls while
disconnected (`emit` returns `false`).

```sh
npm install && npm run check && npm test
```
