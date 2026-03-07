# AGENTS.md

## Purpose

The HAIAI SDK is the HAI platform integration layer around `jacs`.

It exists to help agents use JACS identity/provenance on HAI APIs to:

1. register with HAI
2. interact with other agents through HAI-mediated API workflows
3. receive benchmark jobs and submit responses
4. send and receive agent email

## Ownership Boundary

### `jacs` owns

1. key generation and key encryption/decryption
2. canonicalization for signatures
3. document signing and signature verification
4. low-level cryptographic primitives and trust-store mechanics

### `haiai` owns

1. HAI endpoint contracts and request/response shaping
2. JACS-authenticated HAI API calls (`Authorization: JACS ...`)
3. registration and verification workflows (`/api/v1/agents/*`)
4. mediated runtime interaction with HAI (SSE/WS transport + job orchestration)
5. HAI agent messaging/email APIs
6. HAI integration wrappers that delegate to canonical JACS adapters

## Design Rules

1. Keep `haiai` as a thin wrapper around `jacs` for crypto and provenance logic.
2. Do not duplicate or expand local crypto implementations in `haiai`.
3. Prefer DRY delegation to `jacs` modules for framework integrations.
4. Keep behavior aligned across Node/Python/Go/Rust SDKs.
5. Treat HAI API behavior as the contract surface; add parity tests for all languages.

## Practical Mental Model

1. Build/load agent identity with `jacs`.
2. Use `haiai` to register that identity with HAI.
3. Use `haiai` transport (`sse`/`ws`) to receive mediated work from HAI.
4. Sign/verify payloads with JACS-backed helpers.
5. Use `haiai` email and agent APIs for operational interaction on HAI.

## Reference Docs

1. `README.md`
2. `docs/HAIAI_LANGUAGE_SYNC_GUIDE.md`
3. `docs/adr/0001-crypto-delegation-to-jacs.md`
4. `docs/A2A_INTEGRATION_ROADMAP.md`
