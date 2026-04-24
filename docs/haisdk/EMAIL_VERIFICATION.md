# Local JACS Email Verification Recipe

Every HAI email signed by an agent carries a JACS attachment
(`hai.ai.signature.jacs.json`) that binds the RFC 5322 body, headers,
attachments, and signer identity. To verify that bundle offline — after
the recipient has the message in their mailbox — you need two things:

1. The **exact raw MIME bytes** as JACS signed them.
2. The verification function already in each SDK (`verifyEmail` /
   `verify_email` / `VerifyEmail`).

This document shows the two-call pattern in all four languages.

## The Two Calls

```
  fetched = client.getRawEmail(messageId)   // byte-identical to signed
  result  = client.verifyEmail(fetched)     // { valid: true/false, ... }
```

`getRawEmail` returns a `RawEmailResult`:

| Field            | Happy path      | Legacy / Oversize |
|------------------|-----------------|-------------------|
| `available`      | `true`          | `false`           |
| `rawEmail`       | language bytes  | `null` / `nil`    |
| `sizeBytes`      | byte count      | `null` / `nil`    |
| `omittedReason`  | `null`          | `"not_stored"` or `"oversize"` |

Always check `available` before feeding `rawEmail` to `verifyEmail`.

## Node (TypeScript)

```ts
import { HaiClient } from "@haiai/haiai";

const client = await HaiClient.fromConfig();
const raw = await client.getRawEmail("m.uuid");
if (!raw.available) {
  throw new Error(`raw MIME unavailable: ${raw.omittedReason}`);
}
const result = await client.verifyEmail(raw.rawEmail!);
if (!result.valid) throw new Error("tampered or revoked");
console.log("verified", result.jacsId, "tier:", result.reputationTier);
```

## Python

```python
from haiai import HaiClient

client = HaiClient()
raw = client.get_raw_email(message_id="m.uuid")
if not raw.available:
    raise RuntimeError(f"raw MIME unavailable: {raw.omitted_reason}")

result = client.verify_email(raw_email=raw.raw_email)
if not result.valid:
    raise RuntimeError("tampered or revoked")
print("verified", result.jacs_id, "tier:", result.reputation_tier)
```

Async:

```python
from haiai.async_client import AsyncHaiClient

async def verify(mid: str, hai_url: str):
    client = AsyncHaiClient()
    raw = await client.get_raw_email(hai_url, mid)
    if not raw.available:
        return raw.omitted_reason
    result = await client.verify_email(hai_url, raw.raw_email)
    return result
```

## Go

```go
import haiai "github.com/HumanAssisted/haiai-go"

client, err := haiai.NewClient()
if err != nil { return err }

raw, err := client.GetRawEmail(ctx, "m.uuid")
if err != nil { return err }
if !raw.Available {
    return fmt.Errorf("raw MIME unavailable: %s", raw.OmittedReason)
}

result, err := client.VerifyEmail(ctx, raw.RawEmail)
if err != nil { return err }
if !result.Valid { return errors.New("tampered or revoked") }
fmt.Println("verified", result.JacsID, "tier:", result.ReputationTier)
```

## Rust

```rust
use haiai::{HaiClient, HaiClientOptions};

let client = HaiClient::new(provider, HaiClientOptions::default())?;
let raw = client.get_raw_email("m.uuid").await?;
if !raw.available {
    anyhow::bail!("raw MIME unavailable: {:?}", raw.omitted_reason);
}
let bytes = raw.raw_email.expect("present when available=true");
// Local JACS verification via the existing helper:
let result = haiai::email::verify_email(&bytes, &hai_url).await?;
assert!(result.valid, "tampered or revoked");
```

## CLI

```bash
# Write raw bytes to a file
haiai get-raw-email m.uuid --output /tmp/raw.eml

# Or pipe base64
haiai get-raw-email m.uuid --base64 | tee /tmp/raw.b64
```

Exit code 2 when `available: false`, with `omitted_reason` printed on
stderr so scripts can branch.

There is no `haiai verify-email` CLI today. For offline verification
from the shell, pipe the raw bytes into a short Python or Node script
using the language-library `verifyEmail` / `verify_email` calls shown
above.

## MCP

`hai_get_raw_email` is available — two call flow:

1. `hai_get_raw_email {"messageId":"m.uuid"}` →
   `{ raw_email_b64, available, size_bytes, omitted_reason, ... }`.
2. Decode the base64 on the client side and call the Python / Node / Go
   / Rust `verifyEmail` / `verify_email` helper for the actual JACS
   verification.

moltyjacs wraps the first call as `jacs_hai_get_raw_email`. MCP
`hai_verify_email_raw` and moltyjacs `jacs_hai_verify_email_raw` are
*not* registered today; a `verifyEmail` MCP tool would be additive but
has not been scoped.

## Why Byte-Fidelity Matters (PRD R2)

JACS verification hashes the raw bytes that crossed the wire. Any
silent transformation — `\r\n` → `\n`, re-serialization through
`mail-parser`, `String::from_utf8_lossy` on a binary attachment, trimming
trailing whitespace — breaks the signature. The server persists the
exact bytes handed to Stalwart on send, and the exact bytes received
from SMTP on ingress. The endpoint echoes those bytes unchanged. Every
SDK's `getRawEmail` is tested against a fixture that includes CRLF,
embedded NUL, and non-ASCII bytes to catch regressions.

## When Verification Cannot Happen

- `available: false` with `omitted_reason: "not_stored"`: the message
  predates this feature (row inserted before the `raw_mime` column
  existed). Fall back to the server's `jacs_verified` flag on the
  message row; offline verification is not possible for pre-feature
  rows.
- `available: false` with `omitted_reason: "oversize"`: the MIME was
  larger than the 25 MB attachment cap and not persisted. Again, the
  server-side `jacs_verified` flag is the best you have offline.
- `available: true` but `verify_email().valid == false`: the email
  was tampered in transit or the signer's key has been revoked. Do
  not trust this message.

### Server-side coverage

Outbound and inbound raw bytes are both persisted. The filter-worker
write site (`api/email/filter-worker/src/main.rs`) now calls the same
`persist_raw_mime` helper on every DATA hook + SES inbound, passing the
exact bytes received before any `mail-parser` normalization. The
server-side pipeline is guarded by `hai/api/tests/hosted_raw_email_roundtrip_test.rs`,
which asserts byte-identity through `send-signed → DB → /raw` and
covers the legacy-row / oversize / cross-agent branches.

## Smoke-Test the Endpoint (5 Minutes)

Use this when you want to prove raw-email retrieval actually works
end-to-end against a running `hai/api` instance — no mocks, no CI, just
a real server and your terminal. Assumes `HAI_RUN_DB_TESTS=1` CI, a
local docker compose with Postgres, or a staging deployment.

### Prerequisites

- `hai/api` running (locally: `cd hai && docker compose up -d`).
- Two JACS agents registered and activated for email (simplest: reuse
  the `email_ready_agents` fixture from the Python SDK integration
  tests — `cd hai && pytest tests/haisdk/python/test_raw_email_e2e.py`
  will set them up).
- `curl`, `jq`, and `base64` installed.

### Step 1 — Send a signed message

```bash
HAI_API=http://localhost:3000
AGENT_ID=<agent-uuid-A>
JACS_ID=<jacs-id-A>
AUTH_HEADER="JACS $JACS_ID:$(date +%s):<signature-b64>"

RESPONSE=$(curl -sS -X POST \
  "$HAI_API/api/agents/$AGENT_ID/email/send" \
  -H "Authorization: $AUTH_HEADER" \
  -H "Content-Type: application/json" \
  -d '{"to":"b@hai.ai","subject":"smoke probe","body":"hello world","jacs_signature":"<sig>","jacs_timestamp":"<ts>"}')

MESSAGE_ID=$(echo "$RESPONSE" | jq -r .message_id)
echo "sent as: $MESSAGE_ID"
```

If the signature generation is annoying to script by hand, let the
SDK's integration test do it:

```bash
cd hai && pytest tests/haisdk/python/test_raw_email_e2e.py -v -s
# Printed `message_id` values are live and usable in the next step.
```

### Step 2 — Fetch the raw bytes

```bash
ESCAPED=$(python3 -c 'import urllib.parse,sys; print(urllib.parse.quote(sys.argv[1], safe=""))' "$MESSAGE_ID")
RAW=$(curl -sS -H "Authorization: $AUTH_HEADER_RECIPIENT" \
  "$HAI_API/api/agents/$RECIPIENT_AGENT_ID/email/messages/$ESCAPED/raw")
echo "$RAW" | jq '{available, size_bytes, omitted_reason}'
echo "$RAW" | jq -r .raw_email_b64 | base64 -D > /tmp/raw.eml
ls -la /tmp/raw.eml
```

Expected: `{available: true, size_bytes: N, omitted_reason: null}` and
`/tmp/raw.eml` contains the full RFC 5322 message including the
`hai.ai.signature.jacs.json` attachment.

### Step 3 — Verify offline

```bash
python3 <<'PY'
from haiai import HaiClient
raw = open("/tmp/raw.eml", "rb").read()
c = HaiClient()
r = c.verify_email(raw_email=raw)
print("valid:", r.valid, "jacs_id:", r.jacs_id, "tier:", r.reputation_tier)
PY
```

Expected: `valid: True`. Any other result means either (a) the raw bytes
were mutated somewhere along the pipeline (R2 regression — escalate
immediately) or (b) the signer's key is no longer trusted.

### Step 4 — Failure branches

```bash
# legacy row: make-up a mid that never existed
curl -sS -w "\n%{http_code}\n" -H "Authorization: $AUTH_HEADER" \
  "$HAI_API/api/agents/$AGENT_ID/email/messages/%3Cfake%40hai.ai%3E/raw"
# Expected HTTP 404 — NOT 200 + available:false.

# cross-agent: try fetching someone else's message with your auth
curl -sS -w "\n%{http_code}\n" -H "Authorization: $AUTH_HEADER_C" \
  "$HAI_API/api/agents/$AGENT_ID_OF_B/email/messages/$ESCAPED/raw"
# Expected HTTP 401 / 403 / 404 — NEVER 200 with bytes.
```

If any of these four steps deviates from expected, the feature is
broken on that path. See Issue 014 for why this recipe exists.
