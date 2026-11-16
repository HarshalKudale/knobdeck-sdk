# Releasing the npm packages

Three packages, published by `.github/workflows/publish-npm.yml` when `master` carries a version
that isn't on npm yet:

| Package | What | Depends on |
|---|---|---|
| `@knobdeck/protocol` | generated types and constants (don't edit `src/index.ts`) | — |
| `@knobdeck/sdk` | TypeScript plugins | the exact `@knobdeck/protocol` version |
| `@knobdeck/companion` | companions inside other apps | the exact `@knobdeck/protocol` version |

## One-time setup (owner)

1. On npmjs.com, **create the organization `knobdeck`** (free plan; public packages). Its scope is
   `@knobdeck`.
2. Turn on two-factor authentication for your npm account.
3. **Create a granular access token**: npmjs.com → your avatar → Access Tokens → Generate New Token
   → Granular Access Token. Name it `knobdeck-sdk GitHub Actions`; Packages and scopes: **Read and
   write**, scope **@knobdeck** (for the very first publish, choose "All packages" if the scope isn't
   offered yet); Organizations: none; set an expiry and a calendar reminder to renew it.
4. On GitHub, Knobdeck/knobdeck-sdk → Settings → Secrets and variables → Actions → **New repository
   secret** `NPM_TOKEN` with that token.
5. Merge. The workflow publishes `0.1.0` of all three.

Optional, after the first publish: switch to npm **trusted publishing** (no token at all). On each
package's npm page → Settings → Trusted Publisher → GitHub Actions, repository
`Knobdeck/knobdeck-sdk`, workflow `publish-npm.yml`. Then delete the secret and the
`NODE_AUTH_TOKEN` line, and add a step that updates npm to 11.5.1 or newer (Node 22 ships npm 10).

## Each release

1. If the protocol changed: bump `typescript/protocol/package.json`, and the `@knobdeck/protocol`
   dependency in `sdk` and `companion` to the same version (`npm run versions` checks this).
2. Bump `sdk` and/or `companion`.
3. `cd typescript && npm install && npm run build && npm run check && npm test`.
4. Merge to `master`. Versions already on npm are skipped, so unchanged packages are left alone.

Semver: until 1.0, a minor bump (0.x → 0.y) may break the TypeScript API; the wire protocol itself
follows `COMPAT.md` regardless.
