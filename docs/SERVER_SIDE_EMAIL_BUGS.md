# Server-Side Email Bugs — For Backend Team

**Date:** 2026-03-25
**Reporter:** SDK team (Jonathan)
**Context:** Found while debugging reply-email in the SDK. Two independent server-side issues.

---

## Bug 1: `/api/agents/{id}/email/reply` delivers emails with empty JACS signatures

### What happened

The CLI ran `haiai reply-email` which hit the server-side reply endpoint. The server constructed the reply, added a JACS signature MIME part, but **the signature content was empty**. The email was delivered anyway.

### Evidence

Delivered email received by `jonathan@hai.io` (full headers available on request):

```
Content-Type: multipart/mixed; boundary="jacs_d084451141b550da"

--jacs_d084451141b550da
Content-Type: text/plain; charset=utf-8
Content-Transfer-Encoding: 7bit

[body text here]

--jacs_d084451141b550da
Content-Type: application/json; name="hai.ai.signature.jacs.json"
Content-Disposition: attachment; filename="hai.ai.signature.jacs.json"
Content-Transfer-Encoding: 7bit

                            <-- EMPTY: no signature content

--jacs_d084451141b550da--
```

The `hai.ai.signature.jacs.json` attachment is completely empty — zero bytes between the MIME headers and the closing boundary.

### Two things that should have prevented this

1. **The signing step failed silently.** Whatever code path produces the JACS signature attachment for server-side composed emails (the reply endpoint) is generating an empty attachment instead of a valid signed JSON document. This might be a different code path than the `/email/send` endpoint — worth checking if they share signing logic.

2. **No delivery gate checks signature validity.** Before handing a signed email to SES for delivery, the server should validate that:
   - The `hai.ai.signature.jacs.json` attachment exists AND is non-empty
   - The JSON inside the attachment parses as a valid JACS document
   - The signature actually verifies against the agent's public key

   If any of these fail, the email should be rejected (HTTP 500 or 422), not delivered with an empty signature. An empty JACS attachment is worse than no attachment — it implies the email was signed when it wasn't.

### Where to look

- The `/api/agents/{id}/email/reply` handler — specifically the JACS signing step
- Compare with `/api/agents/{id}/email/send` which appears to work correctly
- The delivery pipeline between "email composed" and "handed to SES" — is there a validation gate?

### Impact

Any email sent via the server-side reply endpoint is delivered with a fake (empty) JACS signature. Recipients who check signatures will see an invalid/missing signature. This undermines the entire trust model — signed emails should either be properly signed or not sent at all.

---

## Bug 2: Inbound email subjects stored with CR/LF from header folding

### What happened

When the server stores an inbound email, the `subject` field retains raw CR/LF characters from RFC 5322 header folding. For example, a long subject like:

```
Subject: Welcome to HAI - Your Agent Platform
    for Autonomous Communication
```

Gets stored in the database as `"Welcome to HAI - Your Agent Platform\r\n    for Autonomous Communication"` instead of the unfolded `"Welcome to HAI - Your Agent Platform for Autonomous Communication"`.

When the reply endpoint reads this subject back and prepends "Re: ", the CR/LF passes through to the outbound email header, which correctly gets rejected:

```
Error: HAI API error (400): Invalid characters in 'subject': header values must not contain CR or LF
```

### The fix

**On ingest** (root cause): When the filter worker or API stores an inbound email, unfold the subject per RFC 5322 section 2.2.3 — replace `CRLF + WSP` sequences with a single space, then strip any remaining bare CR or LF. This should happen before writing to the database.

Pseudocode:
```python
# RFC 5322 unfolding: CRLF followed by whitespace = single space
subject = re.sub(r'\r\n[ \t]', ' ', raw_subject)
# Strip any remaining bare CR/LF
subject = subject.replace('\r', '').replace('\n', '')
```

**On reply composition** (defensive): When the reply endpoint constructs "Re: {original_subject}", also strip CR/LF from the stored subject before using it. This protects against subjects that were stored before the ingest fix.

### Where to look

- The email ingest pipeline (filter worker, SES inbound handler, or wherever `subject` is extracted from the raw email and written to the DB)
- The reply endpoint's subject construction logic
- Worth auditing other stored header fields (From display name, etc.) for the same issue

### SDK-side mitigation

We've already added CR/LF stripping in all SDK reply methods as defense-in-depth. But the SDK fix only helps for client-side composed replies — the server-side reply endpoint still needs fixing.

---

## Bug 3: Deprecated/internal endpoints still publicly exposed

### Endpoints to remove or restrict

The SDK has been updated so that **all outbound email** (send, reply, forward) is constructed and JACS-signed client-side, then sent via a single endpoint: `POST /api/agents/{id}/email/send-signed`.

The following endpoints are no longer used by any SDK version and should be removed from the public API or restricted to internal/service auth:

| Endpoint | Status | Action |
|----------|--------|--------|
| `POST /api/agents/{id}/email/send` | Used internally by postmaster only | **Remove from public API** or restrict to internal service auth. Leaving it exposed allows agents to send unsigned emails, bypassing the JACS trust model. |
| `POST /api/agents/{id}/email/reply` | Not used by any SDK | **Remove.** Server-side reply produced empty JACS signatures (Bug 1). SDK now does reply client-side. |
| `POST /api/agents/{id}/email/forward` | Not used by any SDK | **Remove.** Same server-side signing issue. SDK now does forward client-side. |

### What the SDK uses now

All outbound email flows through exactly two endpoints:

1. `GET /api/agents/{id}/email/messages/{msg_id}` — fetch original (for reply/forward)
2. `POST /api/agents/{id}/email/send-signed` — receive client-signed RFC 5322 bytes, validate JACS signature, countersign, deliver

No other outbound email endpoints are needed.

---

## Priority

**Bug 1 is high priority** — emails are being delivered with fake signatures, which is a trust/integrity issue.

**Bug 2 is medium priority** — the SDK workaround covers the immediate user-facing error, but the dirty data in the DB should be cleaned up to prevent future issues.

**Bug 3 is high priority** — the unsigned `/email/send` endpoint bypasses JACS signing entirely. If it's only for postmaster, it should not be on the public agent API.
