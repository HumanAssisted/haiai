# CLAUDE.md

## What This Is

HAIAI SDK — multi-language SDK (Python, Node, Go, Rust) for the HAI.AI agent benchmarking platform. Thin wrapper around `jacs` for cryptographic identity + HAI platform integration.

The HTTP client is implemented once in Rust and exposed to Python, Node, and Go via FFI bindings (PyO3, napi-rs, CGo). Each SDK is a thin type-safe wrapper that parses JSON responses from the FFI layer into language-native types.

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
python/src/haiai/        # Python SDK (thin wrapper over haiipy FFI)
python/src/jacs/hai/     # Python JACS integration layer
node/src/                # Node SDK (TypeScript, thin wrapper over haiinpm FFI)
go/                      # Go SDK (thin wrapper over haiigo FFI)
go/ffi/                  # Go CGo wrapper for libhaiigo
rust/haiai/              # Rust library crate (HaiClient, the single HTTP implementation)
rust/hai-binding-core/   # Shared JSON-in/JSON-out wrapper for FFI bindings
rust/haiinpm/            # Node.js binding via napi-rs
rust/haiipy/             # Python binding via PyO3
rust/haiigo/             # Go binding via CGo cdylib
rust/hai-mcp/            # Rust MCP server binary
rust/haiai-cli/          # Rust CLI binary
fixtures/                # Shared cross-language test fixtures (contract-driven)
schemas/                 # JSON schemas for HAI events
scripts/ci/              # CI enforcement (crypto policy denylist)
```

## Rules

1. **No local crypto.** All signing/verification/key ops delegate to `jacs`. CI enforces via `scripts/ci/check_no_local_crypto.sh`.
2. **Cross-language parity.** Behavior changes must apply to all 4 SDKs. FFI guarantees parity by construction for HTTP operations. Shared fixtures in `fixtures/` drive contract tests.
3. **All 10 packages share one version.** Bump all together: `rust/haiai/Cargo.toml`, `rust/haiai-cli/Cargo.toml`, `rust/hai-mcp/Cargo.toml`, `rust/hai-binding-core/Cargo.toml`, `rust/haiinpm/Cargo.toml`, `rust/haiipy/Cargo.toml`, `rust/haiigo/Cargo.toml`, `python/pyproject.toml`, `node/package.json`, `.claude-plugin/plugin.json`.
4. **Releases are tag-triggered.** `rust/v*` -> crates.io, `python/v*` -> PyPI, `node/v*` -> npm. Use `make release-*`.
5. **No HTTP clients outside Rust.** All HTTP calls, auth headers, retry logic, and URL building live in `rust/haiai/`. Python/Node/Go SDKs MUST NOT import httpx, fetch, or net/http for API calls.

## Gotchas

- **JACS lives at `~/personal/JACS/jacs` for local dev.** Never re-implement JACS functionality. Rust patches via the commented `[patch.crates-io]` block in `rust/Cargo.toml`; Node uses `npm run deps:local` (rewrites `@hai.ai/jacs` to `file:../../JACS/jacsnpm`); Python uses `pip install -e ../../JACS/jacspy`. See "Local JACS Development" in AGENTS.md.
- **Version bumps must touch lockfiles too.** All 10 manifests plus `rust/Cargo.lock`, `node/package-lock.json`, and `python/uv.lock`. CI fails with `lock file's @hai.ai/jacs@X.Y.Z does not satisfy A.B.C` otherwise.
- **JACS filenames use `:`** (`{id}:{version}.json`) -- illegal on Windows. Rust CI uses sparse checkout for Windows builds.
- **CLI and MCP server are Rust-only.** `cli.ts`, `mcp-server.ts`, `cli.py`, `mcp_server.py`, `go/cmd/haiai/`, and `go/cmd/hai-mcp/` have been deleted. The `haiai` CLI binary and `haiai mcp` subcommand are the canonical implementations.
- **Python test deps.** Use `pip install -e ".[dev,mcp]"` not just `.[dev]` -- MCP tests need the `mcp` package.
- **Path segments must be URL-escaped** in all API paths.
- **Auth header:** `JACS {jacsId}:{timestamp}:{signature_base64}`.
- **FFI build requirements.** All language SDKs now require a Rust toolchain to build from source. CI installs Rust for Python (maturin), Node (napi-rs), and Go (cargo build cdylib).
- **Streaming (SSE/WS) is migrated to FFI.** SSE and WebSocket connections use an opaque handle pattern through binding-core. SDKs call connect/poll/close via FFI.
