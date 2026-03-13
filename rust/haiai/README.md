# HAIAI SDK (Rust)

Rust SDK for the HAI.AI platform.

This crate intentionally delegates cryptographic operations to a caller-provided
JACS integration via `JacsProvider`. It owns HAI-specific concerns such as:

- endpoint contracts
- JACS auth header shape
- URL/path escaping rules
- benchmark/email/verification API workflows
- verify-link generation
- A2A facade composition (`client.get_a2a(...)`) on top of JACS-backed signing

## Trait Architecture (Layers 0-7)

JACS 0.9.4 capabilities are exposed through 8 layered extension traits, all
defined in `src/jacs.rs` and implemented on `LocalJacsProvider` in `src/jacs_local.rs`:

| Layer | Trait | Purpose | Feature |
|-------|-------|---------|---------|
| 0 | `JacsProvider` | Core signing, identity, canonical JSON, A2A verification | -- |
| 1 | `JacsAgentLifecycle` | Key rotation, migration, diagnostics, quickstart | -- |
| 2 | `JacsDocumentProvider` | Document CRUD, versioning, search, storage capabilities | -- |
| 3 | `JacsBatchProvider` | Batch sign/verify | -- |
| 4 | `JacsVerificationProvider` | Document verification, DNS trust, auth headers | -- |
| 5 | `JacsEmailProvider` | Email signing/verification, attachments | -- |
| 6 | `JacsAgreementProvider` | Multi-party agreements | `agreements` |
| 7 | `JacsAttestationProvider` | Verifiable attestation claims | `attestation` |

## Storage Backend Selection

Storage backends are selected by label via `config.rs`:

- `resolve_storage_backend_label(label)` -- validates `fs`, `rusqlite`, `sqlite`
- `resolve_storage_backend(explicit, config_path)` -- priority: flag > env > config > default

## Crypto policy

Do not implement runtime crypto primitives in this crate. Use `JacsProvider`
implementations backed by JACS.

## JACS dependency source

Default builds use the published `jacs` crate pinned to `0.9.4`.

For local development against a sibling checkout at `../../JACS`, this repo
uses a local cargo override in `rust/.cargo/config.toml`:

```bash
[patch.crates-io]
jacs = { path = "../../JACS/jacs" }
jacs-binding-core = { path = "../../JACS/binding-core" }
jacs-mcp = { path = "../../JACS/jacs-mcp" }
```

That file is gitignored, so CI and published builds still use crates.io.

## A2A facade

The HAIAI SDK exposes first-class A2A wrappers that stay at the SDK layer while
delegating cryptographic signing/canonicalization to your `JacsProvider`.

Key entrypoint:

1. `HaiClient::get_a2a(...)`
