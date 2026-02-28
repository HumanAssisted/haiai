# JACS Email Signing & Verification Plan v2

## Status

- Draft plan for implementation.
- This document is normative for the new attachment-only design.
- Legacy `X-JACS-Signature` header signing/verification is deprecated immediately and removed from active roadmap.

## Scope and Product Decisions (Resolved)

1. **Single method of truth:** `jacs-signature.json` attachment only. Header-based signing is removed.
2. **`sendEmail()` and `signEmail()` relationship:** `sendEmail()` must use the same JACS email signing functions internally. No parallel security model.
3. **Forwarding behavior:** forwarding wraps the previous signed email; parent signature linkage is retained.
4. **Primary user:** AI agents communicating with AI agents that understand JACS.
5. **Signing requirement:** JACS signing is required (not opt-in) for this flow.
6. **Verification parity:** server-side and recipient-side use the same JACS verification functions.
7. **Signed object:** full raw RFC 5322 email (including attachments/inline images) without semantic content mutation.
8. **Identity/key/algorithm binding:** use signer `agent_id:version` to resolve correct public key and algorithm.
9. **Input contract:** raw bytes are canonical input/output (`[u8]`), not UTF-8 strings.
10. **Parsing locations:** parsing occurs in client and server, but through shared JACS functions (no divergent signing logic).
11. **Attachment size limit:** 25 MB max message size for v2.
12. **Release cadence:** publish JACS email module first or simultaneously with haisdk changes.

## Scope and Product Decisions (Pending)

1. **Public human verification page (`hai.ai/verify`)**: undecided.
2. **Historical key lookup for rotation**: validate existing registry capability; if missing, add before GA.
3. **Python method arg order (`hai_url` first vs second)**: finalize during SDK API review.
4. **Python `EmailMessage` convenience input**: acceptable if tests pass and behavior remains deterministic.

## Security Priorities

## P0: Canonicalization determinism (must ship)

Without strict canonicalization, implementations can disagree or be bypassed.

### Normative canonicalization profile

1. **Parse model**
   - Parse strict RFC 5322 + MIME.
   - Malformed/ambiguous message in strict mode => fail.

2. **JACS JSON canonicalization**
   - Use RFC 8785-compatible canonicalization for JACS payload hashing/signing.
   - In Rust use JACS-owned canonicalization path (`serde_json_canonicalizer`) instead of ad hoc key sorting.

3. **Header canonicalization**
   - Required singleton headers: `From`, `To`, `Subject`, `Date`, `Message-ID`.
   - Optional singleton headers: `In-Reply-To`, `References`.
   - Normalize by:
     - unfolding continuation lines
     - compressing WSP runs to one SP
     - trimming leading/trailing WSP
     - decoding RFC 2047 encoded words
     - UTF-8 NFC normalization
   - Duplicate singleton headers => fail as ambiguous.

4. **Body canonicalization**
   - MIME decode transfer encoding.
   - Convert declared charset to UTF-8.
   - Normalize line endings to `\n`.
   - Hash decoded canonical bytes.

5. **Attachment canonicalization**
   - MIME decode transfer encoding to raw bytes.
   - Canonical hash input: `filename_utf8_nfc + ":" + content_type_lower + ":" + raw_bytes`.
   - Sort attachment hashes lexicographically.
   - Exclude active `jacs-signature.json` from attachment hash list.

6. **Unicode support**
   - Add normalization dependency support in core implementation.

## P1: Identity binding and policy hardening (must ship)

Verification invariants:

1. `metadata.issuer == resolved_registry.jacs_id`
2. `payload.headers.from.value == resolved_registry.email`
3. `signature.algorithm == resolved_key.algorithm` (or is cryptographically inferable and equal)
4. If DNS policy is required, DNS TXT must bind same agent ID and key hash.
5. Registry lookup uses signer identity path that supports versioned keys.

## P1: Replay controls (must ship)

1. Timestamp acceptance window (default: max age 24h, future skew 5m).
2. Replay cache key: `issuer + message_id + metadata.hash`.
3. Duplicate within TTL => fail or flagged replay based on strictness policy.

## P1: Forwarding chain integrity (MVP-lite + P2 expansion)

- MVP: single-parent validation and parent hash verification.
- P2: multi-hop chain traversal/reporting.

Rules:

1. Exactly one active `jacs-signature.json` in current envelope.
2. Parent chosen by exact `parent_signature_hash` byte match, not filename alone.
3. Parent mismatch or ambiguous matches => fail chain validation.

## Architecture v2

## Core JACS (authoritative implementation)

Implement in `jacs/src/email.rs`.

```rust
pub fn sign_email(raw_email: &[u8], signer: &dyn JacsProvider) -> Result<Vec<u8>>;
pub fn verify_email_document(
    raw_email: &[u8],
    resolver: &dyn PublicKeyResolver,
    policy: &VerificationPolicy,
) -> Result<(JacsEmailSignatureDocument, ParsedEmailParts)>;
pub fn verify_email_content(
    doc: &JacsEmailSignatureDocument,
    parts: &ParsedEmailParts,
    policy: &VerificationPolicy,
) -> Result<ContentVerificationResult>;
pub fn remove_jacs_signature_attachment(raw_email: &[u8]) -> Result<Vec<u8>>;
```

### Rust implementation constraints

1. Use byte APIs (`&[u8]`/`Vec<u8>`) for email I/O.
2. **Do not use `mail-builder` for full reserialization.**
3. Use parser for read/locate; perform minimal raw-byte MIME insertion to preserve payload bytes.
4. Handle plain-text-only and `multipart/alternative` upgrades safely to `multipart/mixed` when attaching signature.
5. Avoid introducing a duplicate signer trait; extend/adapt `JacsProvider`.

## haisdk integration

1. `sendEmail()` pipeline:
   - compose/gather raw RFC 5322 bytes
   - call JACS `sign_email`
   - send signed raw email or server-signed equivalent endpoint
2. `verifyEmail()` pipeline:
   - call JACS `verify_email_document`
   - call JACS `verify_email_content`
   - enrich with registry/DNS policy output for HAI-specific result

No separate legacy header verifier in active path.

## API contracts

## New/updated endpoints

```http
POST /api/v1/email/sign
Body: binary/raw RFC 5322 or base64 envelope (implementation choice, must preserve bytes)
Resp: signed raw RFC 5322 bytes

POST /api/v1/email/verify
Body: raw RFC 5322 bytes
Resp: EmailVerificationResultV2
```

### Payload transport note

Raw email must be treated as bytes. JSON string transport is allowed only if content is base64-encoded and decoded losslessly server-side.

## Limits

- Hard cap: 25 MB raw message size.
- Reject with explicit error code when exceeded.

## Error taxonomy (required)

Define cross-SDK mapped errors:

- `InvalidEmailFormat`
- `CanonicalizationFailed`
- `SignatureAttachmentMissing`
- `SignatureAttachmentInvalid`
- `SignerKeyNotFound`
- `SignerIdentityMismatch`
- `AlgorithmMismatch`
- `DnsBindingFailed`
- `ReplayDetected`
- `ChainValidationFailed`
- `MessageTooLarge`
- `UnsupportedFeature`

## Migration plan

1. Immediately mark header-based verification/signing APIs as deprecated.
2. Block new features on header flow.
3. Remove header flow from default code paths in all SDKs.
4. Remove header fixtures from active conformance suite.
5. Keep temporary compatibility shim only if needed for rollback window.

## Test plan

## Fixture source

Use `docs/email/fixtures/` as baseline fixture corpus for P0/P1 tests.

## Must-have test categories

1. Canonicalization determinism across Rust/Python/Node/Go
2. Identity-binding mismatch rejection
3. Duplicate singleton header rejection
4. Charset and transfer-encoding equivalence tests
5. Attachment hashing equivalence tests
6. 25 MB boundary tests
7. Replay-window tests
8. Forward single-parent validation (MVP)

## SDK consistency

All SDKs must consume the same expected fixture outputs and error codes.

## Open questions tracked

1. Public verification page: PM/product decision pending.
2. Historical key retrieval endpoint behavior for key rotation: confirm and document SLA.
3. Python function signatures for new methods: finalize style choice before beta.

## Implementation sequence

1. Finalize canonicalization/profile spec as testable rules.
2. Implement JACS core byte-based email module.
3. Add fixture conformance tests in JACS.
4. Integrate haisdk send/verify to JACS core functions.
5. Implement API endpoints with byte-preserving transport.
6. Add SDK wrappers + unified error mapping.
7. Deprecate/remove header flow code paths.
8. Ship JACS and haisdk together (or JACS first).
