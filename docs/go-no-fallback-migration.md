# Go No-Fallback Migration Scope

## Goal

Remove Go runtime fallback crypto from `haisdk` while preserving HAI contract
behavior and A2A parity.

`haisdk` should keep owning:

1. HAI request/response shaping
2. auth-header assembly and transport behavior
3. A2A wrapper contracts and parity fixtures

`jacs` should own:

1. key loading and key protection
2. signing and signature verification
3. algorithm selection and algorithm-specific details
4. canonicalization used for signatures where JACS already defines it

## Current Fallback Seams

These are the places that still let Go operate without a JACS-backed signer:

1. `go/crypto_fallback.go`
   Pure Go Ed25519 backend that implements `CryptoBackend`.
2. `go/signing.go`
   Compatibility helpers for parsing keys, signing, verifying, and generating
   keypairs. These are still used by runtime bootstrap paths.
3. `go/auth.go`
   `Client.buildAuthHeader()` still falls back to direct Ed25519 signing if the
   backend cannot sign.
4. `go/client.go`
   Client construction and bootstrap flows still create runtime state from raw
   PEM/private-key material and can end up on the fallback backend.
5. `go/a2a.go`
   A2A signing and verification go through `client.crypto`; if the client was
   built on fallback crypto, A2A silently does too.

## Constraints

These constraints should not change during migration:

1. `Authorization: JACS {jacsId}:{timestamp}:{signature_base64}` stays the auth format.
2. Bootstrap registration must still work before the agent is HAI-registered.
3. A2A parity stays in place; do not remove Go A2A features while removing crypto fallback.
4. Shared fixtures remain the arbiter for wrapper-level behavior:
   `fixtures/cross_lang_test.json`, `fixtures/a2a_verification_contract.json`,
   and `fixtures/mcp_tool_contract.json`.
5. Encrypted key handling remains mandatory for disk-backed keys.

## Recommended Staged Plan

### Stage 1: Make the JACS seam explicit

1. Document the exact Go call-site mapping in `docs/crypto-mapping.md`.
2. Require `CryptoBackend` for all runtime signing and verification paths.
3. Treat missing backend support as an initialization error, not a warning with
   silent fallback.

### Stage 2: Split bootstrap from runtime crypto

The main blocker is bootstrap.

Today, Go can be given raw PEM material and immediately sign requests. After
fallback removal, bootstrap needs a JACS-backed path instead:

1. Accept bootstrap credentials as import material only.
2. Write/import those credentials into a JACS-owned agent/config layout.
3. Re-open the client through the JACS-backed backend.

That keeps `haisdk` thin while still supporting register/create flows.

### Stage 3: Remove fallback from auth and response signing

After Stage 2:

1. Remove direct Ed25519 fallback from `go/auth.go`.
2. Route response signing only through the backend in `go/client.go`.
3. Fail fast when a client lacks signing capability.

### Stage 4: Keep A2A parity, swap signing source

Do not rewrite A2A behavior while doing this migration.

Keep:

1. Go A2A artifact wrapping/unwrapping
2. shared fixture coverage
3. wrapper-level contract parity

Change only the signing and verification source:

1. `SignBytes` must come from the JACS-backed backend
2. `VerifyBytes` must come from the JACS-backed backend
3. any algorithm-specific assumptions in A2A should be moved behind the backend

### Stage 5: Demote remaining local helpers

Once runtime code is clean:

1. mark raw key parsing/sign/verify helpers as compatibility-only
2. keep them only where tests or explicit migration tooling still require them
3. remove them from runtime paths in the next major release

## Exit Criteria

The migration is complete when:

1. `go/auth.go` no longer falls back to direct Ed25519 signing
2. `go/crypto_fallback.go` is unused by runtime code
3. `go/client.go` and `go/a2a.go` require a JACS-backed signer/verifier
4. Go still passes the shared wrapper and A2A parity fixtures
5. bootstrap flows work through JACS-owned key/config state rather than local
   runtime crypto
