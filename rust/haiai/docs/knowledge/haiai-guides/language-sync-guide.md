# HAIAI Language Sync Guide

## Purpose

This document is the source of truth for **HAIAI-specific** behavior that must stay aligned across language SDKs (`python`, `node`, `go`, `rust`).

Use this guide whenever HAIAI behavior changes so each language implementation can be updated consistently.

## Layer Boundaries

### What belongs in `jacs`

`jacs` owns identity and document crypto primitives:

1. key generation
2. key encryption/decryption
3. canonicalization used for signatures
4. signature creation/verification
5. signed JACS document helpers

### What belongs in `jacs-mcp`

`jacs-mcp` owns MCP-level JACS tool surface and protocol packaging around JACS concepts.

### What belongs in `haiai`

`haiai` owns HAI-platform integration behavior:

1. HAI endpoint contracts and request/response shaping
2. JACS auth header usage (`Authorization: JACS ...`) and when auth is required
3. transport behavior for HAI event APIs (SSE/WS)
4. benchmark orchestration (free/pro/enterprise tier flows)
5. username, email, key-discovery, and HAI verification endpoints
6. verify-link generation rules for `hai.ai` verifier URLs

## Protocol ownership and current adapters

JACS owns cryptographic signing, verification, key handling, and canonicalization.
HAIAI owns HAI request contracts and transport. Python, Node, and Go call the
shared Rust HTTP client through FFI; that client asks its `JacsProvider` to sign.
This is implementation reuse. Agent signatures establish provenance under the
verifier's checks, without establishing human approval or principal-to-agent
delegation of authority. Principal delegation is unsupported and unscheduled.
JACS's implemented native-root authorization of an ES256 export key remains a
separate key-binding mechanism.

Missing cryptographic binding support must fail with an actionable dependency
error. Do not substitute local crypto or treat a test mock as runtime support.
See [ADR 0001](adr/0001-crypto-delegation-to-jacs.md).

This ownership rule does not establish discovery parity. Rust/Go A2A bundle
builders and Go DNS generation still construct legacy material; the Rust DNS
verifier also uses a legacy field/digest contract. Those paths must not be
described as producing the current JACS-bound discovery bundle until they pass
the canonical JACS verification contract. Discovery parity belongs to I3.

Agreement v2 inspection is also bounded: `mathematicalChecksValid` reports the
mathematical checks; `valid` and `policyAccepted` remain `false`, with
`overallScope: "consent_signatures_only"`. It does not accept application policy
or establish a person's assent.

## Cross-Language Invariants

These must match in all SDKs.

### Canonical dependency pins

When updating Rust integrations, use these canonical upstream repos as references:

1. `~/personal/JACS/jacs`
2. `~/personal/JACS/jacs-mcp`

The checked-in JACS dependencies target `0.13.0`. Check language manifests and
run `make check-jacs-versions` and `make check-versions` before release. Local path
overrides are development inputs, not evidence of published-package parity.

[CI's JACS source checkout](../.github/workflows/test.yml) separately pins
`992953ea77d4c9a16953aee28e4a1e5d26e62200`, the native source validated for this
integration. Each job shallow-fetches that exact commit, checks it out with
detached HEAD, and runs `make check-jacs-versions JACS_SOURCE_DIR=...` against
the fetched source. The check compares its native manifests with the separate
CI `JACS_VERSION: 0.13.0` expectation and all SDK package pins. Version bumps
update that expectation and select the corresponding `crate/v...` release tag.
The JACS package version remains `0.13.0` and HAIAI remains `0.4.1`.

### Authentication header format

Authenticated operations use `Authorization: JACS v2.<claims>.<signature>`.
The shared Rust transport delegates to JACS with the final method, absolute URL
(including path/query), exact serialized body bytes, and deployment-pinned
audience. JACS binds these to the signing identity/key, time, and fresh nonce.
Bodies must be bounded bytes, at most 10 MiB; authenticated redirects and
streaming request bodies are refused. Each retry builds a fresh proof.

Caller-built requests use `build_request_auth_header` (Rust/Python),
`buildRequestAuthHeader` (Node), or `BuildRequestAuthHeader` (Go). Send the exact
bytes once to the supplied final URL without redirects. The URL must match the
client's configured origin. The audience defaults to `hai.ai` and is configured
on the client, never taken from per-request inputs or discovery.

CLI and MCP expose this as `HAI_REQUEST_AUTH_AUDIENCE`, which must match the
API's configured ingress audience. Only an absent variable uses `hai.ai`;
blank, invalid UTF-8, and values over 256 UTF-8 bytes fail closed at startup.
Valid values are passed unchanged, independently of `HAI_URL`. MCP captures the
value (or configuration error) once; subsequent process-environment changes and
tool arguments cannot replace it. Both ordinary clients and remote document
providers receive that audience, the latter through the existing
`build_document_provider_with_request_auth_audience` helper. Python, Node, Go,
and Rust retain their explicit client audience options.

Context-free header helpers now return an actionable error; there is no legacy
fallback. Public discovery and bootstrap registration remain unsigned. Updating
or rotating a registered identity authenticates the exact new registration
document with the pre-change identity. A successful local change with
`registered_with_hai: false` is not hosted readiness.

`fixtures/cross_lang_test.json` governs the shared wrapper input and exact-byte
FFI encoding contract, plus canonical JSON selection cases. Its historical auth
example is explicitly retired. It should not carry raw private keys or
JACS-owned signature vectors. Native transport tests verify proofs through JACS.
The same fixture declares CLI/MCP audience environment cases; loopback tests
verify actual ordinary and remote-record request proofs against that audience.

### Shared endpoint contract fixture

`fixtures/contract_endpoints.json` is the minimum shared endpoint contract.

Current required parity checks:

1. `hello`: `POST /api/v1/agents/hello` with auth
2. `submit_response`: `POST /api/v1/agents/jobs/{job_id}/response` with auth
3. `reply`: `POST /api/agents/{agent_id}/email/reply` with auth

Each language must have tests that assert method + path + auth behavior from this fixture.

### Shared MCP tool contract fixture

`fixtures/mcp_tool_contract.json` defines the minimum shared HAIAI MCP tool
surface. Languages may expose additional tools, but the required tool names and
input fields in that fixture must stay aligned.

### Path escaping

User-controlled path segments must be URL-escaped before interpolation.

Must-have escaping coverage:

1. `agent_id` in username/email/verify paths
2. `job_id` in submit response path
3. `message_id` in mark-read path
4. `jacs_id` + `version` in remote-key lookup path

### Verify-link constants

Keep these constants aligned:

1. `MAX_VERIFY_URL_LEN = 2048`
2. `MAX_VERIFY_DOCUMENT_BYTES = 1515`

Inline verify links must use base64url **without padding**.

### Email signature compatibility

Outbound email signing must use v2 payload format:

1. `sign_input = "{content_hash}:{from_email}:{timestamp}"`
2. `content_hash` is computed from subject/body (+ sorted attachment hashes)

Verification must remain backward compatible:

1. v2 verify: `{content_hash}:{from_email}:{timestamp}`
2. v1 verify: `{content_hash}:{timestamp}`

### Config discovery and key candidate order

Config discovery order:

1. explicit path argument
2. `JACS_CONFIG_PATH`
3. `./jacs.config.json`

Private-key candidate order:

1. explicit `jacsPrivateKeyPath` / equivalent
2. `agent_private_key.pem`
3. `{agentName}.private.pem`
4. `private_key.pem`

### Bootstrap registration behavior

`register_new_agent`-style flows must preserve:

1. request to `/api/v1/agents/register`
2. no `Authorization` header on bootstrap registration request
3. include owner email/domain/description if provided
4. private key written with secure permissions (POSIX: `0600`)
5. key directory permissions restrictive where applicable (POSIX: `0700`)

## Rust Implementation Layout

The Rust workspace lives under `rust/`:

1. `rust/haiai`: publishable library crate
2. `rust/haiai-cli`: CLI binary crate (`haiai` binary with built-in MCP server)
3. `rust/hai-mcp`: MCP server library crate

Rust-specific boundary points:

1. `rust/haiai/src/jacs.rs`: `JacsProvider` trait (integration seam to JACS)
2. `rust/hai-mcp/src/server.rs`: `HaiMcpServer` composition layer embedding `jacs-mcp`

Do not add runtime primitive crypto logic to `rust/haiai`; implement JACS-backed providers instead.

## Change Workflow

When HAIAI behavior changes:

1. update this guide first (if behavior contract changed)
2. update shared fixtures/schemas in repo root (`fixtures/`, `schemas/`)
3. update each language SDK implementation
4. add or update parity tests in each language
5. verify docs/examples for all language SDKs

## Minimum Parity Test Matrix

For each language SDK:

1. endpoint contract fixture tests (`fixtures/contract_endpoints.json`)
2. cross-language wrapper contract tests (`fixtures/cross_lang_test.json`)
3. MCP tool contract tests (`fixtures/mcp_tool_contract.json`) where applicable
4. path escaping regression tests
5. verify-link length/base64url tests
6. config and key resolution precedence tests
7. bootstrap registration security tests

## Open Integration Items

1. Keep `rust/haiai/src/jacs.rs` `JacsProvider` trait aligned with canonical `jacs` updates.
2. Keep `rust/hai-mcp` embedded `jacs_*` behavior aligned with canonical `jacs-mcp` tool changes.
3. Expand shared fixtures for additional HAI endpoints as contracts stabilize.
4. Missing JACS capabilities must fail clearly; do not implement local crypto to fill binding gaps.
5. Verify `jacs` JACS-side: `unwrap_signed_event` key type (`Vec<u8>` vs PEM `String`) and `get_lookup_id()` vs `get_id()` return format parity.
