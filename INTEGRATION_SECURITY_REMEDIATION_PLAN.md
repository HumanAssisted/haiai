# HAI SDK Integration + Security Remediation Plan

## Scope
This plan addresses:

1. Crypto policy and implementation consistency with JACS.
2. Security hardening and unsafe edge cases.
3. Cross-language integration completeness (Python, Node, Go).
4. Quickstart and documentation correctness.
5. Test/CI gaps that currently allow regressions.

## Non-Negotiable Policy
`haisdk` must not implement its own crypto primitives when JACS provides them.

Rules:

1. All signing, verification, key generation, canonicalization, and encryption must delegate to JACS functions.
2. Any local helper that currently performs cryptographic work must be removed or converted to a thin JACS wrapper.
3. Agent creation flows should support encrypted private key handling using JACS-provided encryption/key-management functionality.
4. New crypto code in `haisdk` is disallowed unless explicitly approved and documented as a JACS-gap exception.

## Policy Enforcement Changes

### P0.1 Add architecture policy docs
- Add `docs/adr/0001-crypto-delegation-to-jacs.md`.
- Add short `CRYPTO POLICY` section in root `README.md`.
- Add top-of-file comment in current crypto entry points:
  - `python/src/jacs/hai/crypt.py`
  - `node/src/crypt.ts`
  - `go/signing.go`

Required note text (or equivalent):
"This SDK must delegate cryptographic operations to JACS. Do not introduce local cryptographic implementations."

### P0.2 Add CI guardrails
- Add CI check that fails on direct crypto primitive usage outside approved adapter files.
- Initial deny patterns:
  - Python: `cryptography.hazmat.primitives.asymmetric.ed25519`
  - Node: `node:crypto` usage for `sign|verify|generateKeyPairSync` in SDK runtime code
  - Go: `crypto/ed25519` usage in SDK runtime code
- Keep tests exempt where needed.

Acceptance criteria:
- CI fails if a new direct crypto primitive call is introduced outside approved bridge modules.

## Workstream A: Migrate Crypto to JACS (All SDKs)

### A1. Capability discovery (P0)
- Inventory official JACS API surface for:
  - sign/verify
  - canonical JSON for signatures
  - key generation
  - key encryption/decryption
  - any JACS envelope helpers
- Produce mapping table: `current_haisdk_function -> jacs_function`.

Deliverable:
- `docs/crypto-mapping.md` with exact imports and replacement plan.

### A2. Python migration (P1)
- Replace usages of local crypto helpers with JACS calls in:
  - `python/src/jacs/hai/client.py`
  - `python/src/jacs/hai/signing.py`
  - `python/src/jacs/hai/async_client.py`
  - `python/src/jacs/hai/crypt.py` (convert to wrappers or deprecate/remove)
- Ensure `register_new_agent` can create encrypted private keys via JACS options.
- Preserve backward compatibility for existing plaintext keys with explicit compatibility mode.

Tests:
- Add/adjust tests to assert JACS adapter is called.
- Add encrypted key roundtrip tests.

### A3. Node migration (P1)
- Add/lock dependency on JACS Node package.
- Refactor:
  - `node/src/crypt.ts` to wrapper-only or remove.
  - `node/src/signing.ts` to use JACS canonicalization/sign/verify.
  - `node/src/client.ts` call sites to JACS-backed API.
- Update key generation in CLI/bootstrap to JACS key management, with encrypted private key default.

Tests:
- Add unit tests proving JACS adapter is used.
- Add encrypted key generation + load tests.

### A4. Go migration (P1)
- Add dependency on official Go JACS package (or approved bridge).
- Refactor:
  - `go/signing.go`
  - relevant methods in `go/client.go`
- Replace local key generation/sign/verify with JACS functions.
- Add encrypted key support in agent bootstrap + CLI path.

Tests:
- Adapter usage tests.
- encrypted private key load/use tests.

### A5. Deprecation cleanup (P2)
- Mark local crypto APIs deprecated for one release.
- Remove them in next major release.

## Workstream B: Integration and API Consistency

### B1. Endpoint consistency normalization (P0)
- Define single authoritative defaults and endpoints in one spec doc.
- Normalize:
  - base URL defaults (`hai.ai` vs `api.hai.ai` mismatch)
  - hello endpoint mismatch (`/api/v1/hello` vs `/api/v1/agents/hello`)
  - key-service endpoint behavior

Files likely impacted:
- `go/client.go`
- `node/src/client.ts`
- `python/src/jacs/hai/client.py`
- CLI defaults in each language.

### B2. Method parity matrix (P0)
- Create `docs/sdk-parity-matrix.md` listing required methods/features:
  - hello, register, verify/status
  - benchmark tiers
  - submit response
  - SSE + WS
  - username/email/key lookup
  - verify-link generation modes
- Fill actual support by language.
- Mark gaps with owners.

### B3. Close async/sync parity gaps in Python (P1)
- `AsyncHaiClient` currently lacks full sync parity.
- Implement missing high-value methods and document intentional exclusions.

## Workstream C: Security Hardening

### C1. Path-segment escaping (P0)
- Ensure all user-provided path segments are escaped in Node/Python (Go already has coverage in many paths).
- Targets:
  - agent IDs
  - message IDs
  - job IDs
  - username claim paths where needed

Tests:
- Add/expand path-escaping tests in Node/Python similar to Go coverage.

### C2. Signature verification behavior consistency (P1)
- Define expected behavior when verification keys are unavailable.
- Make behavior explicit and aligned across SDKs:
  - hello ACK verification
  - SSE/WS signed event verification
  - fallback key fetch strategy
- Ensure callers can opt into strict mode that fails on unverified signatures.

### C3. Key security defaults (P1)
- New key material should be encrypted-at-rest by default via JACS.
- For non-interactive environments, support:
  - env var passphrase
  - callback/provider hook
- Plaintext key output must require explicit opt-in flag and warning.

### C4. Fixture/schema hygiene (P2)
- Remove or repair stale fixture references:
  - `fixtures/cross_lang_test.json`
- Ensure schemas in `/schemas` are either authoritative and tested, or clearly marked as informational drafts.

## Workstream D: Quickstart + Docs Repair

### D1. Fix root README quickstarts (P0)
- Correct Python/Node/Go snippets to actual public APIs.
- Ensure each snippet is copy-paste runnable.
- Add explicit sync vs async examples for Python.

### D2. Fix language example bootstrapping (P0)
- Node/Go quickstarts currently assume config before creating new agent.
- Update new-agent flows to bootstrap without pre-existing config.

### D3. Add tested quickstart smoke checks (P1)
- CI smoke test that exercises quickstart scripts in dry/mock mode.
- At minimum validate:
  - script compiles
  - CLI args valid
  - no obviously incorrect API calls.

## Workstream E: Testing + CI Strategy

### E1. Contract tests against mock API (P0)
- Add shared mock API behavior tests for critical endpoints and auth patterns.
- Validate consistent request shapes across Python/Node/Go.

### E2. Cross-language compatibility tests (P1)
- Rebuild real cross-language test vectors.
- Assert same canonicalization/signature outputs where expected via JACS adapters.

### E3. Security regression suite (P1)
- Add cases for:
  - malformed IDs/path escaping
  - signature verification fallback behavior
  - encrypted key handling failures
  - strict verification mode behavior.

## Execution Order

1. P0 policy + CI guardrails (`P0.1`, `P0.2`).
2. P0 integration correctness (`B1`, `B2`, `C1`, `D1`, `D2`, `E1`).
3. P1 JACS crypto migration (`A2`, `A3`, `A4`, `C2`, `C3`).
4. P1 parity/tests/docs completion (`B3`, `D3`, `E2`, `E3`).
5. P2 cleanup and removals (`A5`, `C4`).

## Release Plan

### Release 0.1.x (stabilization)
- Endpoint normalization.
- Quickstart and docs corrected.
- path-escaping fixes.
- crypto policy docs + CI guardrails.

### Release 0.2.0 (crypto delegation)
- JACS-backed crypto adapters in all SDKs.
- encrypted-key defaults in agent bootstrap.
- deprecations for local crypto helpers.

### Release 1.0.0 (cleanup/strictness)
- Remove deprecated local crypto helpers.
- enable stricter verification defaults.
- finalized parity guarantees and docs.

## Concrete TODO Checklist

### P0 TODOs
- [x] Add ADR and README crypto policy section.
- [x] Add `CRYPTO POLICY` comments in crypto entry point files.
- [x] Add CI denylist for direct crypto primitive imports.
- [x] Normalize base URL + endpoint constants across Python/Node/Go.
- [x] Fix root README quickstart code.
- [x] Fix Node quickstart bootstrap flow.
- [x] Fix Go quickstart bootstrap flow.
- [x] Add path escaping for user-controlled path segments in Node.
- [x] Add path escaping for user-controlled path segments in Python.
- [x] Add/expand Node+Python tests for path escaping.
- [x] Add shared mock contract tests for auth + endpoint shape consistency.
- [x] Add canonical Python `haisdk` namespace wrappers while preserving `jacs.hai` compatibility.

### P1 TODOs
- [ ] Create `docs/crypto-mapping.md` with exact JACS replacements.
- [ ] Migrate Python crypto/signing to JACS-backed implementation.
- [ ] Migrate Node crypto/signing to JACS-backed implementation.
- [ ] Migrate Go crypto/signing to JACS-backed implementation.
- [ ] Add encrypted private key default flow for Python bootstrap.
- [ ] Add encrypted private key default flow for Node bootstrap/CLI.
- [ ] Add encrypted private key default flow for Go bootstrap/CLI.
- [ ] Define and implement consistent signature-verification modes.
- [ ] Close or document async parity gaps in Python.
- [ ] Add quickstart smoke tests in CI.
- [ ] Add cross-language compatibility tests using maintained fixtures.
- [ ] Add security regression suite (strict verification, malformed IDs, key handling).

### P2 TODOs
- [ ] Deprecate old local crypto helper exports with warnings.
- [ ] Remove stale fixture references or regenerate fixture assets.
- [ ] Clarify schema status or enforce schema validation in tests.
- [ ] Remove local crypto code in next major release.

## Definition of Done
This remediation is complete when:

1. All runtime crypto operations in Python/Node/Go are delegated to JACS.
2. Agent creation defaults to encrypted key material using JACS key features.
3. Quickstart docs and examples are accurate and CI-checked.
4. Endpoint defaults and behavior are consistent across SDKs.
5. Security regressions (path escaping, signature verification behavior) are covered by tests.
