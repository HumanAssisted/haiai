# JACS Email Signature: Signing and Verification Process

## Overview

JACS email signatures use a **detached signature** model. A standard JACS
document is attached to the email as `jacs-signature.json`. The document
contains hashes and raw values of the email headers, body, and other
attachments. Verification is a two-phase process:

1. **Validate the JACS document** using standard JACS verification (crypto +
   identity).
2. **Validate the email contents** against the hashes inside the now-trusted
   JACS document.

This separation means multi-algorithm support, DNS verification, and registry
lookup are handled entirely by the JACS layer. The email-specific code only
needs to compute hashes and compare them.

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
│        payload)                                     │
│     c. Verify cryptographic signature using the     │
│        algorithm declared in signature.algorithm    │
│        (ed25519, rsa-pss, or pq2025)               │
│     d. Fetch public key from HAI registry           │
│        GET /api/agents/keys/{from_email}            │
│        → returns public_key, algorithm,             │
│          reputation_tier                            │
│     e. If reputation_tier is dns_certified or       │
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
│   ├── References: <orig789@example.com>            (if in thread)
│   └── X-JACS-Signature: v=2; ...                   (optional fast-path)
├── multipart/mixed
│   ├── multipart/alternative
│   │   ├── text/plain                               (plain text body)
│   │   └── text/html                                (HTML body)
│   ├── report.pdf                                   (user attachment)
│   ├── data.csv                                     (user attachment)
│   └── jacs-signature.json                          (DETACHED JACS SIGNATURE)
```

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
    "body_hash_plain": "sha256:<hex>",
    "body_hash_html": "sha256:<hex>",
    "attachment_hashes": [
      "sha256:<hex>",
      "sha256:<hex>"
    ],
    "parent_signature_hash": null
  },
  "metadata": {
    "issuer": "<agent-jacs-id>",
    "document_id": "<uuid>",
    "created_at": "<ISO 8601>",
    "hash": "sha256:<hex of canonical payload>"
  },
  "signature": {
    "key_id": "hai-pq2025-001",
    "algorithm": "pq2025",
    "signature": "<base64>",
    "signed_at": "<ISO 8601>"
  }
}
```

### Signed Headers

The following headers are included in the JACS document payload:

| Header | Key in payload | Always present | Notes |
|--------|---------------|----------------|-------|
| From | `headers.from` | Yes | Lowercased before hashing |
| To | `headers.to` | Yes | Lowercased before hashing |
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

For From and To, the value is lowercased before hashing to normalize case
differences introduced by mail servers.

### Body Hashes

Both text/plain and text/html parts are hashed separately:

```
body_hash_plain = sha256(text/plain content)    # null if no text/plain part
body_hash_html  = sha256(text/html content)     # null if no text/html part
```

The verifier checks whichever part(s) are present. If a mail provider strips
one format (e.g., drops text/plain), the other hash still allows partial
verification. The verifier should report which body format(s) could be
verified.

### Attachment Hashes

Each non-JACS attachment is hashed and the list is sorted lexicographically
for determinism:

```
attachment_hash = sha256(filename + ":" + content_type + ":" + raw_bytes)
attachment_hashes = sort([hash_1, hash_2, ...])
```

The `jacs-signature.json` attachment itself is excluded from the list.

### Parent Signature Hash (Forwarding Chain)

When an email is forwarded by a JACS-signing agent, the forwarder's JACS
document includes `parent_signature_hash` — the SHA-256 hash of the previous
`jacs-signature.json`. See the **Forwarding Chain** section below.

---

## Signing Flow (Sender)

```
1. Compose the email (headers, body, attachments)

2. Compute hashes:
   a. For each header (From, To, Subject, Date, Message-ID,
      In-Reply-To, References):
      - Store the raw value
      - Compute sha256(value) — lowercase From and To before hashing
   b. Hash the text/plain body → body_hash_plain
   c. Hash the text/html body  → body_hash_html
   d. Hash each attachment (filename:content_type:data), sort the hashes
   e. Set parent_signature_hash = null (unless forwarding, see below)

3. Build the JACS document payload with headers (value + hash),
   body hashes, and attachment hashes

4. Sign the JACS document using the agent's private key:
   - Canonicalize the payload (sorted keys, no whitespace)
   - Compute metadata.hash = sha256(canonical_payload)
   - Sign using the agent's key (algorithm stored in signature.algorithm)
   - The algorithm is NOT assumed — it comes from the agent's key type

5. Attach jacs-signature.json to the email
   Content-Type: application/json; name="jacs-signature.json"
   Content-Disposition: attachment; filename="jacs-signature.json"

6. (Optional) Add X-JACS-Signature header as a fast-path hint
```

---

## Verification Flow (Receiver)

```
1. Find the jacs-signature.json attachment in the email
   - If not found, fall back to X-JACS-Signature header (legacy v1/v2)
   - If multiple jacs-signature.json files exist, this is a forwarding
     chain — see below

2. Validate the JACS document (standard JACS verification):
   a. Parse the JSON as a JacsDocument
   b. Verify the document hash:
      - Canonicalize payload → SHA-256 → compare to metadata.hash
   c. Identify the signer:
      - Extract metadata.issuer (the agent's JACS ID)
   d. Fetch the public key from HAI registry:
      - GET /api/agents/keys/{payload.headers.from.value}
      - Response: public_key, algorithm, reputation_tier
   e. Verify algorithm matches:
      - signature.algorithm must match the registry's algorithm
   f. Verify the cryptographic signature:
      - Use the algorithm from the JACS document
      - The key type is NOT hardcoded
   g. If reputation_tier is "dns_certified" or "fully_certified":
      - Extract domain from the From email
      - Query DNS TXT at _v1.agent.jacs.{domain}
      - Compute sha256(public_key_pem_bytes), base64 encode
      - Compare against jacs_public_key_hash= in the TXT record
      - Mismatch → FAIL

   The JACS document is now trusted. Its payload can be used.

3. Validate email contents against the trusted payload:
   a. For each header in payload.headers:
      - Recompute sha256(actual_header_value)
      - Compare to payload.headers.{field}.hash
      - On mismatch: report tampered field, show original value
        from payload.headers.{field}.value vs current value
   b. Recompute body hashes:
      - sha256(text/plain) → compare to payload.body_hash_plain
      - sha256(text/html)  → compare to payload.body_hash_html
      - If one part is missing (provider stripped it), report as
        "unverifiable" rather than "tampered"
   c. Recompute attachment hashes:
      - For each non-JACS attachment: sha256(filename:content_type:data)
      - Sort lexicographically
      - Compare to payload.attachment_hashes
   d. Check parent_signature_hash for forwarding chain (see below)

4. Return verification result:
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

When a JACS-signing agent forwards an email, it creates a **new** JACS
signature document that wraps the previous one. This creates a verifiable chain
of custody: original signer → forwarder → recipient.

### How it works

```
Original email from Agent A:
├── body: "Hello, here's the report"
├── report.pdf
└── jacs-signature.json              ← signed by Agent A
    {
      payload: {
        headers: { from: { value: "agentA@x.com", ... }, ... },
        body_hash_plain: "sha256:aaa...",
        attachment_hashes: ["sha256:bbb..."],
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
        body_hash_plain: "sha256:ccc...",
        attachment_hashes: ["sha256:bbb..."],
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

3. Compose the forwarded email (new headers, possibly new body)

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
   a. Find jacs-signature-0.json (or iterate jacs-signature-{N}.json)
   b. Compute sha256(raw bytes of that file)
   c. Compare to parent_signature_hash → must match
   d. Recursively validate the parent JACS document
   e. Continue until parent_signature_hash is null (the original)

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

---

## Why This Design

### Multi-algorithm support comes for free

The JACS document's `signature.algorithm` field declares the algorithm. The
standard JACS verification path already handles Ed25519, RSA-PSS, and PQ2025.
Email verification code does not need its own algorithm dispatch.

### Survives email forwarding

X-headers are stripped when emails are forwarded. Attachments are preserved by
all major email providers. The `jacs-signature.json` attachment survives
forwarding, making verification possible even after the email has been relayed.

### Chain of custody for forwarding

The `parent_signature_hash` field creates a tamper-evident chain. Each
forwarder signs a new JACS document that references the previous one. The full
chain can be verified back to the original sender.

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

## Backward Compatibility

### X-JACS-Signature header (legacy / fast-path)

For backward compatibility and as an optimization for direct delivery, the
`X-JACS-Signature` header is still supported:

```
X-JACS-Signature: v=2; a=ed25519; id={jacsId}; from={email};
                  h={content_hash}; jv={jacs_version}; t={timestamp};
                  s={base64_signature}
```

The verification flow checks for `jacs-signature.json` first. If not found, it
falls back to the header-based flow (v1/v2). The header-based flow is limited:

- Algorithm is declared in `a=` but SDK verification must read it from the
  registry rather than trusting the header
- Does not survive forwarding (headers are stripped)
- Cannot include per-field hashes (limited header space)
- Cannot include raw header values for forensics

New implementations should produce both the attachment and the header. Verifiers
should prefer the attachment when present.

---

## Relationship to PGP/MIME and DKIM

| Property | JACS Email | PGP/MIME | DKIM |
|----------|-----------|----------|------|
| What is signed | Header hashes + body hashes + attachment hashes | Body only | Specific headers + body |
| Signature location | JSON attachment (detached) | MIME part (detached) | Email header |
| Survives forwarding | Yes (attachment preserved) | Yes (MIME part preserved) | No (header + body may change) |
| Forwarding chain | Yes (parent_signature_hash) | No | ARC (separate standard) |
| Identity model | Agent registry + DNS TXT | Web of Trust / key servers | Domain DNS (selector._domainkey) |
| Algorithm agility | Yes (declared in JACS doc) | Yes (declared in signature) | Yes (declared in header) |
| Proves sender identity | Via registry + DNS | Via key fingerprint trust | Via domain ownership |
| Signs From: header | Yes (hashed + raw value) | No | Yes |
| Signs other headers | Yes (To, Subject, Date, Message-ID, threading) | No | Configurable |
| Tamper forensics | Yes (original values preserved) | No | No |

---

## Provider Behavior: What Survives Transit

| Method | Direct delivery | Forward | Reply | Reliability |
|--------|----------------|---------|-------|-------------|
| JSON attachment | Preserved | Preserved | Lost | High |
| X-JACS-Signature header | Preserved | Lost | Lost | Medium |
| Hidden HTML div | Mostly preserved | Preserved in quoted body | Preserved | Low (Outlook issues) |
| HTML comments | Stripped by Gmail/Yahoo | N/A | N/A | None |
| data-* attributes | Stripped by Gmail | N/A | N/A | None |

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
