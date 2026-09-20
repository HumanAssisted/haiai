# ADR 0001: Crypto Delegation To JACS

## Status
Accepted

## Context
`haiai` exists to provide HAI platform integration on top of JACS identity and document primitives.

Over time, this repository accumulated local cryptographic implementations in multiple languages. That increases drift risk and makes cross-language verification behavior harder to reason about. The original migration allowed transitional helpers and temporary exceptions; those describe migration history, not the current ownership rule.

## Decision
All runtime cryptographic operations in `haiai` must delegate to JACS functions.

Here, "delegation" means implementation forwarding. Python, Node, and Go send
HAI operations through FFI to the shared Rust client, whose JACS provider owns
signing. It does not mean principal-to-agent delegation of authority, and an
agent signature does not establish a person's approval. Principal delegation
remains unsupported and unscheduled. JACS's native-root authorization of an
ES256 export key is a separate implemented key-binding mechanism.

This includes:

1. signature creation
2. signature verification
3. key generation
4. key encryption/decryption
5. canonicalization used for signing workflows

Missing JACS binding support must fail with an actionable dependency error.
Promote a needed non-public JACS function upstream or expose a thin JACS wrapper;
do not add a local cryptographic fallback in HAIAI. The same ownership rule
applies to tests: use JACS's public functions for cryptographic assertions.

Authenticated API requests use JACS request-auth v2 over the final method, URL,
exact body bytes and pinned audience. Context-free credentials are rejected;
public discovery and bootstrap registration remain unsigned. See the
[language sync guide](../HAIAI_LANGUAGE_SYNC_GUIDE.md#authentication-header-format).

## Consequences

1. New direct primitive crypto calls and fallback implementations are disallowed in HAIAI source and tests.
2. CI's denylist catches prohibited primitive use. Historical exclusions are enforcement debt, not permission to introduce local crypto or proof of runtime parity.
3. Agent bootstrap and key storage use JACS key-generation and encryption support.
4. Remaining A2A/DNS adapter drift must be corrected through canonical JACS APIs and shared verification contracts; this rule does not claim discovery parity is already complete.
