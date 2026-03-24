# AGENTS.md

## Purpose

HAIAI SDK -- HAI platform integration layer around `jacs`. Helps agents use JACS identity/provenance to register with HAI, receive benchmark jobs, interact with other agents, and send/receive agent email.

## Ownership Boundary

**`jacs` owns:** key generation, key encryption/decryption, canonicalization, document signing/verification, cryptographic primitives.

**`haiai` owns:** HAI endpoint contracts, JACS-authenticated API calls (`Authorization: JACS ...`), registration/verification workflows, SSE/WS transport, email APIs, integration wrappers delegating to JACS adapters.

**`haiai` HTTP client is Rust-only.** All HTTP calls, auth headers, retry logic, and URL building live in `rust/haiai/`. Python, Node, and Go SDKs delegate to the Rust client via FFI bindings (hai-binding-core).

## Layout

```
python/src/haiai/        # Python SDK (thin wrapper over haiipy FFI binding)
python/src/jacs/hai/     # Python JACS integration layer (two source roots in wheel)
node/src/                # Node SDK (TypeScript, thin wrapper over haiinpm FFI binding)
go/                      # Go SDK (thin wrapper over haiigo FFI binding)
go/ffi/                  # Go CGo wrapper for libhaiigo cdylib
rust/haiai/              # Rust library crate (HaiClient -- the single HTTP implementation)
rust/hai-binding-core/   # Shared JSON-in/JSON-out FFI core (wraps HaiClient for all bindings)
rust/haiinpm/            # Node.js binding via napi-rs (returns Promise<string>)
rust/haiipy/             # Python binding via PyO3 (async + sync variants)
rust/haiigo/             # Go binding via CGo cdylib (spawn+channel pattern)
rust/hai-mcp/            # Rust MCP server binary
rust/haiai-cli/          # Rust CLI binary
fixtures/                # Shared cross-language test fixtures
schemas/                 # JSON schemas for HAI events
scripts/ci/              # CI enforcement (crypto policy denylist)
```

## FFI Architecture

```
                    +-------------+
                    |  HAI API    |
                    |  (REST)     |
                    +------+------+
                           |
                    +------+------+
                    | Rust        |
                    | HaiClient   |
                    | (reqwest)   |
                    +------+------+
                           |
                  +--------+--------+
                  |                  |
           +------+------+  +------+------+
           |hai-binding-  |  | hai-mcp /   |
           |core (JSON)   |  | haiai-cli   |
           +------+-------+  +-------------+
                  |
       +----------+----------+
       |          |          |
  +----+----+ +---+---+ +---+----+
  | haiipy  | |haiinpm| | haiigo |
  | (PyO3)  | |(napi) | | (CGo)  |
  +---------+ +-------+ +--------+
```

## Trait Architecture (JACS 0.9.4)

The Rust SDK exposes JACS capabilities through 8 layered extension traits defined in `rust/haiai/src/jacs.rs`, implemented in `rust/haiai/src/jacs_local.rs`:

- **Layer 0** `JacsProvider` -- Core signing, identity, canonical JSON
- **Layer 1** `JacsAgentLifecycle` -- Key rotation, migration, diagnostics, quickstart
- **Layer 2** `JacsDocumentProvider` -- Document CRUD, versioning, search
- **Layer 3** `JacsBatchProvider` -- Batch sign/verify
- **Layer 4** `JacsVerificationProvider` -- Document verification, DNS trust, auth headers
- **Layer 5** `JacsEmailProvider` -- Email signing/verification, attachments
- **Layer 6** `JacsAgreementProvider` -- Multi-party agreements (feature: `agreements`)
- **Layer 7** `JacsAttestationProvider` -- Attestation claims (feature: `attestation`)

Storage backend selection: `rust/haiai/src/config.rs` (`resolve_storage_backend()`). Labels: `fs`, `rusqlite`, `sqlite` (alias).

Full parity map: `docs/haisdk/PARITY_MAP.md` (53 exposed, 18 excluded, 71 total).

## Rules

1. **No local crypto in haiai.** Delegate to `jacs`. CI enforces via `scripts/ci/check_no_local_crypto.sh`.
2. **Cross-language parity.** Changes must apply to all 4 SDKs. FFI guarantees HTTP parity by construction. Tests read shared fixtures from `fixtures/`.
3. **All 10 packages share one version.** `make check-versions` to verify.
4. **Releases are tag-triggered.** `rust/v*` -> crates.io, `python/v*` -> PyPI, `node/v*` -> npm. Use `make release-*`.
5. **No HTTP clients outside Rust.** Python/Node/Go SDKs MUST NOT use httpx/fetch/net-http for API calls. All HTTP, retry, auth, and URL logic lives in `rust/haiai/`.

## Local JACS Development

Each SDK pins a published JACS version for CI/release, but supports local path overrides for development:

- **Node:** `npm run deps:local` switches `@hai.ai/jacs` to `file:../../JACS/jacsnpm`. Use `npm run deps:prod` to switch back. The committed `package.json` must always use the published version.
- **Rust:** Uncomment the `[patch.crates-io]` block in `rust/Cargo.toml` to build against `../../JACS/`. Must be commented out before publish.
- **Python:** Pin in `pyproject.toml` (`jacs==X.Y.Z`). For local dev, use `pip install -e ../../JACS/jacspy` (or equivalent) to shadow the published version.

`make check-jacs-versions` verifies all SDKs agree on the published JACS version.

## Gotchas

- **JACS filenames use `:`** (`{id}:{version}.json`) -- illegal on Windows. Rust CI uses sparse checkout for Windows builds.
- **CLI and MCP server are Rust-only.** `cli.ts`, `mcp-server.ts`, `cli.py`, `mcp_server.py`, `go/cmd/haiai/`, and `go/cmd/hai-mcp/` have been deleted. The `haiai` CLI binary and `haiai mcp` subcommand are the canonical implementations.
- **Python test deps.** Use `pip install -e ".[dev,mcp]"` not just `.[dev]`.
- **Path segments must be URL-escaped** in all API paths.
- **Auth header:** `JACS {jacsId}:{timestamp}:{signature_base64}`.
- **FFI build requirements.** All language SDKs require a Rust toolchain to build from source. CI installs Rust for Python (maturin), Node (napi-rs), and Go (cargo build cdylib).
- **Streaming (SSE/WS) not yet FFI-migrated.** Native SSE/WS implementations remain in each SDK temporarily. Phase 2 of FFI migration.
