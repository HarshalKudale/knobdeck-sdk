// What KnobDeck's native shim does, in Node, for tests: be the plugin process the host starts, and
// relay its stdin/stdout to the shared runner.
//
// Usage: node shim.js <runner address> <plugin entry>

import { connect } from "node:net";

const [address, entry] = process.argv.slice(2);
const sock = connect(address, () => {
  sock.write(JSON.stringify({ entry }) + "\n");
  process.stdin.pipe(sock);
});
sock.pipe(process.stdout);
sock.on("close", () => process.exit(0));
sock.on("error", (e) => {
  process.stderr.write(`shim: ${e.message}\n`);
  process.exit(1);
});
