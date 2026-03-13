# AGENTS.md

## Purpose

HAIAI SDK — HAI platform integration layer around `jacs`. Helps agents use JACS identity/provenance to register with HAI, receive benchmark jobs, interact with other agents, and send/receive agent email.

## Ownership Boundary

**`jacs` owns:** key generation, key encryption/decryption, canonicalization, document signing/verification, cryptographic primitives.

**`haiai` owns:** HAI endpoint contracts, JACS-authenticated API calls (`Authorization: JACS ...`), registration/verification workflows, SSE/WS transport, email APIs, integration wrappers delegating to JACS adapters.

## Layout

```
python/src/haiai/        # Python SDK
python/src/jacs/hai/     # Python JACS integration layer (two source roots in wheel)
node/src/                # Node SDK (TypeScript, dual ESM/CJS)
go/                      # Go SDK
rust/haiai/              # Rust library crate
rust/hai-mcp/            # Rust MCP server binary
rust/haiai-cli/          # Rust CLI binary
fixtures/                # Shared cross-language test fixtures
schemas/                 # JSON schemas for HAI events
scripts/ci/              # CI enforcement (crypto policy denylist)
```

## Rules

1. **No local crypto in haiai.** Delegate to `jacs`. CI enforces via `scripts/ci/check_no_local_crypto.sh`.
2. **Cross-language parity.** Changes must apply to all 4 SDKs. Tests read shared fixtures from `fixtures/`.
3. **All 5 packages share one version.** `make check-versions` to verify.
4. **Releases are tag-triggered.** `rust/v*` → crates.io, `python/v*` → PyPI, `node/v*` → npm. Use `make release-*`.

## Gotchas

- **JACS filenames use `:`** (`{id}:{version}.json`) — illegal on Windows. Rust CI uses sparse checkout for Windows builds.
- **CLI and MCP server are Rust-only.** `cli.ts`, `mcp-server.ts`, `cli.py`, and `mcp_server.py` have been deleted. The `haiai` CLI binary and `haiai mcp` subcommand are the canonical implementations.
- **Python test deps.** Use `pip install -e ".[dev,mcp]"` not just `.[dev]`.
- **Path segments must be URL-escaped** in all API paths.
- **Auth header:** `JACS {jacsId}:{timestamp}:{signature_base64}`.
