# ADR 0001: Crypto Delegation To JACS

## Status
Accepted

## Context
`haiai` exists to provide HAI platform integration on top of JACS identity and document primitives.

Over time, this repository accumulated local cryptographic implementations in multiple languages. That increases drift risk and makes cross-language verification behavior harder to reason about.

## Decision
All runtime cryptographic operations in `haiai` must delegate to JACS functions.

This includes:

1. signature creation
2. signature verification
3. key generation
4. key encryption/decryption
5. canonicalization used for signing workflows

Local crypto logic may remain only as transitional compatibility code until each SDK is fully JACS-backed.

## Consequences

1. New direct primitive crypto calls are disallowed in `haiai` runtime code unless explicitly approved as a temporary JACS-gap exception.
2. CI enforces a denylist policy for direct primitive usage outside explicitly-allowed transitional files.
3. Agent bootstrap flows should move toward encrypted private key defaults using JACS key/encryption support.
4. Existing local crypto helpers are expected to be deprecated and removed after migration.
