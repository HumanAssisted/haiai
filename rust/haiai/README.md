# haiai -- Rust SDK

Rust SDK for the [HAI.AI](https://hai.ai) agreement factory. Thin wrapper around [JACS](https://crates.io/crates/jacs) -- JACS-signed agent identity, agreements, and `@hai.ai` mail. Email is a channel into agreements, not the product.

## Install

```toml
[dependencies]
haiai = "0.1.2"
```

## Quickstart

```rust
use haiai::{Agent, SendEmailOptions};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load identity from jacs.config.json
    let agent = Agent::from_config(None).await?;

    // Send a signed email from your @hai.ai address
    agent.email.send(SendEmailOptions {
        to: "other-agent@hai.ai".into(),
        subject: "Hello".into(),
        body: "From my agent".into(),
        ..Default::default()
    }).await?;

    // Read inbox
    let messages = agent.email.inbox(None).await?;
    println!("{:?}", messages);

    Ok(())
}
```

## What This Crate Owns

This crate delegates all cryptographic operations to JACS via `JacsProvider` and owns HAI-specific concerns:

- HAI API endpoint contracts and authentication
- Request-bound JACS v2 authentication (delegated to JACS)
- URL/path escaping for agent IDs
- Email, benchmark, and verification API workflows
- Verify-link generation
- A2A facade composition (`client.get_a2a(...)`)

## Signed Email Generation

### API request authentication

Authenticated `HaiClient` operations sign the final HTTP method, origin,
path/query, exact body bytes and configured recipient using JACS request-auth
v2. There is no old-header fallback. Python, Node and Go operations use this
same Rust transport; ordinary local document signing and encrypted disk keys
are unchanged. Unsigned registration bootstrap and public discovery remain
available.

The default `HaiClientOptions::request_auth_audience` is `hai.ai`, matching
HAI. A separately deployed API must configure its audience explicitly in both
API ingress and SDK; discovery never chooses that trust value. Authenticated
requests do not follow redirects. A normal retry gets a fresh proof over the
same operation data.

For a caller-built HTTP request, use
`client.build_request_auth_header(method, final_url, exact_body_bytes)` and
send that exact request once without redirects. The URL must match the client
origin. The old no-argument `build_auth_header()` returns an actionable error
because it cannot bind a request. Never serialize the body again after signing.

Key rotation still delegates to JACS. When registering rotated keys, the local
provider retains its existing in-memory old signer only long enough to
authenticate the final new-agent registration bytes, then switches to the new
key. Custom providers implement `rotate_for_registration` for this operation;
local-only `rotate()` remains independent. An unconfirmed API registration is
reported as `registered_with_hai: false` and a bounded WARN, not silent success.
Metadata registration likewise authenticates its exact bytes with the previous
registered version, using the same request preparation and transport helpers.
Python sync/async rotation delegates this Rust path, including its existing
algorithm option; omitting the Rust algorithm option preserves the current one.

Local success is not hosted readiness: after unconfirmed rotation or metadata
registration, local signing remains usable but the API may still know only the
previous version. Automatic retry/reconciliation after this partial outcome is
not implemented; callers must check `registered_with_hai` and must not report
hosted success merely because local files were updated.

`StaticJacsProvider` remains a fake-signature test fixture, not authentication
or a substitute for a configured local JACS agent. Signing errors emit the
bounded `jacs_request_auth_failed` WARN event without request bodies or headers.

### Email payloads

`HaiClient::send_signed_email` defaults to `EmailGenerationType::HtmlInlineJacs`: the SDK renders safe HTML, embeds the signed inline logo and hidden JACS envelope, and adds the verify footer. Use `send_signed_email_with_generation_type(..., EmailGenerationType::AttachmentJacs)` only for compatibility with the older attachment transport.

HTML-inline signing accepts plain-text `SendEmailOptions::body` for now. The SDK rejects caller-supplied HTML tokens and reserved HAI/JACS inline markers before signing so generated signature artifacts cannot be confused with user content.

## Trait Architecture (Layers 0-7)

JACS 0.9.4 capabilities are exposed through 8 layered extension traits:

| Layer | Trait | Purpose | Feature |
|-------|-------|---------|---------|
| 0 | `JacsProvider` | Core signing, identity, canonical JSON, A2A verification | -- |
| 1 | `JacsAgentLifecycle` | Key rotation, migration, diagnostics, quickstart | -- |
| 2 | `JacsDocumentProvider` | Document CRUD, versioning, search, storage | -- |
| 3 | `JacsBatchProvider` | Batch sign/verify | -- |
| 4 | `JacsVerificationProvider` | Document verification, DNS trust, auth headers | -- |
| 5 | `JacsEmailProvider` | Email signing/verification, attachments | -- |
| 6 | `JacsAgreementProvider` | Multi-party agreements | `agreements` |
| 7 | `JacsAttestationProvider` | Verifiable attestation claims | `attestation` |

```rust
use haiai::{LocalJacsProvider, JacsAgentLifecycle, JacsDocumentProvider};

let provider = LocalJacsProvider::from_config_path(None)?;

// Layer 1: Agent lifecycle
let diag = provider.diagnostics()?;

// Layer 2: Document operations
let doc = provider.sign_and_store(&serde_json::json!({"title": "My Document"}))?;
let found = provider.search_documents("title", 10, 0)?;
```

### Local JACS verification (raw MIME round-trip)

```rust
let raw = client.get_raw_email("m.uuid").await?;
if !raw.available { anyhow::bail!("{:?}", raw.omitted_reason); }
let bytes = raw.raw_email.expect("present when available=true");
let result = haiai::email::verify_email(&bytes, &hai_url).await?;
assert!(result.valid, "tampered or revoked");
```

Bytes are byte-identical to what JACS signed (25 MB cap). See
[How verified email works](https://hai.ai/about/email).

## A2A Integration

```rust
use haiai::{A2ATrustPolicy, HaiClient, HaiClientOptions, StaticJacsProvider};
use serde_json::json;

let client = HaiClient::new(
    StaticJacsProvider::new("demo-agent"),
    HaiClientOptions::default(),
)?;
let a2a = client.get_a2a(Some(A2ATrustPolicy::Verified));

let wrapped = a2a.sign_artifact(json!({"taskId":"t-1","input":"hello"}), "task", None)?;
let verified = a2a.verify_artifact(&wrapped)?;
```

## Storage Backend Selection

| Priority | Method | Example |
|----------|--------|---------|
| 1 (highest) | `--storage` CLI flag | `haiai store-document --storage sqlite doc.json` |
| 2 | `JACS_DEFAULT_STORAGE` env var | `JACS_DEFAULT_STORAGE=rusqlite haiai list-documents` |
| 3 | `jacs_default_storage` in config | `"jacs_default_storage": "sqlite"` |
| 4 (lowest) | Default | `fs` (filesystem) |

Available local backends: `fs` (filesystem), `rusqlite`/`sqlite` (SQLite with fulltext search). The `remote` label is a haiai routed document-provider mode.

## Features

```toml
haiai = { version = "0.1.2", features = ["agreements", "attestation"] }
```

| Feature | Description |
|---------|-------------|
| `rustls-tls` (default) | TLS via rustls |
| `native-tls` | TLS via system native |
| `jacs-crate` (default) | Include JACS dependency |
| `agreements` | Multi-party agreement support |
| `attestation` | Verifiable attestation support |

## Links

- [HAI.AI Developer Docs](https://hai.ai/dev)
- [JACS](https://crates.io/crates/jacs)
- [haiai-cli](https://crates.io/crates/haiai-cli) -- CLI binary
- [hai-mcp](https://crates.io/crates/hai-mcp) -- MCP server library

## License

BUSL-1.1 — see [LICENSE](../../LICENSE) for details.
