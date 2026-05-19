# Releasing HAIAI

HAIAI ships as 12 packages that share one version (HAIAI_WASM_PRD §4.11):

| Package | Manifest | Registry | Tag |
|---------|----------|----------|-----|
| `haiai` (Rust core) | `rust/haiai/Cargo.toml` | crates.io | `rust/v*` |
| `haiai-cli` | `rust/haiai-cli/Cargo.toml` | crates.io + GitHub Release | `rust/v*` |
| `hai-mcp` | `rust/hai-mcp/Cargo.toml` | crates.io | `rust/v*` |
| `hai-binding-core` | `rust/hai-binding-core/Cargo.toml` | crates.io | `rust/v*` |
| `haiinpm` (Node FFI) | `rust/haiinpm/Cargo.toml` | npm (`haiinpm`) | `node/v*` |
| `haiipy` (Python FFI) | `rust/haiipy/Cargo.toml` | PyPI | `python/v*` |
| `haiigo` (Go FFI cdylib) | `rust/haiigo/Cargo.toml` | GitHub Release | `node/v*` (cdylib only) |
| `@haiai/haiai` (Node SDK) | `node/package.json` | npm | `node/v*` |
| `haiai` (Python SDK) | `python/pyproject.toml` | PyPI | `python/v*` |
| Claude Code plugin | `.claude-plugin/plugin.json` | (in-repo only) | — |
| `haiai-wasm` (Rust browser crate) | `rust/haiai-wasm/Cargo.toml` | (not published to crates.io; consumed via wasm-pack output) | `haiai-wasm-v*` |
| `@haiai/wasm` (npm browser pkg) | `node-wasm/package.template.json` | npm | `haiai-wasm-v*` |

## Standard release flow

```bash
# 1. Bump versions in all 12 manifests (and lockfiles).
$EDITOR rust/haiai/Cargo.toml      # → e.g. 0.4.1 → 0.4.2
$EDITOR rust/haiai-cli/Cargo.toml  # ... and the remaining 10
$EDITOR rust/haiai-wasm/Cargo.toml
$EDITOR node-wasm/package.template.json

# 2. Verify all versions match.
make versions
make check-versions       # exits non-zero if any drift

# 3. Update CHANGELOG.md (entry per version).

# 4. Commit and push to main.
git commit -am "Bump to v0.4.2"
git push origin main

# 5. Tag + push triggers CI publish (per-tag → matching workflow):
make release-rust         # rust/v0.4.2 → crates.io + GitHub Release CLI binaries
make release-python       # python/v0.4.2 → PyPI
make release-node         # node/v0.4.2 → npm
make release-haiai-wasm   # haiai-wasm-v0.4.2 → npm (@haiai/wasm)

# Or all four in dependency order:
make release-all          # crates → python → node (waits between)
# Note: release-all does NOT auto-trigger haiai-wasm; run it after JACS
# (@jacs/wasm) is on npm at the matching version.
```

## Browser package release notes

The `haiai-wasm-vX.Y.Z` tag triggers `.github/workflows/release-haiai-wasm.yml`:

1. Asserts the resolved `VERSION` env var is non-empty (JACS_WASM ISSUE 001 lesson — empty version would publish over an existing release).
2. Refuses to publish if `@jacs/wasm@${VERSION}` is not already on npm (`npm view`). HAIAI's browser package depends on it at runtime.
3. Runs the wasm policy gates (`forbidden-deps-wasm.sh`, `check_wasm_surface.sh`).
4. Builds via `wasm-pack build --target web --release rust/haiai-wasm`.
5. Runs `node-wasm/scripts/finalize-pkg.sh` to merge the published `package.json` shape into `rust/haiai-wasm/pkg/`.
6. `npm publish --access public --provenance` from `rust/haiai-wasm/pkg/`.

A failed publish (e.g. crates.io transient flake, npm 5xx) is recoverable via:

```bash
make retry-haiai-wasm
```

which deletes the local + remote tag and re-pushes it.

## CI surface

PR runs (`.github/workflows/test.yml::wasm-checks` job) all of:

- `forbidden-deps-wasm.sh` (allowlist on the wasm tree)
- `check_no_local_crypto.sh` (no rolling our own crypto)
- `check_wasm_surface.sh` (every JS method maps to a `HaiClient` Rust method)
- `cargo check -p haiai --no-default-features --features wasm --target wasm32-unknown-unknown`
- `cargo check -p haiai-wasm --target wasm32-unknown-unknown`
- `tsc --noEmit -p node-wasm/tsconfig.json`
- Build wasm-pack + finalize-pkg + Vite + Playwright smoke (`node-wasm/examples/vite-smoke/`)

All wasm checks are PR-blocking per HAIAI_WASM_PRD §8 + JACS ISSUE 003 lesson.

## Required GitHub Secrets

- `CRATES_IO_TOKEN` — used by `rust/v*` to publish to crates.io
- `PYPI_API_TOKEN` — used by `python/v*` for PyPI
- `NPM_TOKEN` — used by `node/v*` AND `haiai-wasm-v*` for npm
- (provenance) workflows use `id-token: write` permissions to attest provenance via npm's sigstore endpoint
