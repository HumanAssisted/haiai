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

## Crypto policy

Do not implement runtime crypto primitives in this crate. Use `JacsProvider`
implementations backed by JACS.

## JACS dependency source

Default builds use the published `jacs` crate pinned to `0.9.3`.

For local development against a checkout at `../../../JACS/jacs`, disable
default features and enable `jacs-local`:

```bash
cargo test -p haiai --no-default-features --features rustls-tls,jacs-local
```

## A2A facade

The HAIAI SDK exposes first-class A2A wrappers that stay at the SDK layer while
delegating cryptographic signing/canonicalization to your `JacsProvider`.

Key entrypoint:

1. `HaiClient::get_a2a(...)`
