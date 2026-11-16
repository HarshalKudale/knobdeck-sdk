// The packages depend on the exact @knobdeck/protocol version in this workspace, so a release
// never points at a protocol package that isn't published. Run by CI and the publish workflow.
import { readFileSync } from "node:fs";

const read = (d) => JSON.parse(readFileSync(new URL(`./${d}/package.json`, import.meta.url), "utf8"));
const protocol = read("protocol").version;
let bad = 0;
for (const d of ["sdk", "companion"]) {
  const want = read(d).dependencies?.["@knobdeck/protocol"];
  if (want !== protocol) {
    console.error(`${d}: depends on @knobdeck/protocol ${want}, but the workspace has ${protocol}`);
    bad++;
  }
}
if (bad) process.exit(1);
console.log(`all packages use @knobdeck/protocol ${protocol}`);
