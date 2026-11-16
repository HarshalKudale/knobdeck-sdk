# Releasing the npm packages

Three packages, published by `.github/workflows/publish-npm.yml` when `master` carries a version
that isn't on npm yet:

| Package | What | Depends on |
|---|---|---|
| `@knobdeck/protocol` | generated types and constants (don't edit `src/index.ts`) | — |
| `@knobdeck/sdk` | TypeScript plugins | the exact `@knobdeck/protocol` version |
| `@knobdeck/companion` | companions inside other apps | the exact `@knobdeck/protocol` version |

## One-time setup (owner)

Publishing uses npm **Trusted Publishing**: GitHub proves to npm that the upload comes from this
repository's `publish-npm.yml`, so there is no token to leak or renew. npm only lets you attach a
trusted publisher to a package that exists, so `0.1.0` goes up once by hand.

1. On npmjs.com, create the organization **`knobdeck`** (free; public packages) and turn on
   two-factor authentication for your account.
2. **First publish, from your machine**, from a checkout of this repo:

   ```sh
   cd typescript
   npm ci && npm run build && npm run check && npm test
   npm login
   npm publish --workspace protocol  --access public --provenance=false
   npm publish --workspace sdk       --access public --provenance=false
   npm publish --workspace companion --access public --provenance=false
   ```

   (`--provenance=false` because provenance can only be made in CI; later versions get it.)
   Protocol first: the other two depend on it.
3. For **each** of the three packages: npmjs.com → the package → Settings → **Trusted Publisher** →
   GitHub Actions:
   - Organization or user: `Knobdeck`
   - Repository: `knobdeck-sdk`
   - Workflow filename: `publish-npm.yml`
   - Environment name: `npm`
4. Same page, **Publishing access**: "Require two-factor authentication and disallow tokens". Only
   the workflow (and you, with 2FA) can publish.
5. Optional: GitHub → Knobdeck/knobdeck-sdk → Settings → Environments → `npm` (it appears after the
   workflow's first run, or create it) → Required reviewers: you. Each release then waits for your
   approval.
6. Merge. The workflow sees `0.1.0` is already on npm and skips it; the next version bump publishes
   through Actions.

No secrets are needed in GitHub.

## Each release

1. If the protocol changed: bump `typescript/protocol/package.json`, and the `@knobdeck/protocol`
   dependency in `sdk` and `companion` to the same version (`npm run versions` checks this).
2. Bump `sdk` and/or `companion`.
3. `cd typescript && npm install && npm run build && npm run check && npm test`.
4. Merge to `master`. Versions already on npm are skipped, so unchanged packages are left alone.

Semver: until 1.0, a minor bump (0.x → 0.y) may break the TypeScript API; the wire protocol itself
follows `COMPAT.md` regardless.
