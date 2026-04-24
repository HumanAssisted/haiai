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
6. **Fixture-first parity.** Every API surface (MCP tools, CLI commands, FFI methods) is governed by a JSON fixture in `fixtures/`. Update the fixture before (or alongside) code changes. Tests enforce bidirectional coverage -- see "Parity Enforcement" below.

## Parity Enforcement

Parity is enforced by JSON fixture contracts in `fixtures/`. Each fixture is the source of truth for one API surface. Tests are **bidirectional**: they fail if code adds something not in the fixture OR the fixture declares something not in code. This means you cannot add a new operation without updating the fixture, and you cannot leave stale entries in fixtures.

### Fixtures and what they govern

| Fixture | Surface | Tested in |
|---------|---------|-----------|
| `mcp_tool_contract.json` | MCP tools (27 tools) | `rust/hai-mcp/tests/integration.rs`, `python/tests/test_mcp_parity.py` |
| `cli_command_parity.json` | CLI commands (28 commands) | `rust/haiai-cli/src/main.rs` (mod tests) |
| `ffi_method_parity.json` | FFI binding methods (67 methods) | Language-specific FFI adapter tests |
| `mcp_cli_parity.json` | MCP-to-CLI mapping | `rust/haiai-cli/src/main.rs` (mod tests) |
| `contract_endpoints.json` | HTTP endpoint contracts | `rust/haiai/tests/contract_endpoints.rs`, Python/Node/Go contract tests |
| `cross_lang_test.json` | Auth headers, canonical JSON | `rust/haiai/tests/cross_lang_contract.rs`, `python/tests/test_cross_lang_contract.py` |
| `email_conformance.json` | Email verification contracts | `rust/haiai/tests/email_conformance.rs` |

### MCP-to-CLI parity model

MCP and CLI are intentionally **not** 1:1. The `mcp_cli_parity.json` fixture declares three sections:

- **`paired`** -- MCP tools that have a CLI equivalent (e.g., `hai_send_email` <-> `send-email`)
- **`mcp_only`** -- Tools intentionally MCP-only, with a reason (e.g., `hai_mark_read` -- MCP-only message management)
- **`cli_only`** -- Commands intentionally CLI-only, with a reason (e.g., `init` -- local agent creation)

Every real MCP tool and every real CLI command must appear in exactly one section. The test fails if any tool or command is undeclared, or if a fixture entry references something that doesn't exist.

### How to add a new API operation

1. **Add the Rust implementation** in `rust/haiai/` (endpoint, types, client method).
2. **Add FFI method** in `rust/hai-binding-core/` -- update `ffi_method_parity.json`.
3. **Add MCP tool** in `rust/hai-mcp/src/hai_tools.rs` -- update `mcp_tool_contract.json`.
4. **Add CLI command** in `rust/haiai-cli/src/main.rs` -- update `cli_command_parity.json`.
5. **Map MCP<->CLI** in `mcp_cli_parity.json` (add to `paired`, `mcp_only`, or `cli_only` with reason).
6. **Update language SDKs** (Python/Node/Go wrappers parse the new FFI response).
7. **Run `make test`** -- parity tests will catch anything you missed.

If you add a tool to MCP but not CLI (or vice versa), you must add it to the appropriate `*_only` section with a reason explaining why.

## Raw Email Retrieval

A recipient agent can fetch the exact raw RFC 5322 bytes of any message in
its mailbox and feed them to local JACS verification, without contacting
Stalwart. The server persists the exact bytes that JACS signed on every
send path (PRD R2 byte-fidelity). Inbound messages from external agents
currently require a server-side filter-worker write site still pending in
`hai/api` (see `docs/RAW_EMAIL_RETRIEVAL_ISSUES/RAW_EMAIL_RETRIEVAL_ISSUE_004.md`);
until that lands, inbound raw bytes return `available: false`.

- HTTP: `GET /api/agents/{agent_id}/email/messages/{message_id}/raw`
  → JSON `{ message_id, rfc_message_id, available, raw_email_b64,
  size_bytes, omitted_reason }`.
- Rust: `HaiClient::get_raw_email(message_id) -> RawEmailResponse`
  (decodes base64 into `Vec<u8>` at the client boundary).
- Python: `HaiClient.get_raw_email(hai_url=None, message_id="")`
  (sync) and `AsyncHaiClient.get_raw_email(hai_url, message_id)` (async)
  — returns `RawEmailResult.raw_email: bytes | None`.
- Node: `HaiClient.getRawEmail(messageId): Promise<RawEmailResult>`
  — returns `{ rawEmail: Buffer | null, ... }`.
- Go: `Client.GetRawEmail(ctx, messageID) (*RawEmailResult, error)`
  — returns `{ RawEmail []byte, ... }`.
- MCP: `hai_get_raw_email`. CLI: `haiai get-raw-email <id> [--output F] [--base64]`.
- moltyjacs: `jacs_hai_get_raw_email` delegates through `@haiai/haiai`.

25 MB cap (matches existing attachment limit). Legacy rows predating the
feature return `available: false` with `omitted_reason: "not_stored"`;
oversize rows return `"oversize"`. Recipe + cross-language snippets live
in `docs/haisdk/EMAIL_VERIFICATION.md`.

## Local JACS Development

Each SDK pins a published JACS version for CI/release, but supports local path overrides for development:

- **Node:** `npm run deps:local` switches `@hai.ai/jacs` to `file:../../JACS/jacsnpm`. Use `npm run deps:prod` to switch back. The committed `package.json` must always use the published version.
- **Rust:** Uncomment the `[patch.crates-io]` block in `rust/Cargo.toml` to build against `../../JACS/`. Must be commented out before publish. Building with the patch block (even temporarily) adds `[[patch.unused]]` entries to `rust/Cargo.lock` — the git clean filter in `.gitattributes` strips these automatically on staging. **First-time setup:** run `git config filter.clean-cargo-lock.clean 'sed "/^\[\[patch\.unused\]\]/,/^$/d"'` and `git config filter.clean-cargo-lock.smudge cat` to activate the filter.
- **Python:** Pin in `pyproject.toml` (`jacs==X.Y.Z`). For local dev, use `pip install -e ../../JACS/jacspy` (or equivalent) to shadow the published version.

`make check-jacs-versions` verifies all SDKs agree on the published JACS version.

## Gotchas

- **JACS filenames use `:`** (`{id}:{version}.json`) -- illegal on Windows. Rust CI uses sparse checkout for Windows builds.
- **CLI and MCP server are Rust-only.** `cli.ts`, `mcp-server.ts`, `cli.py`, `mcp_server.py`, `go/cmd/haiai/`, and `go/cmd/hai-mcp/` have been deleted. The `haiai` CLI binary and `haiai mcp` subcommand are the canonical implementations.
- **Python test deps.** Use `pip install -e ".[dev,mcp]"` not just `.[dev]`.
- **Path segments must be URL-escaped** in all API paths.
- **Auth header:** `JACS {jacsId}:{timestamp}:{signature_base64}`.
- **FFI build requirements.** All language SDKs require a Rust toolchain to build from source. CI installs Rust for Python (maturin), Node (napi-rs), and Go (cargo build cdylib).
- **Streaming (SSE/WS) is migrated to FFI.** SSE and WebSocket connections use an opaque handle pattern through binding-core. SDKs call connect/poll/close via FFI.
