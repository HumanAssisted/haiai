# haisdk (Rust)

Rust SDK for the HAI.AI platform.

This crate intentionally delegates cryptographic operations to a caller-provided
JACS integration via `JacsProvider`. It owns HAI-specific concerns such as:

- endpoint contracts
- JACS auth header shape
- URL/path escaping rules
- benchmark/email/verification API workflows
- verify-link generation

## Status

Initial scaffold for parity with existing Python/Node/Go SDK contracts.

## Crypto policy

Do not implement runtime crypto primitives in this crate. Use `JacsProvider`
implementations backed by JACS.
