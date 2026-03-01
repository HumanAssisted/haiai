# JACS Email Signing & Verification Plan v2

## Status

- Draft plan for implementation.
- This document is normative for the attachment-only design.
- Legacy `X-JACS-Signature` header signing/verification is deprecated immediately and removed from active roadmap.
- Canonicalization model aligns with RFC 3156/RFC 9580 principles for robust in-transit email verification.

## Scope and Product Decisions (Resolved)

1. Single signing method: `jacs-signature.json` attachment only.
2. `sendEmail()` must use the same JACS signing/verification functions internally.
3. Forwarding keeps prior email content and re-signs before forward.
4. Primary user is AI-agent-to-AI-agent email where recipients understand JACS.
5. JACS signing is required for this flow (not optional).
6. Server-side and recipient-side verification use the same JACS functions.
7. Signed input is full raw RFC 5322 email bytes.
8. Signer key + algorithm are resolved from agent `id:version` and registry.
9. Parsing/canonicalization is shared in JACS and used by both client/server paths.
10. Max raw message size is 25 MB.
11. JACS crate release happens first or at same time as haisdk.

## Scope and Product Decisions (Pending)

1. Public human verification page (`hai.ai/verify`) is undecided.
2. Historical key lookup behavior for key rotation must be confirmed.
3. Python API argument order (`hai_url` position) to be finalized in SDK review.
4. Python `EmailMessage` convenience input is acceptable if test parity holds.

## Core Security Model

1. **Per-field hashes + one JACS document signature**
   - The email signature document stores per-field/per-part hashes.
   - The JACS document itself is signed once using the agent key.
   - No per-field asymmetric signatures are required.
   - Signed units include canonical MIME parts (body/attachments/inline) and optional outer-header claims.
   - MIME part hashes include selected MIME content headers plus canonicalized part content bytes.

2. **Trust order**
   - Verify the attached JACS document signature first.
   - Only after document trust is established, compare email content hashes.

3. **Field-level verification output**
   - Return per-field status: `pass`, `fail`, or `unverifiable`.
   - Include original signed value (where present) for forensics.

## Canonicalization (P0, Normative)

The signer and verifier MUST use identical canonicalization rules.

### 1. Parse model

- Parse strict RFC 5322 + MIME.
- Malformed/ambiguous message in strict mode => hard fail.

### 2. JACS JSON canonicalization

- Canonicalize JACS payload using RFC 8785-compatible rules.
- Rust implementation uses JACS canonicalization (`serde_json_canonicalizer`), not ad hoc key sorting.

### 3. Header canonicalization

Outer transport headers are not part of MIME body integrity in PGP/MIME-style systems.
In this design they are treated as optional signed claims, not required for base MIME integrity.

Verified claimed headers: `From`, `To`, `Cc`, `Subject`, `Date`.
Recorded (non-gating) header claim: `Message-ID`.
Optional claims: `In-Reply-To`, `References`.
`From` claim is mandatory for identity binding when verification policy enforces sender-email binding.
BCC remains excluded.

Normalize by:

- unfolding continuation lines
- compressing whitespace runs to one SP
- trimming surrounding whitespace
- decoding RFC 2047 encoded words
- UTF-8 NFC normalization

Rules:

- Duplicate claimed singleton headers => hard fail at sign time; `unverifiable` or `fail` at verify time per policy.
- Missing optional singleton headers => `unverifiable` for that field only.
- Outer-header claim mismatch does not invalidate MIME-part integrity unless policy requires strict header binding.
- For `From`/`To`/`Cc`, store both canonical claim value and strict claim hash in JACS payload.
- Store `Message-ID` value/hash in JACS payload for evidence, but do not use it as a verification gate.
- Address-claim verification uses two-stage comparison:
  1. strict hash comparison over canonical claim value
  2. if strict fails, semantic mailbox fallback comparison:
     - parse mailbox addresses
     - compare normalized addr-spec values case-insensitively
     - ignore display-name/comment differences
     - for multi-recipient fields (`To`/`Cc`), iterate each observed mailbox and require a match in the signed mailbox set (case-insensitive), then require no unmatched signed mailboxes remain
- Semantic fallback success is reported separately from strict success (`match_mode=semantic_fallback`), not silently treated as strict match.

### 4. Body canonicalization (`text/plain`, `text/html`)

- Decode `Content-Transfer-Encoding`.
- Decode bytes using declared charset (default `us-ascii` when absent per MIME defaults).
- Normalize Unicode text to NFC.
- Normalize line endings to canonical `\r\n` (CRLF).
- Re-encode canonical text bytes for hashing in the declared charset.
- Include canonicalized part content headers in the hashed unit: at minimum `Content-Type` (with charset) and `Content-Transfer-Encoding`.
- If declared charset cannot be decoded/encoded deterministically, fail with `CanonicalizationFailed`.

This follows industry email-signing practice for surviving cross-platform newline changes (`\n` vs `\r\n`) while preserving semantic content.

### 5. Attachment and inline-image canonicalization

- Decode transfer encoding to raw bytes.
- Preserve decoded bytes exactly (no newline rewriting/transcoding).
- Hash input includes canonicalized MIME content headers + raw decoded bytes:
  `content_type_lower + ":" + content_disposition_norm + ":" + content_id_norm + ":" + filename_utf8_nfc + ":" + raw_bytes`.
- Inline images are treated as attachments for hashing.
- Sort attachment hashes lexicographically.
- Exclude the active `jacs-signature.json` from attachment hash list.

### 6. Reconstruction and evidence expectations

- The primary guarantee is authenticity/tamper detection over canonicalized signed units.
- Exact SMTP wire-byte reconstruction from `jacs-signature.json` alone is not guaranteed.
- For dispute/evidence workflows, retain the signed `.eml` artifact and use `signed_value` vs `observed_value` plus hash results.

## Identity Binding (P1, Normative)

Verification invariants:

1. `metadata.issuer == registry.jacs_id`
2. `payload.headers.from.value == registry.email`
3. `signature.algorithm == resolved_key.algorithm` (or cryptographically inferred and equal)
4. If DNS policy requires DNS, DNS TXT must bind same agent ID and key hash.

## Replay Detection (P1)

- Max age: 24h
- Max future skew: 5m
- Replay key: `issuer + metadata.hash`
- Duplicate within TTL => `ReplayDetected`

## Forwarding Semantics

### MVP behavior

- Forwarder keeps previous content and signs the forwarded message as sent.
- Prior signed content remains part of forwarded content.
- The forwarder signs the fully composed forwarded email bytes (including quoted/attached prior content), not a reconstructed historical message.
- `parent_signature_hash` references the immediate prior signature document bytes.
- MVP requires single-parent validation; multi-hop recursive chain reporting is P2.

### Validation rules

1. Exactly one active `jacs-signature.json` at top level for the current message.
2. Parent is resolved by exact `parent_signature_hash` byte match (not filename alone).
3. Parent missing/ambiguous/mismatch => `ChainValidationFailed`.

## Core JACS Functions (Authoritative)

Implement in `jacs/src/email.rs` using byte I/O.

```rust
pub fn sign_email(raw_email: &[u8], signer: &dyn JacsProvider) -> Result<Vec<u8>>;

pub fn verify_email(
    raw_email: &[u8],
    resolver: &dyn PublicKeyResolver,
    policy: &VerificationPolicy,
) -> Result<EmailVerificationResultV2>;

pub fn extract_jacs_signature_attachment(raw_email: &[u8]) -> Result<Vec<u8>>;
pub fn remove_jacs_signature_attachment(raw_email: &[u8]) -> Result<Vec<u8>>;
```

### Signing flow (`sign_email`)

1. Parse + canonicalize raw email for edge cases.
2. Compute per-field/per-part hashes over canonical MIME units (part headers + canonical part bytes).
3. Build JACS email signature document containing:
   - canonical values (for forensics/comparison)
   - hashes for each signed unit
4. Compute `metadata.hash` over canonical JACS payload.
5. Sign JACS document once with signer key.
6. Return `.eml` bytes with `jacs-signature.json` attached.

### Verification flow (`verify_email`)

1. Extract `jacs-signature.json` attachment.
2. Verify JACS document signature + identity binding first.
3. Remove active JACS attachment from message bytes.
4. Canonicalize message content the same way as sign path.
5. Compare each content hash against JACS document fields.
6. For claimed headers, attempt strict hash match first, then semantic mailbox fallback for `From`/`To`/`Cc` when strict fails.
7. Evaluate `Message-ID` as an informational recorded-claim check only (never sole cause of invalid verdict).
8. Return field-by-field statuses (`pass|fail|unverifiable`) plus summary verdict, including MIME-part header mismatch reporting.

## Rust Implementation Constraints

1. Use byte APIs (`&[u8]` / `Vec<u8>`), not `&str`.
2. Do not use `mail-builder` for full email reserialization.
3. Use parser for read/locate, plus minimal raw-byte MIME insertion/removal.
4. Handle plain single-part and `multipart/alternative` upgrades to `multipart/mixed` when adding signature attachment.
5. Avoid duplicate signer traits; extend/adapt `JacsProvider`.

## API Contracts

```http
POST /api/v1/email/sign
Body: raw RFC 5322 bytes (or base64 envelope preserving bytes)
Resp: signed raw RFC 5322 bytes

POST /api/v1/email/verify
Body: raw RFC 5322 bytes
Resp: EmailVerificationResultV2
```

## Error Taxonomy (Required)

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
- `MimePartHeaderMismatch`
- `OuterHeaderClaimMismatch`
- `AddressClaimMismatch`

## Verification Result Shape (Required)

`EmailVerificationResultV2` must include at minimum:

- overall `valid`
- signer identity fields (`jacs_id`, algorithm, reputation/policy info)
- `field_results[]` with:
  - `field`
  - `status` (`pass|fail|unverifiable`)
  - `match_mode` (`strict|semantic_fallback|none`)
  - `expected_hash`
  - `actual_hash` (when computable)
  - `signed_value` (when available)
  - `observed_value` (when available)
- `errors[]`

## Migration Plan

1. Mark header signing/verification APIs deprecated immediately.
2. Stop new development on header flow.
3. Remove header flow from default SDK/server code paths.
4. Keep temporary compatibility shim only if needed for rollback window.

## Test Plan

Fixture baseline is `docs/email/fixtures/`.

Must-have categories:

1. Canonicalization determinism across Rust/Python/Node/Go
2. Identity binding mismatches
3. LF/CRLF equivalence for text parts under canonical CRLF hashing
4. Duplicate singleton outer-header claim handling
5. Charset/transfer-encoding equivalence
6. 25 MB limit boundary tests
7. Replay-window tests
8. Forward single-parent validation
9. Field-level result shape consistency across SDKs
10. MIME part content-header tamper detection (`Content-Type`, `Content-Transfer-Encoding`, `Content-Disposition`, `Content-ID`)
11. `From`/`To`/`Cc` strict-hash mismatch but semantic mailbox fallback pass (case-only/display-name-only differences)
12. `To`/`Cc` recipient reordering with identical mailbox set
13. `Cc` comparison where each observed mailbox is matched against signed set, including missing/unexpected recipient detection
14. `Message-ID` rewritten in transit: informational mismatch only, message remains valid if all gating checks pass

## Implementation Sequence

1. Finalize canonicalization rules as executable tests.
2. Implement byte-based JACS email core (`sign_email`, `verify_email`, attachment ops).
3. Add conformance tests against shared fixtures.
4. Integrate haisdk send/verify paths to JACS functions.
5. Add/adjust API endpoints with byte-preserving transport.
6. Implement unified error and result mapping across SDKs.
7. Remove header flow code paths.
8. Release JACS and haisdk together (or JACS first).
