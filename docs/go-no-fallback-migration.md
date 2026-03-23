# Go No-Fallback Migration Scope

## Status: Complete (2026-03-22)

The crypto fallback has been fully removed from the Go SDK. The JACS CGo
backend is the only active `CryptoBackend`. All stages below are complete.

- `go/crypto_fallback.go` -- deleted
- `go/sign_response_local.go` -- deleted
- Build tags removed from `crypto_jacs.go` -- JACS backend compiles unconditionally
- Deprecated raw-key helpers (`BuildAuthHeader`, `Sign`, `Verify`, `GenerateKeyPair`,
  `PublicKeyFromPrivate`) moved from runtime code to test-only `_test.go` files
- `VerifyHaiMessage` now delegates to `cryptoBackend.VerifyBytes`
- CI builds jacsgo shared library and runs Go tests with CGo enabled
- `check_no_local_crypto.sh` allowlist tightened (removed `crypto_fallback`,
  `sign_response_local`)
- All 286+ tests pass, `go vet` clean, crypto policy passes

### Remaining known limitation

`jacsBackend.GenerateKeyPair()` still uses local `ed25519.GenerateKey` because
jacsgo does not yet expose pq2025 key generation via FFI. This is documented
in `crypto_jacs.go` with a TODO.

---

## Goal

Remove Go runtime fallback crypto from `haiai` while preserving HAI contract
behavior and A2A parity.

`haiai` should keep owning:

1. HAI request/response shaping
2. auth-header assembly and transport behavior
3. A2A wrapper contracts and parity fixtures

`jacs` should own:

1. key loading and key protection
2. signing and signature verification
3. algorithm selection and algorithm-specific details
4. canonicalization used for signatures where JACS already defines it

## Former Fallback Seams (all resolved)

1. `go/crypto_fallback.go` -- DELETED
2. `go/signing.go` -- deprecated helpers moved to test-only files; key parsing retained
3. `go/auth.go` -- deprecated helpers moved to test-only files; `Client.buildAuthHeader()` delegates to CryptoBackend
4. `go/client.go` -- bootstrap uses `CryptoBackend.GenerateKeyPair()`
5. `go/a2a.go` -- signing and verification go through `CryptoBackend`

## Constraints

These constraints were preserved during migration:

1. `Authorization: JACS {jacsId}:{timestamp}:{signature_base64}` stays the auth format.
2. Bootstrap registration still works before the agent is HAI-registered.
3. A2A parity stays in place; Go A2A features were not removed.
4. Shared fixtures remain the arbiter for wrapper-level behavior.
5. Encrypted key handling remains mandatory for disk-backed keys.

## Staged Plan (all complete)

### Stage 1: Make the JACS seam explicit -- DONE

1. `CryptoBackend` required for all runtime signing and verification paths.
2. Missing backend is an initialization error, not a warning with silent fallback.

### Stage 2: Split bootstrap from runtime crypto -- DONE

1. Bootstrap uses `CryptoBackend.GenerateKeyPair()`.
2. Client construction goes through JACS-backed backend.

### Stage 3: Remove fallback from auth and response signing -- DONE

1. Direct Ed25519 fallback removed from `go/auth.go`.
2. Response signing goes only through the backend.
3. Client fails fast when crypto backend cannot sign.

### Stage 4: Keep A2A parity, swap signing source -- DONE

1. `SignBytes` comes from the JACS-backed backend.
2. `VerifyBytes` comes from the JACS-backed backend.

### Stage 5: Demote remaining local helpers -- DONE

1. Raw sign/verify/generate helpers moved to `_test.go` files.
2. Runtime code only uses `CryptoBackend` interface.

## Exit Criteria (all met)

1. `go/auth.go` no longer falls back to direct Ed25519 signing -- DONE
2. `go/crypto_fallback.go` deleted -- DONE
3. `go/client.go` and `go/a2a.go` require a JACS-backed signer/verifier -- DONE
4. Go still passes the shared wrapper and A2A parity fixtures -- DONE
5. Bootstrap flows use CryptoBackend -- DONE
