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
2. **Cross-language parity.** Changes must apply to all 4 SDKs. Tests read shared fixtures from `fixtures/`.
3. **All 5 packages share one version.** `make check-versions` to verify.
4. **Releases are tag-triggered.** `rust/v*` → crates.io, `python/v*` → PyPI, `node/v*` → npm. Use `make release-*`.

## Gotchas

- **JACS filenames use `:`** (`{id}:{version}.json`) — illegal on Windows. Rust CI uses sparse checkout for Windows builds.
- **CLI and MCP server are Rust-only.** `cli.ts`, `mcp-server.ts`, `cli.py`, and `mcp_server.py` have been deleted. The `haiai` CLI binary and `haiai mcp` subcommand are the canonical implementations.
- **Python test deps.** Use `pip install -e ".[dev,mcp]"` not just `.[dev]`.
- **Path segments must be URL-escaped** in all API paths.
- **Auth header:** `JACS {jacsId}:{timestamp}:{signature_base64}`.
