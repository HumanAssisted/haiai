# CLAUDE.md

## Project Overview

HAIAI SDK — multi-language SDK (Python, Node, Go, Rust) for the HAI.AI agent benchmarking platform. Wraps `jacs` for cryptographic identity and adds HAI platform integration.

## Key Commands

```bash
make test              # all languages
make test-python       # Python only
make test-node         # Node only
make test-go           # Go only
make test-rust         # Rust only
make versions          # show all package versions
make check-versions    # fail if versions don't match
make release-all       # tag + push all releases (triggers CI publish)
```

## Version Synchronization

All 5 packages MUST have the same version. Check before releasing:
- `rust/haiai/Cargo.toml`, `rust/haiai-cli/Cargo.toml`, `rust/hai-mcp/Cargo.toml`
- `python/pyproject.toml`
- `node/package.json`

Use `make check-versions` to verify.

## Project Layout

```
python/src/haiai/          # Python SDK source
python/src/jacs/hai/       # Python JACS integration layer
python/pyproject.toml      # Python package config (optional deps: mcp, ws, sse, etc.)
python/tests/              # Python tests

node/src/                  # Node SDK source (TypeScript)
node/tsconfig.json         # ESM build config
node/tsconfig.cjs.json     # CJS build config (excludes cli.ts, mcp-server.ts)
node/package.json          # Node package config

go/                        # Go SDK source
rust/haiai/                # Rust library crate
rust/hai-mcp/              # Rust MCP server binary crate
rust/haiai-cli/            # Rust CLI binary crate

fixtures/                  # Shared cross-language test fixtures
schemas/                   # JSON schemas for HAI events
contract/                  # API response contract examples
scripts/ci/                # CI enforcement scripts (crypto policy check)
```

## CI/CD

- **Test workflow** (`.github/workflows/test.yml`): runs on push/PR to main. Tests all 4 languages.
- **Publish workflows**: triggered by git tags, not manual. Use `make release-*`.
  - `rust/v*` tag → `publish-rust.yml` → crates.io + CLI binaries (GitHub Release)
  - `python/v*` tag → `publish-python.yml` → PyPI (trusted publisher OIDC)
  - `node/v*` tag → `publish-node.yml` → npm

## Critical Rules

1. **No local crypto in haiai.** All signing, verification, key generation must delegate to `jacs`. CI enforces this via `scripts/ci/check_no_local_crypto.sh`.
2. **Cross-language parity.** Behavior changes must be applied to all 4 SDKs. Update `docs/HAIAI_LANGUAGE_SYNC_GUIDE.md` first.
3. **Shared fixtures drive tests.** `fixtures/contract_endpoints.json`, `fixtures/cross_lang_test.json`, and `fixtures/mcp_tool_contract.json` define the contract. Tests in each language read these fixtures.
4. **JACS is the upstream dependency.** Don't duplicate jacs functionality. Canonical repos: `jacs` and `jacs-mcp`.

## Known Gotchas

- **JACS filenames use `:` separator** (`{agent_id}:{version_id}.json`). This is illegal on Windows NTFS. The Rust publish workflow uses sparse checkout (`rust/` only) to avoid this on Windows builds.
- **Node dual build (ESM + CJS).** `mcp-server.ts` and `cli.ts` use `import.meta` and are ESM-only entry points (bin scripts). They are excluded from the CJS build in `tsconfig.cjs.json`.
- **Python optional deps.** MCP tests require `pip install -e ".[dev,mcp]"`, not just `.[dev]`. The `mcp` extra pulls in the `mcp` package.
- **Python package uses two source roots:** `src/haiai/` (SDK) and `src/jacs/hai/` (JACS integration layer). Both are included in the wheel via `pyproject.toml` `[tool.hatch.build.targets.wheel]`.
- **Auth header format:** `JACS {jacsId}:{timestamp}:{signature_base64}`. The signed message is `{jacsId}:{timestamp}`.
- **Path segments must be URL-escaped** before interpolation in API paths (agent_id, job_id, message_id, jacs_id).

## Reference Docs

- `docs/HAIAI_LANGUAGE_SYNC_GUIDE.md` — cross-language invariants and parity rules
- `docs/adr/0001-crypto-delegation-to-jacs.md` — why no local crypto
- `docs/JACS_DRY.md` — JACS integration architecture
- `fixtures/README.md` — shared test fixture documentation
