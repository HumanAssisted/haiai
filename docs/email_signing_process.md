# JACS Email Signature: Signing and Verification Process

## Target Users

AI agents that understand JACS. The JACS email signing system is designed for
agent-to-agent communication where both sender and recipient use JACS for
identity and trust. The HAI server uses the same JACS verification functions as
the recipient — there is no separate "server-side" vs "client-side" verification
path.

## Overview

JACS email signatures use a **detached signature** model. A standard JACS
document is attached to the email as `jacs-signature.json`. The document
contains hashes and raw values of the email headers, body, and other
attachments.

The JACS document is signed by definition — JACS handles cryptographic
validation, identity resolution, and trust. The email-specific code only
needs to:

1. **Extract** the `jacs-signature.json` attachment (which JACS validates
   as part of its standard document verification).
2. **Compare** the hashes inside the now-trusted JACS document against
   the actual email content.

Multi-algorithm support, DNS verification, and registry lookup are handled
entirely by the JACS layer. The email module computes hashes and compares
them — it never touches cryptography.

The JACS attachment is the **only** signing method. The legacy `X-JACS-Signature`
header (v1/v2) is deprecated and must not be used in new implementations. See
the **Deprecation** section for migration details.

---

## Trust Chain

```
┌─────────────────────────────────────────────────────┐
│  1. JACS Document Validation                        │
│     "Was this document authentically signed by       │
│      the claimed agent?"                            │
│                                                     │
│     a. Parse jacs-signature.json as JacsDocument    │
│     b. Verify document hash (SHA-256 of canonical   │
│        payload via RFC 8785 / JCS)                  │
│     c. Resolve the agent's public key and algorithm │
│        using the agent ID version from              │
│        metadata.issuer                              │
│     d. Fetch public key from HAI registry           │
│        GET /api/agents/keys/{from_email}            │
│        → returns public_key, algorithm,             │
│          reputation_tier                            │
│     e. Verify cryptographic signature using the     │
│        algorithm determined by the agent ID version │
│        (ed25519, rsa-pss, or pq2025)               │
│     f. If reputation_tier is dns_certified or       │
│        fully_certified: verify public key hash      │
│        against DNS TXT record at                    │
│        _v1.agent.jacs.{domain}                     │
│                                                     │
│     Result: The JACS document is authentic.         │
│     Its payload can be trusted.                     │
└─────────────────┬───────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────┐
│  2. Email Content Validation                        │
│     "Does the email match what the agent signed?"   │
│                                                     │
│     a. Extract hashes AND raw values from the       │
│        trusted JACS document payload                │
│     b. Recompute hashes from the actual email       │
│     c. Compare each hash                            │
│     d. If mismatch: report which fields were        │
│        tampered, show original vs current values    │
│                                                     │
│     Result: The email content is intact and          │
│     matches what the agent signed.                  │
└─────────────────────────────────────────────────────┘
```

---

## Algorithm Resolution

The signing algorithm is **not** declared in the email or assumed by the
verifier. It is determined by the **agent ID version** stored in
`metadata.issuer`. The JACS agent ID encodes which key type and algorithm the
agent uses. The verification flow resolves the public key and algorithm from
the agent ID version, either via local JACS lookup or HAI registry.

This means:
- No algorithm field needs to be trusted from the email itself
- Multi-algorithm support (Ed25519, RSA-PSS, PQ2025) is automatic
- Key rotation is handled by the agent ID versioning system

---

## Email Structure

```
Email:
├── Headers
│   ├── From: agent@example.com
│   ├── To: recipient@example.com
│   ├── Subject: Monthly Report
│   ├── Date: Thu, 27 Feb 2026 12:00:00 +0000
│   ├── Message-ID: <abc123@example.com>
│   ├── In-Reply-To: <prev456@example.com>          (if replying)
│   └── References: <orig789@example.com>            (if in thread)
├── multipart/mixed
│   ├── multipart/alternative
│   │   ├── text/plain                               (plain text body)
│   │   └── text/html                                (HTML body)
│   ├── report.pdf                                   (user attachment)
│   ├── data.csv                                     (user attachment)
│   ├── image001.png                                 (inline image)
│   └── jacs-signature.json                          (DETACHED JACS SIGNATURE)
```

The entire email is signed — headers, body (all MIME parts), all attachments
including inline/embedded images. Content must be properly encoded without
modification. The JACS signature covers exactly what the sender composed.

---

## JACS Signature Document Format

The `jacs-signature.json` attachment is a standard JACS document. The payload
includes both **hashes** (for verification) and **raw values** (so a verifier
can display what was originally signed even if the email was tampered with).

```json
{
  "version": "1.0",
  "document_type": "email_signature",
  "payload": {
    "headers": {
      "from": {
        "value": "agent@example.com",
        "hash": "sha256:<hex>"
      },
      "to": {
        "value": "recipient@example.com",
        "hash": "sha256:<hex>"
      },
      "subject": {
        "value": "Monthly Report",
        "hash": "sha256:<hex>"
      },
      "date": {
        "value": "Thu, 27 Feb 2026 12:00:00 +0000",
        "hash": "sha256:<hex>"
      },
      "message_id": {
        "value": "<abc123@example.com>",
        "hash": "sha256:<hex>"
      },
      "in_reply_to": {
        "value": "<prev456@example.com>",
        "hash": "sha256:<hex>"
      },
      "references": {
        "value": "<orig789@example.com>",
        "hash": "sha256:<hex>"
      }
    },
    "body_plain": {
      "content_hash": "sha256:<hex>",
      "mime_headers_hash": "sha256:<hex>"
    },
    "body_html": {
      "content_hash": "sha256:<hex>",
      "mime_headers_hash": "sha256:<hex>"
    },
    "attachments": [
      {
        "content_hash": "sha256:<hex>",
        "mime_headers_hash": "sha256:<hex>",
        "filename": "report.pdf"
      }
    ],
    "parent_signature_hash": null
  },
  "metadata": {
    "issuer": "<agent-jacs-id-with-version>",
    "document_id": "<uuid>",
    "created_at": "<ISO 8601>",
    "hash": "sha256:<hex of RFC 8785 canonical payload>"
  },
  "signature": {
    "key_id": "<agent-key-id>",
    "algorithm": "<resolved-from-agent-id-version>",
    "signature": "<base64>",
    "signed_at": "<ISO 8601>"
  }
}
```

### Signed Headers

The following headers are included in the JACS document payload:

| Header | Key in payload | Always present | Notes |
|--------|---------------|----------------|-------|
| From | `headers.from` | Yes | Domain lowercased before hashing |
| To | `headers.to` | Yes | Domain lowercased before hashing |
| Subject | `headers.subject` | Yes | |
| Date | `headers.date` | Yes | Helps detect replay attacks |
| Message-ID | `headers.message_id` | Yes | Unique message identifier |
| In-Reply-To | `headers.in_reply_to` | No | Present only if replying |
| References | `headers.references` | No | Present only if in a thread |

BCC is intentionally excluded — it is stripped before delivery by mail servers.
CC may be added in a future version.

Each header entry contains:
- `value`: the raw header value as the sender saw it (for display/forensics)
- `hash`: `sha256(<header value>)` (for verification)

For From and To, the **domain part** of each email address (after `@`) is
lowercased before hashing. The local part (before `@`) is preserved as-is
per RFC 5321 (local-part is technically case-sensitive).

### Body Parts

Both text/plain and text/html parts are stored with a content hash and a
MIME headers hash:

```
body_plain = {
  content_hash: sha256(canonicalized text/plain bytes),   # null if absent
  mime_headers_hash: sha256(canonical MIME part headers)   # null if absent
}
body_html = {
  content_hash: sha256(canonicalized text/html bytes),    # null if absent
  mime_headers_hash: sha256(canonical MIME part headers)   # null if absent
}
```

The `mime_headers_hash` covers `Content-Type`, `Content-Transfer-Encoding`,
and `Content-Disposition` of the MIME part (see Canonicalization Profile §5).
This prevents attacks that alter content type or encoding while preserving
the raw body bytes.

The verifier checks whichever part(s) are present. If a mail provider strips
one format (e.g., drops text/plain), the other hash still allows partial
verification. The verifier should report which body format(s) could be
verified.

### Attachments

Each non-JACS attachment is stored with a content hash, MIME headers hash,
and filename for identification. Inline/embedded images are treated as
attachments. The list is sorted lexicographically by `content_hash` for
determinism:

```
attachments = sort_by_content_hash([
  {
    content_hash: sha256(filename + ":" + content_type + ":" + raw_bytes),
    mime_headers_hash: sha256(canonical MIME part headers),
    filename: "report.pdf"
  },
  ...
])
```

The `jacs-signature.json` attachment itself is excluded from the list.
Attachments must be properly MIME-decoded before hashing (see
**Canonicalization Profile**). Maximum email size including all attachments
is **25 MB**.

### Parent Signature Hash (Forwarding Chain)

When an email is forwarded by a JACS-signing agent, the forwarder's JACS
document includes `parent_signature_hash` — the SHA-256 hash of the previous
`jacs-signature.json`. See the **Forwarding Chain** section below.

---

## Canonicalization Profile (Normative)

All implementations MUST follow this canonicalization profile exactly.
Without strict rules, independent implementations will compute different
hashes for the same semantic email. This profile is enforced on both sign
and verify paths.

### 1. Message Parsing

- Parse as RFC 5322 + MIME.
- If message is malformed/ambiguous, fail with `InvalidEmailFormat` error
  (no best-effort mode).

### 2. JACS JSON Canonicalization

- Canonicalize the JACS payload with **RFC 8785 (JCS)** before
  `metadata.hash` and signature generation/verification.
- Use JACS library's `serde_json_canonicalizer` (Rust) or equivalent.
  Do NOT use simple key-sorting — RFC 8785 also handles number
  serialization and Unicode escape normalization.

### 3. Header Canonicalization (for signed headers)

- Use DKIM-style relaxed normalization:
  - lowercase header name
  - unfold continuation lines
  - compress WSP runs to one SP
  - trim leading/trailing WSP
- For required singleton headers (`From`, `To`, `Subject`, `Date`,
  `Message-ID`): if duplicates exist, fail as ambiguous.
- For optional headers (`In-Reply-To`, `References`): if absent, field is
  `null`; if duplicated unexpectedly, fail as ambiguous.
- Decode RFC 2047 encoded words before hashing, then UTF-8 NFC normalize.

### 4. Body-Part Canonicalization (`text/plain`, `text/html`)

- MIME decode `Content-Transfer-Encoding`.
- Convert charset to UTF-8 (if declared/parseable; else fail).
- Normalize line endings to `\r\n` (CRLF — the RFC 5322 canonical form).
- Strip trailing whitespace (SP, TAB) from each line.
- Strip trailing blank lines at the end of the body part.
- Hash resulting bytes exactly.

### 5. MIME Part Header Hashing

Each body part and attachment has MIME structural headers that affect
interpretation. These MUST be hashed alongside the content to prevent
attacks that alter content type or encoding metadata while preserving raw
bytes. For each MIME part, hash the following headers (if present):

- `Content-Type` (including parameters like `charset`, `boundary`)
- `Content-Transfer-Encoding`
- `Content-Disposition` (including `filename` parameter)

Canonicalize each MIME part header the same way as top-level headers
(lowercase name, unfold, compress WSP, trim). The hash is stored per
body part / attachment entry in the JACS document as `mime_headers_hash`:

```
mime_headers_hash = sha256(
  "content-type:" + canonical_content_type + "\n" +
  "content-transfer-encoding:" + canonical_cte + "\n" +
  "content-disposition:" + canonical_disposition + "\n"
)
```

Omit lines for headers not present on the part. Sort remaining lines
lexicographically.

### 6. Attachment Canonicalization

- MIME decode `Content-Transfer-Encoding` to raw attachment bytes.
- Do not transcode payload bytes after decode.
- Canonical hash input:
  `sha256(filename_utf8_nfc + ":" + content_type_lower + ":" + raw_bytes)`.
- Exclude only the active `jacs-signature.json` attachment from the
  `attachments` list.
- Inline/embedded images (Content-Disposition: inline) are treated as
  attachments for hashing purposes.

### 6. Determinism Tests

- Shared cross-language fixtures MUST include tricky cases:
  folded headers, RFC 2047 subjects, mixed charsets, quoted-printable vs
  base64 bodies, duplicate headers, Unicode normalization edge cases,
  trailing whitespace on body lines, trailing blank lines at end of body,
  mixed line endings (LF, CRLF, CR), MIME Content-Type parameter variations
  (charset casing, boundary quoting), and email addresses with mixed-case
  local parts and domains.

---

## Signing Flow (Sender)

```
1. Accept the raw RFC 5322 email as bytes

2. Parse and canonicalize:
   a. Parse the raw email using mail-parser (read-only)
   b. Apply the canonicalization profile to extract clean values

3. Compute hashes:
   a. For each header (From, To, Subject, Date, Message-ID,
      In-Reply-To, References):
      - Store the canonicalized value
      - Compute sha256(value) — lowercase From/To domain only before hashing
   b. For each body part (text/plain, text/html):
      - Hash canonicalized body content → content_hash
      - Hash canonical MIME part headers → mime_headers_hash
   c. For each attachment (including inline images):
      - Hash sha256(filename:content_type:data) → content_hash
      - Hash canonical MIME part headers → mime_headers_hash
      - Sort attachments by content_hash
   e. Set parent_signature_hash = null (unless forwarding, see below)

4. Build the JACS document payload with headers (value + hash),
   body hashes, and attachment hashes

5. Sign the JACS document using the agent's private key:
   - Canonicalize the payload via RFC 8785 (JCS)
   - Compute metadata.hash = sha256(canonical_payload)
   - Resolve algorithm from the agent's ID version
   - Sign using the agent's key

6. Attach jacs-signature.json to the email via raw byte injection:
   - Find the outermost MIME boundary
   - Insert new MIME part before closing boundary
   - If email is not multipart/mixed, wrap it first
   Content-Type: application/json; name="jacs-signature.json"
   Content-Disposition: attachment; filename="jacs-signature.json"
```

### MIME Attachment Injection

The `sign_email` function works at the raw byte level to preserve the
original email content exactly:

1. **Already `multipart/mixed` at top level**: Find the closing boundary
   (`--{boundary}--`) and insert the JACS attachment part before it.
2. **`multipart/alternative` at top level** (text + html, no attachments):
   Wrap in a new `multipart/mixed` envelope.
3. **Single-part** (`text/plain` or `text/html` only): Wrap in a new
   `multipart/mixed` envelope.

For cases 2 and 3, the original content becomes the first part of the new
`multipart/mixed`, and the JACS attachment is added as the second part.
The key invariant: **hash body parts AFTER parsing but BEFORE
reconstruction.** The verifier also parses and then hashes. As long as both
sides use the same canonicalization profile, reconstruction does not break
verification.

Do NOT use `mail-builder` or any library that re-serializes the full email.
Re-serialization alters boundary strings, header ordering, whitespace, and
Content-Transfer-Encoding, which breaks hash verification.

---

## Verification Flow (Receiver)

```
1. Accept the raw RFC 5322 email as bytes

2. Find the jacs-signature.json attachment in the email
   - If not found, fail — JACS attachment is required
   - If multiple jacs-signature.json files exist, this is a forwarding
     chain — see below

3. Remove the jacs-signature.json attachment for content verification
   (the signature covers the email WITHOUT itself)

4. Validate the JACS document (standard JACS verification):
   a. Parse the JSON as a JacsDocument
   b. Verify the document hash:
      - Canonicalize payload via RFC 8785 → SHA-256 → compare to
        metadata.hash
   c. Identify the signer:
      - Extract metadata.issuer (the agent's JACS ID with version)
   d. Resolve the public key and algorithm from the agent ID version
   e. Fetch the public key from HAI registry:
      - GET /api/agents/keys/{payload.headers.from.value}
      - Response: public_key, algorithm, reputation_tier
   f. Verify identity binding:
      - metadata.issuer must match registry.jacs_id
      - payload.headers.from.value must match registry email
   g. Verify the cryptographic signature:
      - Use the algorithm resolved from the agent ID version
   h. If reputation_tier is "dns_certified" or "fully_certified":
      - Extract domain from the From email
      - Query DNS TXT at _v1.agent.jacs.{domain}
      - Compute sha256(public_key_pem_bytes), base64 encode
      - Compare against jacs_public_key_hash= in the TXT record
      - Mismatch → FAIL

   The JACS document is now trusted. Its payload can be used.

5. Parse and canonicalize the email content (same profile as signing)

6. Validate email contents against the trusted payload:
   a. For each header in payload.headers:
      - Recompute sha256(canonicalized_header_value)
      - Compare to payload.headers.{field}.hash
      - On mismatch: report tampered field, show original value
        from payload.headers.{field}.value vs current value
   b. Recompute body part hashes:
      - sha256(text/plain content) → compare to payload.body_plain.content_hash
      - sha256(text/html content)  → compare to payload.body_html.content_hash
      - sha256(MIME part headers)  → compare to *.mime_headers_hash
      - If one part is missing (provider stripped it), report as
        "unverifiable" rather than "tampered"
   c. Recompute attachment hashes:
      - For each non-JACS attachment: sha256(filename:content_type:data)
      - sha256(MIME part headers) for each attachment
      - Sort by content_hash, compare to payload.attachments
   d. Check parent_signature_hash for forwarding chain (see below)

7. Return verification result:
   - valid: true/false
   - jacs_id: the signer's agent ID
   - algorithm: the algorithm used
   - reputation_tier: from the registry
   - dns_verified: true/false/null
   - tampered_fields: list of fields that don't match
   - original_values: map of field → original value (from payload)
   - chain: list of signers if forwarding chain exists
```

---

## Forwarding Chain

When a JACS-signing agent forwards an email, it wraps the original email in a
new one and creates a **new** JACS signature document. The new signature
references the previous one via `parent_signature_hash`. This creates a
verifiable chain of custody: original signer → forwarder → recipient.

### How it works

```
Original email from Agent A:
├── body: "Hello, here's the report"
├── report.pdf
└── jacs-signature.json              ← signed by Agent A
    {
      payload: {
        headers: { from: { value: "agentA@x.com", ... }, ... },
        body_plain: { content_hash: "sha256:aaa...", mime_headers_hash: "sha256:..." },
        attachments: [{ content_hash: "sha256:bbb...", mime_headers_hash: "sha256:...", filename: "report.pdf" }],
        parent_signature_hash: null      ← no parent (original)
      },
      signature: { ... by Agent A }
    }

Agent B forwards to Agent C:
├── body: "FYI, see below\n---\nHello, here's the report"
├── report.pdf
├── jacs-signature-0.json            ← Agent A's original (renamed)
└── jacs-signature.json              ← signed by Agent B (NEW)
    {
      payload: {
        headers: { from: { value: "agentB@y.com", ... }, ... },
        body_plain: { content_hash: "sha256:ccc...", mime_headers_hash: "sha256:..." },
        attachments: [{ content_hash: "sha256:bbb...", mime_headers_hash: "sha256:...", filename: "report.pdf" }],
        parent_signature_hash: "sha256:ddd..."  ← hash of Agent A's doc
      },
      signature: { ... by Agent B }
    }
```

### Forwarding signing flow

```
1. Agent B receives the email with jacs-signature.json from Agent A

2. Before composing the forward:
   a. Compute sha256(raw bytes of Agent A's jacs-signature.json)
      → this becomes parent_signature_hash
   b. Rename Agent A's attachment to jacs-signature-0.json
      (or jacs-signature-{N}.json for deeper chains)

3. Wrap the original email in a new forwarded email (new headers, new body)

4. Build Agent B's JACS document:
   - Hash the NEW headers (Agent B's From, new To, etc.)
   - Hash the NEW body (may include forwarded text)
   - Hash all attachments (including the renamed jacs-signature-0.json)
   - Set parent_signature_hash = sha256 of Agent A's original document

5. Sign and attach as jacs-signature.json
```

### Forwarding verification flow

```
1. Find jacs-signature.json (the most recent signer, Agent B)

2. Validate Agent B's JACS document (standard JACS verification)

3. Validate email contents against Agent B's payload

4. If parent_signature_hash is not null:
   a. Find candidate parent documents among attachments
   b. Compute sha256(raw bytes) of each candidate
   c. Match by parent_signature_hash value (NOT by filename alone)
   d. If no match or multiple matches → fail chain validation
   e. Recursively validate the parent JACS document
   f. Continue until parent_signature_hash is null (the original)

5. Return the full chain:
   [
     { signer: "agentA@x.com", jacs_id: "...", valid: true },
     { signer: "agentB@y.com", jacs_id: "...", valid: true, forwarded: true }
   ]
```

### Chain properties

- Each link in the chain is independently verifiable
- The forwarder cannot tamper with the original signature (it's hashed)
- The chain proves: "Agent A signed the original, Agent B forwarded it"
- A broken chain (parent hash mismatch) indicates the original attachment
  was modified
- The original signer's raw header values are preserved in their JACS
  document, so even if headers changed during forwarding, the verifier
  can recover the original values
- Parent resolution is by `parent_signature_hash` matching raw bytes,
  not by filename alone
- Exactly one active `jacs-signature.json` (latest signer) is required

---

## JACS Attachment Operations

The JACS email module provides three core operations on raw RFC 5322 emails:

| Operation | Purpose |
|-----------|---------|
| `add_jacs_attachment(email, doc)` | Attach `jacs-signature.json` to raw email via byte-level MIME injection |
| `get_jacs_attachment(email)` | Extract `jacs-signature.json` from raw email without modifying it |
| `remove_jacs_attachment(email)` | Remove `jacs-signature.json` for content hash verification |

These operations work on raw bytes (`&[u8]` in Rust) and preserve the
original email content exactly. They are used by both the JACS library
and the HAI API.

---

## Error Taxonomy

All implementations must use consistent error types:

| Error | When |
|-------|------|
| `InvalidEmailFormat` | Raw email is not valid RFC 5322 / MIME |
| `CanonicalizationFailed` | Email is ambiguous or unparseable per strict canonicalization profile |
| `MissingJacsSignature` | No `jacs-signature.json` attachment found |
| `InvalidJacsDocument` | JACS document fails hash or structural validation |
| `SignatureVerificationFailed` | Cryptographic signature does not verify |
| `SignerKeyNotFound` | Registry lookup for agent's public key failed |
| `IdentityMismatch` | metadata.issuer or from header doesn't match registry |
| `DNSVerificationFailed` | DNS TXT record check failed |
| `ContentTampered` | One or more email content hashes don't match payload |
| `ChainVerificationFailed` | Parent signature hash mismatch in forwarding chain |
| `AlgorithmMismatch` | Signature algorithm does not match algorithm resolved from agent ID version |
| `ReplayDetected` | Duplicate message within replay detection TTL |
| `EmailTooLarge` | Email exceeds 25 MB limit |
| `UnsupportedFeature` | Requested capability is not implemented in this version |

---

## Identity Binding (Normative)

Verification MUST enforce these invariants:

1. `metadata.issuer` must match `registry.jacs_id` for the claimed agent
2. `payload.headers.from.value` must match the registered email for
   `registry.jacs_id`
3. `signature.algorithm` must match the algorithm resolved from the
   agent's public key (or be cryptographically inferable and equal)
4. If DNS policy requires verification, the DNS TXT binding must match
   the same agent ID and key hash — do not rely on `reputation_tier` alone
5. Registry lookup must use the signer's versioned identity path to
   support key rotation and multi-version key resolution

---

## Replay Detection

- Max accepted age: 24 hours
- Max future skew: 5 minutes
- Replay cache key: `issuer + message_id + metadata.hash`
- On duplicate cache hit within TTL: fail with `ReplayDetected`

---

## Why This Design

### Multi-algorithm support comes for free

The agent ID version determines the algorithm. The standard JACS verification
path already handles Ed25519, RSA-PSS, and PQ2025. Email verification code
does not need its own algorithm dispatch.

### Survives email forwarding

Attachments are preserved by all major email providers. The
`jacs-signature.json` attachment survives forwarding, making verification
possible even after the email has been relayed.

### Chain of custody for forwarding

The `parent_signature_hash` field creates a tamper-evident chain. Each
forwarder wraps the original email and signs a new JACS document that
references the previous one. The full chain can be verified back to the
original sender.

### Clean separation of concerns

- **JACS layer**: "Is this document authentically from agent X?" (crypto,
  identity, DNS)
- **Email layer**: "Does this email match what agent X signed?" (hash
  comparison)

The email layer never touches cryptography. The JACS layer never thinks about
email structure. Each does one thing well.

### Rich forensics on tampering

Because both hashes and raw values are stored, a verifier can report:
"The From header was changed from `agent@x.com` to `attacker@y.com`" rather
than just "verification failed." This makes the system useful for
investigation, not just pass/fail.

### Identity verification is robust

The verification chain is multi-factor:

1. **Cryptographic**: The signature is valid for the claimed public key
2. **Registry**: The public key is registered with HAI for this agent
3. **DNS** (when applicable): The public key hash matches an independent DNS
   record controlled by the agent's domain

An attacker cannot forge a signature without the private key. A compromised
registry is detected by the DNS check. A compromised DNS record is detected by
the registry check. Both must agree.

---

## Relationship to PGP/MIME and DKIM

JACS email signatures are **complementary** to DKIM/ARC, not a replacement.
DKIM validates domain-level sending authority. JACS validates agent-level
identity and content integrity. Both can coexist on the same email.

| Property | JACS Email | PGP/MIME | DKIM |
|----------|-----------|----------|------|
| What is signed | Header hashes + body hashes + attachment hashes | Body only | Specific headers + body |
| Signature location | JSON attachment (detached) | MIME part (detached) | Email header |
| Survives forwarding | Yes (attachment preserved) | Yes (MIME part preserved) | No (header + body may change) |
| Forwarding chain | Yes (parent_signature_hash) | No | ARC (separate standard) |
| Identity model | Agent registry + DNS TXT | Web of Trust / key servers | Domain DNS (selector._domainkey) |
| Algorithm agility | Yes (from agent ID version) | Yes (declared in signature) | Yes (declared in header) |
| Proves sender identity | Via registry + DNS | Via key fingerprint trust | Via domain ownership |
| Signs From: header | Yes (hashed + raw value) | No | Yes |
| Signs other headers | Yes (To, Subject, Date, Message-ID, threading) | No | Configurable |
| Tamper forensics | Yes (original values preserved) | No | No |

---

## Provider Behavior: What Survives Transit

| Method | Direct delivery | Forward | Reply | Reliability |
|--------|----------------|---------|-------|-------------|
| JSON attachment | Preserved | Preserved | Lost | High |

The JSON attachment is the only method that reliably survives forwarding across
all major providers (Gmail, Outlook, Yahoo, ProtonMail).

---

## Attachment Details

The JACS signature attachment uses:

```
Content-Type: application/json; name="jacs-signature.json"
Content-Disposition: attachment; filename="jacs-signature.json"
```

For forwarding chains, previous signatures are renamed:

```
jacs-signature-0.json    ← original signer
jacs-signature-1.json    ← first forwarder
...
jacs-signature.json      ← most recent signer (always this name)
```

---

## Deprecation: X-JACS-Signature Header (v1/v2)

The `X-JACS-Signature` header-based signing flow (v1 and v2) is **immediately
deprecated**. All implementations must:

1. **Stop producing** `X-JACS-Signature` headers
2. **Stop verifying** `X-JACS-Signature` headers
3. **Remove** header-based signing code paths (e.g., `verify_email_signature()`
   in Python/Node/Rust/Go, `computeContentHash()` for signing purposes)
4. **Use only** the `jacs-signature.json` attachment model

The `sendEmail()` methods in all SDKs must be updated to use the JACS
attachment signing flow. The existing `computeContentHash()` function and
v1/v2 header generation code are removed.

Header-based signing had these limitations that made it unsuitable:
- Did not survive forwarding (headers stripped)
- Could not include per-field hashes (limited header space)
- Could not include raw header values for forensics
- Hardcoded Ed25519 in all SDK implementations
- Algorithm declared in `a=` field was untrusted

---

## Migration Plan (Header Signing Removal)

1. Immediately mark header-based verification/signing APIs as deprecated
   in all SDKs
2. Block new features on the header flow — no bug fixes, no enhancements
3. Remove header flow from default code paths in all SDKs
4. Remove header fixtures from the active conformance suite
5. Keep a temporary compatibility shim only if needed for a rollback
   window (aim to remove within one release cycle)

---

## SDK Consistency Requirement

All SDKs (Rust, Python, Node, Go) MUST:

- Consume the same email fixture suite (`jacs/tests/fixtures/email/`)
- Produce identical hashes for the same fixture inputs
- Use the same error type names (mapped to language-idiomatic casing)
- Return structurally equivalent `EmailVerificationResultV2` responses

Cross-SDK conformance is verified by running each SDK against the shared
fixtures and comparing outputs.

---

## Implementation Plan

### Principles

**DRY (Don't Repeat Yourself):** The core email signing and verification logic
is implemented once in the JACS library (neutral, no HAI dependency). haisdk
wraps it to add HAI-specific trust chain (registry lookup, DNS verification).
The HAI API uses the same JACS functions as the SDK clients.

**TDD (Test-Driven Development):** A shared test fixture suite of raw RFC 5322
emails is created first. Tests are written before implementation. All SDKs run
the same fixture suite to ensure cross-language consistency.

### Architecture: JACS (neutral) vs haisdk (HAI-specific)

The core functions live in the **JACS library** so any email client can use
them without depending on HAI:

```rust
// In JACS library (neutral — no HAI, no network)
// All functions operate on raw bytes (&[u8]), not &str.
// RFC 5322 emails are byte streams, not valid UTF-8.

/// Sign a raw RFC 5322 email, return the email with jacs-signature.json attached.
/// Uses raw byte MIME injection — does NOT re-serialize the email.
pub fn sign_email(raw_email: &[u8], signer: &dyn JacsProvider) -> Result<Vec<u8>>

/// Extract and validate the JACS document from an email.
/// Verifies document hash (via RFC 8785) + cryptographic signature.
/// No network calls. The caller provides the public key.
pub fn verify_email_document(
    raw_email: &[u8],
    public_key_pem: &str,
) -> Result<(JacsEmailSignatureDocument, ParsedEmailParts)>

/// Compare the trusted JACS payload against actual email content.
/// Pure hash comparison, no crypto, no network.
pub fn verify_email_content(
    doc: &JacsEmailSignatureDocument,
    parts: &ParsedEmailParts,
) -> ContentVerificationResult

/// JACS attachment operations (used by sign and verify)
pub fn add_jacs_attachment(raw_email: &[u8], doc: &[u8]) -> Result<Vec<u8>>
pub fn get_jacs_attachment(raw_email: &[u8]) -> Result<Vec<u8>>
pub fn remove_jacs_attachment(raw_email: &[u8]) -> Result<Vec<u8>>
```

The **haisdk** wraps these to add HAI trust chain:

```rust
// In haisdk (HAI-specific — registry lookup, DNS verification)

/// Full email verification: JACS document validation + HAI registry
/// lookup + DNS check + content hash comparison.
pub async fn verify_email(
    raw_email: &[u8],
    hai_url: &str,
) -> EmailVerificationResultV2
```

This separation means:

- **Any email client** can call `jacs::sign_email()` and
  `jacs::verify_email_document()` directly without HAI
- **HAI users** call `haisdk::verify_email()` which adds registry + DNS
- **The HAI API** uses the same JACS functions — same code path as clients
- The JACS library never makes network calls
- haisdk never reimplements crypto or MIME parsing

### Extending `JacsProvider` (not a new trait)

The existing `JacsProvider` trait in haisdk must be extended with the
fields needed for email signing. Do NOT create a separate `JacsSigner` trait:

```rust
pub trait JacsProvider: Send + Sync {
    // Existing methods
    fn jacs_id(&self) -> &str;
    fn sign_string(&self, message: &str) -> Result<String>;
    fn canonical_json(&self, value: &Value) -> Result<String>;
    fn sign_response(&self, payload: &Value) -> Result<SignedPayload>;

    // New methods for email signing
    fn sign_bytes(&self, data: &[u8]) -> Result<Vec<u8>>;
    fn key_id(&self) -> &str;
    fn algorithm(&self) -> &str;   // resolved from agent ID version
}
```

`SimpleAgent` in JACS needs a `key_id()` accessor added.

### Test Fixture Suite

A shared directory of raw RFC 5322 email fixtures for TDD:

```
jacs/tests/fixtures/email/
├── simple_text.eml              # plain text only, no attachments
├── html_only.eml                # HTML body only
├── multipart_alternative.eml    # text/plain + text/html
├── with_attachments.eml         # body + 2 file attachments
├── with_inline_images.eml       # body + inline image attachments
├── threaded_reply.eml           # In-Reply-To + References headers
├── forwarded_chain.eml          # email with jacs-signature-0.json + jacs-signature.json
├── unicode_headers.eml          # non-ASCII subject and body (RFC 2047, NFC edge cases)
├── folded_headers.eml           # continuation lines, WSP normalization
├── mixed_charsets.eml           # quoted-printable vs base64 bodies
├── embedded_images.eml          # inline Content-Disposition images
└── README.md                    # describes each fixture and expected results
```

Each fixture has a corresponding expected-result JSON:

```
jacs/tests/fixtures/email/expected/
├── simple_text.json             # expected payload hashes, header values
├── html_only.json
├── multipart_alternative.json
├── with_attachments.json
└── ...
```

**Test categories** (written before implementation):

| Test | Fixture | What it validates |
|------|---------|-------------------|
| `sign_roundtrip` | `simple_text.eml` | Sign → verify → `valid: true`, zero tampered fields |
| `sign_multipart` | `multipart_alternative.eml` | Both body_plain and body_html entries are populated with content + MIME hashes |
| `sign_attachments` | `with_attachments.eml` | Attachment hashes are sorted and correct |
| `sign_inline_images` | `embedded_images.eml` | Inline images are hashed as attachments |
| `tamper_header` | `simple_text.eml` | Sign, modify From, verify → `tampered_fields` contains `headers.from` |
| `tamper_body` | `simple_text.eml` | Sign, modify body, verify → `tampered_fields` contains `body_plain` |
| `tamper_attachment` | `with_attachments.eml` | Sign, modify attachment, verify → attachment hash mismatch |
| `strip_body_part` | `multipart_alternative.eml` | Sign, strip text/plain, verify → html valid, plain "unverifiable" |
| `forwarding_chain` | `forwarded_chain.eml` | Verify full chain, both signers valid |
| `broken_chain` | `forwarded_chain.eml` | Tamper parent attachment → chain validation failure |
| `multi_algorithm` | `simple_text.eml` | Sign with RSA-PSS, verify → algorithm resolved from agent ID |
| `canonicalization` | `folded_headers.eml`, `unicode_headers.eml`, `mixed_charsets.eml` | Cross-language hash agreement |
| `missing_signature` | plain email (no attachment) | Verify → `MissingJacsSignature` error |
| `cross_language` | all fixtures | Same fixture produces identical hashes in Rust, Python, Node, Go |

### Phase 1: JACS library (neutral core)

**New crate dependencies** (in JACS `Cargo.toml`):

```toml
mail-parser = "0.9"              # RFC 5322 parsing (read-only, zero-copy)
unicode-normalization = "0.1"    # UTF-8 NFC normalization for canonicalization
```

Do NOT add `mail-builder`. MIME attachment injection is implemented as raw
byte manipulation (see **MIME Attachment Injection** above).

The JACS library already has SHA-256, signing capabilities, and
`serde_json_canonicalizer` for RFC 8785. The new module adds MIME handling
on top.

### Phase 2: haisdk Rust wrapper

haisdk does NOT duplicate MIME parsing or crypto. It imports the JACS email
module and adds HAI-specific logic on top.

**New dependency** (`rust/haisdk/Cargo.toml`):

```toml
jacs = { path = "..." }  # already a dependency — gains email module
```

No need for `mail-parser` in haisdk — that lives in JACS.

**New types** (`rust/haisdk/src/types.rs`) — HAI-specific result types only.
The core types (`EmailSignaturePayload`, `JacsEmailSignatureDocument`, etc.)
are defined in JACS and re-exported:

```rust
// Core types defined in JACS library (neutral), re-exported by haisdk.

pub struct SignedHeaderEntry {
    pub value: String,           // raw header value
    pub hash: String,            // "sha256:<hex>"
}

pub struct BodyPartEntry {
    pub content_hash: String,
    pub mime_headers_hash: String,
}

pub struct AttachmentEntry {
    pub content_hash: String,
    pub mime_headers_hash: String,
    pub filename: String,
}

pub struct EmailSignaturePayload {
    pub headers: EmailSignatureHeaders,
    pub body_plain: Option<BodyPartEntry>,
    pub body_html: Option<BodyPartEntry>,
    pub attachments: Vec<AttachmentEntry>,
    pub parent_signature_hash: Option<String>,
}

pub struct EmailSignatureHeaders {
    pub from: SignedHeaderEntry,
    pub to: SignedHeaderEntry,
    pub subject: SignedHeaderEntry,
    pub date: SignedHeaderEntry,
    pub message_id: SignedHeaderEntry,
    pub in_reply_to: Option<SignedHeaderEntry>,
    pub references: Option<SignedHeaderEntry>,
}

pub struct JacsEmailSignatureDocument {
    pub version: String,              // "1.0"
    pub document_type: String,        // "email_signature"
    pub payload: EmailSignaturePayload,
    pub metadata: JacsEmailMetadata,
    pub signature: JacsEmailSignature,
}

// HAI-specific type (defined in haisdk, not JACS).
// Wraps the JACS ContentVerificationResult and adds registry + DNS fields.
pub struct EmailVerificationResultV2 {
    pub valid: bool,
    pub jacs_id: String,
    pub algorithm: String,
    pub reputation_tier: String,
    pub dns_verified: Option<bool>,
    pub tampered_fields: Vec<TamperedField>,
    pub original_values: HashMap<String, String>,
    pub chain: Vec<ChainEntry>,
    pub error: Option<String>,
}

pub struct TamperedField {
    pub field: String,           // e.g., "headers.from", "body_plain"
    pub original_hash: String,
    pub current_hash: String,
    pub original_value: Option<String>,
    pub current_value: Option<String>,
}

pub struct ChainEntry {
    pub signer: String,
    pub jacs_id: String,
    pub valid: bool,
    pub forwarded: bool,
}
```

**JACS library new module** (`jacs/src/email.rs`) — all core logic:

| Function | Purpose |
|----------|---------|
| `sign_email()` | Parse email → compute hashes → build JACS doc → attach |
| `verify_email_document()` | Extract JACS attachment → verify hash + signature |
| `verify_email_content()` | Compare payload hashes against actual email |
| `extract_email_parts()` | Parse raw RFC 5322 → headers, body parts, attachments |
| `canonicalize_header()` | DKIM-style relaxed normalization + RFC 2047 decode + NFC |
| `compute_header_entry()` | Hash a canonicalized header value |
| `compute_body_hash()` | SHA-256 of canonicalized body content |
| `compute_attachment_hash()` | SHA-256 of `filename_nfc:content_type_lower:raw_bytes` |
| `build_jacs_email_document()` | Assemble the JACS document from payload + sign |
| `add_jacs_attachment()` | Inject attachment into raw MIME via byte manipulation |
| `get_jacs_attachment()` | Extract jacs-signature.json from raw email |
| `remove_jacs_attachment()` | Remove jacs-signature.json for verification |

**haisdk wrapper module** (`rust/haisdk/src/email.rs`) — HAI trust chain only:

| Function | Purpose |
|----------|---------|
| `verify_email()` | Calls JACS verify functions + HAI registry lookup + DNS |
| `fetch_public_key_from_registry()` | GET /api/agents/keys/{email} |
| `verify_dns_public_key()` | Check public key hash against DNS TXT record |

**Files changed — JACS library:**

| File | Change |
|------|--------|
| `jacs/src/email.rs` | **NEW** — all email functions (sign, verify, MIME ops, canonicalization) |
| `jacs/src/lib.rs` | Add `pub mod email;` |
| `jacs/src/types.rs` (or inline) | Add `SignedHeaderEntry`, `EmailSignaturePayload`, `EmailSignatureHeaders`, `JacsEmailSignatureDocument`, `ParsedEmailParts`, `ContentVerificationResult` |
| `jacs/Cargo.toml` | Add `mail-parser`, `unicode-normalization` |
| `jacs/src/simple.rs` | Add `key_id()` accessor to `SimpleAgent` |

**Files changed — haisdk:**

| File | Change |
|------|--------|
| `rust/haisdk/src/email.rs` | **NEW** — `verify_email()` wrapper (HAI trust chain) |
| `rust/haisdk/src/types.rs` | Add `EmailVerificationResultV2`, `TamperedField`, `ChainEntry`; re-export JACS types |
| `rust/haisdk/src/lib.rs` | Add `pub mod email;` + re-exports |
| `rust/haisdk/src/jacs.rs` | Extend `JacsProvider` trait with `sign_bytes()`, `key_id()`, `algorithm()` |
| `rust/haisdk/src/verify.rs` | Remove legacy `verify_email_signature()` header-based flow |

### Phase 3: hai API endpoints

Add two new endpoints. The hai API uses the **same JACS library functions**
as the SDK clients — there is no separate server-side verification path.

```
POST /api/v1/email/sign
  Body: { "raw_email": "<base64-encoded RFC 5322 bytes>" }
  Response: { "signed_email": "<base64-encoded RFC 5322 bytes with jacs-signature.json>" }

POST /api/v1/email/verify
  Body: { "raw_email": "<base64-encoded RFC 5322 bytes>" }
  Response: EmailVerificationResultV2
```

The existing `sendEmail()` flow must be updated to use `sign_email()` from
the JACS library internally. The legacy `jacs_email.rs` header-based flow
is removed.

**Files changed in hai:**

| File | Change |
|------|--------|
| `hai/api/src/routes/agent_email.rs` | Add `/api/v1/email/sign` and `/api/v1/email/verify`; update `send_email` to use JACS attachment signing |
| `hai/api/src/hai_signing.rs` | Extend to implement full `JacsProvider` (add `sign_bytes`, `key_id`, `algorithm`) |
| `hai/api/src/jacs_email.rs` | Remove header-based signing/verification code |

### Phase 4: SDK clients (Python, Node, Go)

Each SDK:
1. Calls the new hai API endpoints for sign/verify
2. Uses the JACS email parsing functions for client-side operations
3. Updates `sendEmail()` to use JACS attachment signing
4. Removes all header-based signing code (`computeContentHash()`,
   `verify_email_signature()`, `X-JACS-Signature` generation)

**Python** (`python/src/jacs/hai/client.py` and `async_client.py`):

```python
def sign_email(self, raw_email: bytes) -> bytes:
    """POST /api/v1/email/sign → returns signed email bytes."""

def verify_email(self, raw_email: bytes) -> EmailVerificationResultV2:
    """POST /api/v1/email/verify → returns verification result."""
```

New types in `models.py`:

```python
@dataclass
class TamperedField:
    field: str
    original_hash: str
    current_hash: str
    original_value: Optional[str] = None
    current_value: Optional[str] = None

@dataclass
class ChainEntry:
    signer: str
    jacs_id: str
    valid: bool
    forwarded: bool = False

@dataclass
class EmailVerificationResultV2:
    valid: bool
    jacs_id: str
    algorithm: str
    reputation_tier: str
    dns_verified: Optional[bool] = None
    tampered_fields: list[TamperedField] = field(default_factory=list)
    original_values: dict[str, str] = field(default_factory=dict)
    chain: list[ChainEntry] = field(default_factory=list)
    error: Optional[str] = None
```

Python `email.message.EmailMessage` objects are also accepted (converted
via `msg.as_bytes()` before sending to the API), as long as tests pass.

**Node** (`node/src/client.ts`):

```typescript
async signEmail(rawEmail: Buffer): Promise<Buffer>
async verifyEmail(rawEmail: Buffer): Promise<EmailVerificationResultV2>
```

New types in `types.ts`:

```typescript
interface TamperedField {
  field: string;
  originalHash: string;
  currentHash: string;
  originalValue?: string;
  currentValue?: string;
}

interface ChainEntry {
  signer: string;
  jacsId: string;
  valid: boolean;
  forwarded: boolean;
}

interface EmailVerificationResultV2 {
  valid: boolean;
  jacsId: string;
  algorithm: string;
  reputationTier: string;
  dnsVerified: boolean | null;
  tamperedFields: TamperedField[];
  originalValues: Record<string, string>;
  chain: ChainEntry[];
  error: string | null;
}
```

**Go** (`go/client.go`):

```go
func (c *Client) SignEmail(ctx context.Context, rawEmail []byte) ([]byte, error)
func (c *Client) VerifyEmail(ctx context.Context, rawEmail []byte) (*EmailVerificationResultV2, error)
```

**Files changed per SDK:**

| SDK | Files changed | Files removed/deprecated |
|-----|---------------|------------------------|
| Python | `client.py`, `async_client.py`, `models.py` | Remove `signing.py` header flow, `computeContentHash()` |
| Node | `client.ts`, `types.ts` | Remove `signing.ts` header flow, `computeContentHash()` |
| Go | `client.go`, `types.go` | Remove header-based signing in `email_verify.go` |

### Phase 4 SDK Test Plan

Each SDK must have these tests for the new attachment-based flow:

| Test | What it validates |
|------|-------------------|
| `test_sign_email_roundtrip` | Sign a raw email, verify the result contains `jacs-signature.json` |
| `test_verify_email_valid` | Verify a pre-signed email returns `valid=True` with correct fields |
| `test_verify_email_tampered` | Modify a signed email, verify returns `tampered_fields` |
| `test_verify_email_chain` | Verify a forwarded email returns chain entries |
| `test_verify_email_missing_signature` | Email without attachment returns `MissingJacsSignature` |
| `test_verify_email_network_error` | Server unreachable returns error gracefully |
| `test_send_email_uses_jacs_attachment` | `sendEmail()` produces email with `jacs-signature.json` |
| Async parity tests | All above repeated for async client (Python, Node) |

### Implementation Order (TDD)

Tests are written FIRST, using the shared email fixture suite. Implementation
follows to make the tests pass.

```
 1. Create jacs/tests/fixtures/email/ with .eml files + expected results
 2. Write test stubs in JACS: sign_roundtrip, tamper_*, forwarding_chain,
    canonicalization
 3. Add mail-parser + unicode-normalization to JACS Cargo.toml
 4. Add types to JACS (SignedHeaderEntry, EmailSignaturePayload, etc.)
 5. Extend JacsProvider trait with sign_bytes(), key_id(), algorithm()
 6. Implement canonicalization functions in JACS
 7. Implement sign_email() in JACS — make roundtrip test pass
 8. Implement verify_email_document() in JACS — make tamper tests pass
 9. Implement verify_email_content() in JACS — make content comparison pass
10. Add forwarding chain support — make chain tests pass
11. Wire haisdk wrapper: verify_email() with HAI registry + DNS
12. Remove legacy header-based signing from haisdk verify.rs
13. Add hai API endpoints (Phase 3)
14. Update sendEmail() in hai API to use JACS attachment signing
15. Add HTTP wrappers to Python, Node, Go SDKs (Phase 4)
16. Remove legacy header signing code from all SDKs
17. Run cross-SDK contract tests against same fixtures
18. Update this document with final API signatures
```

### JACS Crate Publishing

JACS will be published first or at the same time as haisdk changes. haisdk
may use a local path dependency (`jacs = { path = "..." }`) during
development, but the published version must reference a released JACS crate.
