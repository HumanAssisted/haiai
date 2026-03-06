# HAISDK Language Sync Guide

## Purpose

This document is the source of truth for **HAISDK-specific** behavior that must stay aligned across language SDKs (`python`, `node`, `go`, `rust`).

Use this guide whenever HAISDK behavior changes so each language implementation can be updated consistently.

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

### What belongs in `haisdk`

`haisdk` owns HAI-platform integration behavior:

1. HAI endpoint contracts and request/response shaping
2. JACS auth header usage (`Authorization: JACS ...`) and when auth is required
3. transport behavior for HAI event APIs (SSE/WS)
4. benchmark orchestration (free/dns_certified/certified tier flows)
5. username, email, key-discovery, and HAI verification endpoints
6. verify-link generation rules for `hai.ai` verifier URLs

## Cross-Language Invariants

These must match in all SDKs.

### Canonical dependency pins

When updating Rust integrations, use these canonical upstream repos as references:

1. `~/personal/JACS/jacs`
2. `~/personal/JACS/jacs-mcp`

Target canonical version pin for both integrations: `0.8.0`.

### Authentication header format

Header format is:

`JACS {jacsId}:{timestamp}:{signature_base64}`

Signed message is:

`{jacsId}:{timestamp}`

`fixtures/cross_lang_test.json` is the shared wrapper-level fixture for this
shape plus canonical JSON selection cases. It should not carry raw private keys
or JACS-owned signature vectors.

### Shared endpoint contract fixture

`fixtures/contract_endpoints.json` is the minimum shared endpoint contract.

Current required parity checks:

1. `hello`: `POST /api/v1/agents/hello` with auth
2. `check_username`: `GET /api/v1/agents/username/check` without auth
3. `submit_response`: `POST /api/v1/agents/jobs/{job_id}/response` with auth

Each language must have tests that assert method + path + auth behavior from this fixture.

### Shared MCP tool contract fixture

`fixtures/mcp_tool_contract.json` defines the minimum shared HAISDK MCP tool
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

1. `rust/haisdk`: publishable library crate
2. `rust/hai-mcp`: MCP server binary crate

Rust-specific boundary points:

1. `rust/haisdk/src/jacs.rs`: `JacsProvider` trait (integration seam to JACS)
2. `rust/hai-mcp/src/server.rs`: `HaiMcpServer` composition layer embedding `jacs-mcp`

Do not add runtime primitive crypto logic to `rust/haisdk`; implement JACS-backed providers instead.

## Change Workflow

When HAISDK behavior changes:

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

1. Keep `rust/haisdk/src/jacs_local.rs` aligned with canonical `jacs` updates.
2. Keep `rust/hai-mcp` embedded `jacs_*` behavior aligned with canonical `jacs-mcp` tool changes.
3. Expand shared fixtures for additional HAI endpoints as contracts stabilize.
