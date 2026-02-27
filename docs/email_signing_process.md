# JACS Email Signature: Signing and Verification Process

## Overview

JACS email signatures use a **detached signature** model. A standard JACS
document is attached to the email as `jacs-signature.json`. The document
contains hashes of the email headers, body, and other attachments. Verification
is a two-phase process:

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
│     a. Extract hashes from the trusted JACS         │
│        document payload:                            │
│        - header_hashes.from                         │
│        - header_hashes.to                           │
│        - header_hashes.subject                      │
│        - body_hash                                  │
│        - attachment_hashes[]                        │
│     b. Recompute hashes from the actual email       │
│     c. Compare each hash                            │
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
│   ├── Date: ...
│   ├── Message-ID: ...
│   └── X-JACS-Signature: v=2; ...          (optional fast-path, see below)
├── multipart/mixed
│   ├── text/html or text/plain              (the email body)
│   ├── report.pdf                           (user attachment)
│   ├── data.csv                             (user attachment)
│   └── jacs-signature.json                  (THE DETACHED JACS SIGNATURE)
```

---

## JACS Signature Document Format

The `jacs-signature.json` attachment is a standard JACS document:

```json
{
  "version": "1.0",
  "document_type": "email_signature",
  "payload": {
    "header_hashes": {
      "from": "sha256:<hex>",
      "to": "sha256:<hex>",
      "subject": "sha256:<hex>"
    },
    "body_hash": "sha256:<hex>",
    "attachment_hashes": [
      "sha256:<hex>",
      "sha256:<hex>"
    ]
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

### Hash Computation

**Header hashes** &mdash; each header value is hashed individually:

```
header_hashes.from    = sha256(lowercase(From header value))
header_hashes.to      = sha256(lowercase(To header value))
header_hashes.subject = sha256(Subject header value)
```

**Body hash:**

```
body_hash = sha256(body_text)
```

Where `body_text` is the plain text content (or `text/html` content if no
`text/plain` part exists).

**Attachment hashes** &mdash; each non-JACS attachment is hashed and the list
is sorted lexicographically for determinism:

```
attachment_hash = sha256(filename + ":" + content_type + ":" + raw_bytes)
attachment_hashes = sort([hash_1, hash_2, ...])
```

The `jacs-signature.json` attachment itself is excluded from the attachment
hash list.

---

## Signing Flow (Sender)

```
1. Compose the email (headers, body, attachments)

2. Compute hashes:
   a. Hash each relevant header (From, To, Subject)
   b. Hash the body
   c. Hash each attachment (filename:content_type:data), sort the hashes

3. Build the JACS document payload:
   {
     "header_hashes": { "from": "...", "to": "...", "subject": "..." },
     "body_hash": "...",
     "attachment_hashes": ["...", "..."]
   }

4. Sign the JACS document using the agent's private key:
   - Canonicalize the payload (sorted keys, no whitespace)
   - Compute metadata.hash = sha256(canonical_payload)
   - Sign using the agent's key (algorithm stored in signature.algorithm)
   - The algorithm is NOT assumed to be ed25519 — it comes from the agent's
     registered key type

5. Attach jacs-signature.json to the email

6. (Optional) Add X-JACS-Signature header as a fast-path hint for
   direct-delivery verification
```

---

## Verification Flow (Receiver)

```
1. Find the jacs-signature.json attachment in the email
   - If not found, fall back to X-JACS-Signature header (legacy v1/v2 flow)

2. Validate the JACS document (standard JACS verification):
   a. Parse the JSON as a JacsDocument
   b. Verify the document hash:
      - Canonicalize payload → compute SHA-256 → compare to metadata.hash
   c. Identify the signer:
      - Extract metadata.issuer (the agent's JACS ID)
   d. Fetch the public key from HAI registry:
      - GET /api/agents/keys/{from_email}
      - Response includes: public_key, algorithm, reputation_tier
   e. Verify the algorithm matches:
      - signature.algorithm in the JACS document must match the registry's
        algorithm for this agent
   f. Verify the cryptographic signature:
      - Use the algorithm from the JACS document (ed25519, rsa-pss, or pq2025)
      - The key type is NOT hardcoded — it comes from the registry
   g. If reputation_tier is "dns_certified" or "fully_certified":
      - Extract domain from From email address
      - Query DNS TXT at _v1.agent.jacs.{domain}
      - Compute sha256(public_key_pem_bytes), base64 encode
      - Compare against jacs_public_key_hash= value in the TXT record
      - If mismatch → FAIL

   At this point the JACS document is trusted. Its payload can be used.

3. Validate the email contents against the trusted payload:
   a. Recompute header hashes from the actual email:
      - sha256(lowercase(From)) → compare to payload.header_hashes.from
      - sha256(lowercase(To))   → compare to payload.header_hashes.to
      - sha256(Subject)         → compare to payload.header_hashes.subject
   b. Recompute body hash:
      - sha256(body_text)       → compare to payload.body_hash
   c. Recompute attachment hashes:
      - For each non-JACS attachment: sha256(filename:content_type:data)
      - Sort lexicographically
      - Compare list to payload.attachment_hashes

   If any hash mismatches → the email has been tampered with.

4. Return verification result:
   - valid: true/false
   - jacs_id: the signer's agent ID
   - algorithm: the algorithm used (from the JACS document)
   - reputation_tier: from the registry
   - dns_verified: true/false/null
   - tampered_fields: list of mismatched fields (if any)
```

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

### Clean separation of concerns

- **JACS layer**: "Is this document authentically from agent X?" (crypto,
  identity, DNS)
- **Email layer**: "Does this email match what agent X signed?" (hash
  comparison)

The email layer never touches cryptography. The JACS layer never thinks about
email structure. Each does one thing well.

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

New implementations should produce both the attachment and the header. Verifiers
should prefer the attachment when present.

---

## Relationship to PGP/MIME and DKIM

| Property | JACS Email | PGP/MIME | DKIM |
|----------|-----------|----------|------|
| What is signed | Header hashes + body hash + attachment hashes | Body only | Specific headers + body |
| Signature location | JSON attachment (detached) | MIME part (detached) | Email header |
| Survives forwarding | Yes (attachment preserved) | Yes (MIME part preserved) | No (header + body may change) |
| Identity model | Agent registry + DNS TXT | Web of Trust / key servers | Domain DNS (selector._domainkey) |
| Algorithm agility | Yes (declared in JACS doc) | Yes (declared in signature) | Yes (declared in header) |
| Proves sender identity | Via registry + DNS | Via key fingerprint trust | Via domain ownership |
| Signs From: header | Yes (hashed in payload) | No | Yes |
| Signs other headers | Yes (To, Subject hashed) | No | Configurable |

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

## Open Questions

- Should `header_hashes` include additional headers (Date, Message-ID,
  References)?
- Should the body hash cover `text/html` and `text/plain` separately when both
  are present in a `multipart/alternative`?
- Should the JACS document include the raw header values (not just hashes) so
  a verifier can display "this email claimed to be from X" even if the From
  header was modified?
- What `Content-Type` and filename should the JACS attachment use? Proposed:
  `application/json; name="jacs-signature.json"` with
  `Content-Disposition: attachment; filename="jacs-signature.json"`
