# CLAUDE.md

## What This Is

HAIAI SDK — multi-language SDK (Python, Node, Go, Rust) for the HAI.AI agent benchmarking platform. Thin wrapper around `jacs` for cryptographic identity + HAI platform integration.

## Commands

```bash
make test                # all languages
make test-{python,node,go,rust}  # individual
make versions            # show all package versions
make check-versions      # fail if versions don't match
make release-all         # tag + push all releases (triggers CI publish)
```

## Layout

```
python/src/haiai/        # Python SDK
python/src/jacs/hai/     # Python JACS integration layer
node/src/                # Node SDK (TypeScript, dual ESM/CJS build)
go/                      # Go SDK
rust/haiai/              # Rust library crate
rust/hai-mcp/            # Rust MCP server binary
rust/haiai-cli/          # Rust CLI binary
fixtures/                # Shared cross-language test fixtures (contract-driven)
schemas/                 # JSON schemas for HAI events
scripts/ci/              # CI enforcement (crypto policy denylist)
```

## Rules

1. **No local crypto.** All signing/verification/key ops delegate to `jacs`. CI enforces via `scripts/ci/check_no_local_crypto.sh`.
2. **Cross-language parity.** Behavior changes must apply to all 4 SDKs. Shared fixtures in `fixtures/` drive contract tests.
3. **All 6 packages share one version.** Bump all together: `rust/haiai/Cargo.toml`, `rust/haiai-cli/Cargo.toml`, `rust/hai-mcp/Cargo.toml`, `python/pyproject.toml`, `node/package.json`, `.claude-plugin/plugin.json`.
4. **Releases are tag-triggered.** `rust/v*` → crates.io, `python/v*` → PyPI, `node/v*` → npm. Use `make release-*`.

## Gotchas

- **JACS filenames use `:`** (`{id}:{version}.json`) — illegal on Windows. Rust CI uses sparse checkout for Windows builds.
- **CLI and MCP server are Rust-only.** `cli.ts`, `mcp-server.ts`, `cli.py`, and `mcp_server.py` have been deleted. The `haiai` CLI binary and `haiai mcp` subcommand are the canonical implementations.
- **Python test deps.** Use `pip install -e ".[dev,mcp]"` not just `.[dev]` — MCP tests need the `mcp` package.
- **Path segments must be URL-escaped** in all API paths.
- **Auth header:** `JACS {jacsId}:{timestamp}:{signature_base64}`.
