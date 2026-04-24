# haiai -- Rust SDK

Rust SDK for the [HAI.AI](https://hai.ai) platform. Thin wrapper around [JACS](https://crates.io/crates/jacs) -- build helpful, trustworthy AI agents with cryptographic identity, signed email, and verified benchmarks.

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
- JACS auth header construction (`JACS {jacsId}:{timestamp}:{signature_base64}`)
- URL/path escaping for agent IDs
- Email, benchmark, and verification API workflows
- Verify-link generation
- A2A facade composition (`client.get_a2a(...)`)

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
[`docs/haisdk/EMAIL_VERIFICATION.md`](../../docs/haisdk/EMAIL_VERIFICATION.md).

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
| 2 | `JACS_STORAGE` env var | `JACS_STORAGE=rusqlite haiai list-documents` |
| 3 | `default_storage` in config | `"defaultStorage": "sqlite"` |
| 4 (lowest) | Default | `fs` (filesystem) |

Available backends: `fs` (filesystem), `rusqlite`/`sqlite` (SQLite with fulltext search).

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

Apache-2.0 OR MIT
