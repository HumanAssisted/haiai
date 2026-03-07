# JACS DRY: Moving Shared Protocol Code to JACS

## Problem

Five pieces of JACS protocol logic are duplicated across HAIAI SDK languages (Python, Node, Rust, and partially Go). This code uses JACS signing primitives to produce JACS-formatted artifacts. It belongs in JACS, not in the HAIAI SDK.

| Function | Current locations | Lines duplicated per language |
|----------|-------------------|------------------------------|
| `build_auth_header` | 4 languages | ~10 |
| `sign_response` | 3 (Py, Node, Rust) | ~30-50 |
| `canonicalize_json` | 3 (Py, Node, Rust) | ~5-15 |
| `generate_verify_link` | 3 (Py, Node, Rust) | ~25-60 |
| `unwrap_signed_event` | 2 (Py, Node) | ~60-80 |

## Current Duplication

### 1. Auth Header — 4 implementations

**Rust** `rust/haiai/src/client.rs:150-154`
```rust
let ts = OffsetDateTime::now_utc().unix_timestamp();
let message = format!("{}:{ts}", self.jacs.jacs_id());
let signature = self.jacs.sign_string(&message)?;
Ok(format!("JACS {}:{ts}:{signature}", self.jacs.jacs_id()))
```

**Python** `python/src/jacs/hai/client.py:221-239`
```python
timestamp = int(time.time())
message = f"{cfg.jacs_id}:{timestamp}"
signature = agent.sign_string(message)
return f"JACS {cfg.jacs_id}:{timestamp}:{signature}"
```

**Node** `node/src/client.ts:238-245`
```typescript
const timestamp = Math.floor(Date.now() / 1000).toString();
const message = `${this.jacsId}:${timestamp}`;
const signature = this.agent.signStringSync(message);
return `JACS ${this.jacsId}:${timestamp}:${signature}`;
```

**Go** `go/auth.go:73-86`
```go
timestamp := strconv.FormatInt(time.Now().Unix(), 10)
message := authHeaderMessage(c.jacsID, timestamp)
sigB64, err := c.crypto.SignString(message)
return fmt.Sprintf("JACS %s:%s:%s", jacsID, timestamp, signatureB64)
```

### 2. Sign Response Envelope — 3 implementations

**Rust** `rust/haiai/src/jacs.rs:199-227`
```rust
let doc = serde_json::json!({
    "version": "1.0.0",
    "document_type": "job_response",
    "data": data,
    "metadata": { "issuer": self.jacs_id, "document_id": ..., "created_at": now, "hash": hash },
    "jacsSignature": { "agentID": self.jacs_id, "date": now, "signature": signature },
});
```

**Python** `python/src/jacs/hai/signing.py:339-389`
```python
jacs_doc = {
    "version": "1.0.0",
    "document_type": "job_response",
    "data": sorted_data,
    "metadata": { "issuer": jacs_id, "document_id": doc_id, "created_at": now, "hash": payload_hash },
    "jacsSignature": { "agentID": jacs_id, "date": now, "signature": signature },
}
```

**Node** `node/src/signing.ts:168-206`
```typescript
const jacsDoc = {
    version: '1.0.0',
    document_type: 'job_response',
    data: sortedData,
    metadata: { issuer: jacsId, document_id: documentId, created_at: now, hash },
    jacsSignature: { agentID: jacsId, date: now, signature },
};
```

### 3. Canonical JSON — 3 implementations

- **Rust** `rust/haiai/src/jacs.rs:237-239` — `serde_json_canonicalizer` (RFC 8785)
- **Python** `python/src/jacs/hai/signing.py:37-42` — `json.dumps(obj, sort_keys=True, separators=(",",":"))`
- **Node** `node/src/signing.ts:57-68` — recursive key-sorting `JSON.stringify` replacer

### 4. Verify Link Generation — 3 implementations

Base64url encoding + URL construction + length check + hosted doc ID extraction:
- **Rust** `rust/haiai/src/verify.rs:9-33`
- **Python** `python/src/jacs/hai/client.py:3613-3677`
- **Node** `node/src/verify.ts:41-63`

### 5. Unwrap Signed Events — 2 implementations

Server signature verification with key lookup, canonical JSON, hash fallback:
- **Python** `python/src/jacs/hai/signing.py:270-331`
- **Node** `node/src/signing.ts:80-157`

---

## Proposed JACS Rust API

New module: `jacs::protocol`

```rust
use crate::agent::Agent;
use serde_json::Value;
use std::collections::HashMap;

// -- Auth --

/// Build `Authorization: JACS {jacs_id}:{unix_timestamp}:{signature_b64}`
pub fn build_auth_header(agent: &mut Agent) -> Result<String, JacsError> {
    let jacs_id = agent.get_id()?;
    let ts = now_unix();
    let message = format!("{jacs_id}:{ts}");
    let signature = agent.sign_string(&message)?;
    Ok(format!("JACS {jacs_id}:{ts}:{signature}"))
}

// -- Signing --

/// Create a signed JACS document envelope.
///
/// Returns `{ version, document_type, data, metadata, jacsSignature }`.
pub fn sign_response(agent: &mut Agent, payload: &Value) -> Result<Value, JacsError> {
    let jacs_id = agent.get_id()?;
    let now = now_rfc3339();
    let canonical = canonicalize_json(payload);
    let hash = sha256_hex(canonical.as_bytes());
    let signature = agent.sign_string(&canonical)?;
    let data: Value = serde_json::from_str(&canonical)?;

    Ok(serde_json::json!({
        "version": "1.0.0",
        "document_type": "job_response",
        "data": data,
        "metadata": {
            "issuer": jacs_id,
            "document_id": uuid::Uuid::new_v4().to_string(),
            "created_at": now,
            "hash": hash,
        },
        "jacsSignature": {
            "agentID": jacs_id,
            "date": now,
            "signature": signature,
        },
    }))
}

/// RFC 8785 canonical JSON.
pub fn canonicalize_json(value: &Value) -> String {
    serde_json_canonicalizer::to_string(value)
        .unwrap_or_else(|_| "null".to_string())
}

// -- Verification --

/// Generate a verification URL with base64url-encoded document.
pub fn generate_verify_link(
    document: &str,
    base_url: Option<&str>,
) -> Result<String, JacsError> {
    let base = base_url.unwrap_or("https://hai.ai").trim_end_matches('/');
    let encoded = base64_url_no_pad(document.as_bytes());
    let url = format!("{base}/jacs/verify?s={encoded}");
    if url.len() > MAX_VERIFY_URL_LEN {
        return Err(JacsError::VerifyUrlTooLong);
    }
    Ok(url)
}

/// Generate a hosted verification URL using the document's ID.
pub fn generate_verify_link_hosted(
    document: &str,
    base_url: Option<&str>,
) -> Result<String, JacsError> {
    let base = base_url.unwrap_or("https://hai.ai").trim_end_matches('/');
    let value: Value = serde_json::from_str(document)?;
    let doc_id = value.get("jacsDocumentId")
        .or_else(|| value.get("document_id"))
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .ok_or(JacsError::MissingDocumentId)?;
    Ok(format!("{base}/verify/{doc_id}"))
}

/// Unwrap and verify a JACS-signed event.
///
/// Returns `(inner_data, verified)`.
pub fn unwrap_signed_event(
    agent: &Agent,
    event: &Value,
    server_public_keys: &HashMap<String, String>,
) -> Result<(Value, bool), JacsError> {
    // JacsDocument format
    if let (Some(data), Some(sig)) = (event.get("data"), event.get("jacsSignature")) {
        let agent_id = sig.get("agentID").and_then(Value::as_str).unwrap_or("");
        let signature = sig.get("signature").and_then(Value::as_str).unwrap_or("");

        if let Some(pub_key_pem) = server_public_keys.get(agent_id) {
            let canonical = canonicalize_json(data);
            let valid = agent.verify_string(&canonical, signature, pub_key_pem)?;
            if !valid {
                return Err(JacsError::SignatureVerificationFailed(agent_id.into()));
            }
            return Ok((data.clone(), true));
        }
        return Ok((data.clone(), false));
    }

    // Legacy format: { payload, signature }
    if let Some(payload) = event.get("payload") {
        return Ok((payload.clone(), false));
    }

    Ok((event.clone(), false))
}
```

## How Bindings Expose These

### Python (PyO3 in `jacspy`)

Methods on existing `SimpleAgent` class:

```python
from jacs.client import JacsClient

jacs = JacsClient.quickstart(name="my-agent", ...)

header = jacs.build_auth_header()
doc    = jacs.sign_response(payload)
url    = jacs.generate_verify_link(document)
url    = jacs.generate_verify_link(document, hosted=True)
data, verified = jacs.unwrap_signed_event(event_data, server_keys)
```

### Node (napi-rs in `jacsnpm`)

Methods on existing `JacsClient` class:

```typescript
import { JacsClient } from "@hai.ai/jacs/client";

const jacs = await JacsClient.quickstart({ name: "my-agent", ... });

const header = jacs.buildAuthHeader();
const doc    = jacs.signResponse(payload);
const url    = jacs.generateVerifyLink(document);
const url    = jacs.generateVerifyLink(document, { hosted: true });
const { data, verified } = jacs.unwrapSignedEvent(eventData, serverKeys);
```

### Go (CGo FFI in `jacsgo`)

```go
header := jacs.BuildAuthHeader(agent)
doc    := jacs.SignResponse(agent, payload)
url    := jacs.GenerateVerifyLink(document, "")
data, verified := jacs.UnwrapSignedEvent(agent, event, serverKeys)
```

## What Changes in HAIAI SDK

Each HAIAI SDK client replaces its local plumbing with a single JACS call:

```python
# Before (haiai SDK)
def _build_jacs_auth_header(self) -> str:
    timestamp = int(time.time())
    message = f"{cfg.jacs_id}:{timestamp}"
    signature = agent.sign_string(message)
    return f"JACS {cfg.jacs_id}:{timestamp}:{signature}"

# After (delegates to jacs)
def _build_jacs_auth_header(self) -> str:
    return self._jacs.build_auth_header()
```

### Files to delete from HAIAI SDK after migration

- `python/src/jacs/hai/signing.py` — `canonicalize_json`, `sign_response`, `unwrap_signed_event`
- `node/src/signing.ts` — `canonicalJson`, `signResponse`, `unwrapSignedEvent`
- `node/src/verify.ts` — `generateVerifyLink`
- `rust/haiai/src/verify.rs` — `generate_verify_link`, `generate_verify_link_hosted`
- Auth header construction in all 4 clients

## Implementation Order

1. Add `jacs::protocol` module to the JACS Rust crate
2. Expose via PyO3 in `jacspy` as `SimpleAgent` methods
3. Expose via napi-rs in `jacsnpm` as `JacsClient` methods
4. Expose via CGo in `jacsgo`
5. Update HAIAI SDK Python to call `jacs.build_auth_header()` etc.
6. Update HAIAI SDK Node to call `jacs.buildAuthHeader()` etc.
7. Update HAIAI SDK Go
8. Update HAIAI SDK Rust (calls `jacs::protocol::` directly)
9. Delete duplicated code from all HAIAI SDK clients
10. Bump JACS version, bump HAIAI SDK JACS dependency pin
