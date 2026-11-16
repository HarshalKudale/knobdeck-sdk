# KnobDeck's shared Node runner

One Node process for every TypeScript plugin (PLATFORM_PLAN.md decision 3). Each plugin runs in a
`worker_thread`, about 10 MiB instead of ~45 MiB for a Node of its own (§7).

```
KnobDeck ──stdio── shim (one small process per plugin) ──local socket── runner ──worker── plugin
```

- `runner.mjs <address>` (plain JS, run by any Node LTS): listens on a Unix socket path or
  `\\.\pipe\…`. Each connection's first line is `{"token": …, "entry": "<plugin JS>"}` (the token
  from `KNOBDECK_RUNNER_TOKEN`, so other users can't load code into it); then protocol lines pass through unchanged. Closing the
  connection stops the worker (terminated after 2 s); the worker ending closes the connection.
- `shim.ts <address> <entry>`: what the host's native shim will do, in Node, for tests.
- `@knobdeck/sdk` notices it is in a runner worker and talks over the parent port instead of stdio.

Tested by `tests/parity.rs` (`the_shared_runner_gives_the_same_transcript`): two plugins in one
runner give the same frames as the Rust parity plugin. Not yet in the app: the native shim, and
downloading a pinned Node.
