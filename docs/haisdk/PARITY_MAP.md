# HAIAI SDK Parity Map (JACS 0.9.4)

This document maps every stable JACS 0.9.4 public capability to its SDK exposure status.

**Statuses:**
- **Exposed** -- Directly accessible through an SDK trait method
- **Delegated** -- Accessible via JACS MCP tools or CLI, not a direct Rust trait method
- **Excluded** -- Not exposed, with rationale

---

## Summary

| Category | Exposed | Delegated | Excluded | Total |
|---|---|---|---|---|
| Identity / Signing (Layer 0) | 8 | 0 | 0 | 8 |
| Agent Lifecycle (Layer 1) | 9 | 0 | 0 | 9 |
| Document Operations (Layer 2) | 15 | 0 | 2 | 17 |
| Batch Operations (Layer 3) | 2 | 0 | 0 | 2 |
| Verification (Layer 4) | 6 | 0 | 0 | 6 |
| Email (Layer 5) | 6 | 0 | 0 | 6 |
| Agreements (Layer 6) | 3 | 0 | 3 | 6 |
| Attestation (Layer 7) | 2 | 0 | 3 | 5 |
| Protocol | 2 | 0 | 1 | 3 |
| DNS | 1 | 0 | 1 | 2 |
| A2A Provenance | 1 | 0 | 0 | 1 |
| Storage Internals | 0 | 0 | 4 | 4 |
| Crypto Internals | 0 | 0 | 5 | 5 |
| **Total** | **55** | **0** | **19** | **74** |

---

## Layer 0: Identity / Signing (`JacsProvider`)

| JACS Capability | SDK Method | Status |
|---|---|---|
| `Agent::sign_string()` | `JacsProvider::sign_string()` | Exposed |
| `Agent::sign_bytes()` | `JacsProvider::sign_bytes()` | Exposed |
| `Agent::get_id()` | `JacsProvider::jacs_id()` | Exposed |
| `Agent::get_key_algorithm()` | `JacsProvider::algorithm()` | Exposed |
| `Agent::get_public_key()` | `LocalJacsProvider::public_key_pem()` | Exposed |
| `protocol::canonicalize_json()` | `JacsProvider::canonical_json()` | Exposed |
| `protocol::sign_response()` | `JacsProvider::sign_response()` | Exposed |
| `a2a::provenance::verify_wrapped_artifact()` | `JacsProvider::verify_a2a_artifact()` | Exposed |

## Layer 1: Agent Lifecycle (`JacsAgentLifecycle`)

| JACS Capability | SDK Method | Status |
|---|---|---|
| `simple::advanced::rotate()` | `JacsAgentLifecycle::lifecycle_rotate()` | Exposed |
| `simple::advanced::migrate_agent()` | `JacsAgentLifecycle::lifecycle_migrate()` | Exposed |
| `Agent::update_self()` | `JacsAgentLifecycle::lifecycle_update_agent()` | Exposed |
| `SimpleAgent::export_agent()` | `JacsAgentLifecycle::lifecycle_export_agent_json()` | Exposed |
| `SimpleAgent::diagnostics()` | `JacsAgentLifecycle::diagnostics()` | Exposed |
| `SimpleAgent::verify_self()` | `JacsAgentLifecycle::verify_self()` | Exposed |
| `simple::advanced::quickstart()` | `JacsAgentLifecycle::quickstart()` | Exposed |
| `simple::advanced::reencrypt_key()` | `JacsAgentLifecycle::reencrypt_key()` | Exposed |
| `simple::advanced::get_setup_instructions()` | `JacsAgentLifecycle::get_setup_instructions()` | Exposed |

## Layer 2: Document Operations (`JacsDocumentProvider`)

| JACS Capability | SDK Method | Status |
|---|---|---|
| `DocumentService::create()` | `JacsDocumentProvider::sign_document()` | Exposed |
| `DocumentService::create()` (store) | `JacsDocumentProvider::store_document()` | Exposed |
| Create + store combo | `JacsDocumentProvider::sign_and_store()` | Exposed |
| File signing | `JacsDocumentProvider::sign_file()` | Exposed |
| `DocumentService::get()` | `JacsDocumentProvider::get_document()` | Exposed |
| `DocumentService::list()` | `JacsDocumentProvider::list_documents()` | Exposed |
| `DocumentService::versions()` | `JacsDocumentProvider::get_document_versions()` | Exposed |
| `DocumentService::get_latest()` | `JacsDocumentProvider::get_latest_document()` | Exposed |
| `DocumentService::remove()` | `JacsDocumentProvider::remove_document()` | Exposed |
| `DocumentService::update()` | `JacsDocumentProvider::update_document()` | Exposed |
| `DocumentService::search()` | `JacsDocumentProvider::search_documents()` | Exposed |
| Query by type | `JacsDocumentProvider::query_by_type()` | Exposed |
| Query by field | `JacsDocumentProvider::query_by_field()` | Exposed |
| Capabilities | `JacsDocumentProvider::storage_capabilities()` | Exposed |
| Query by agent | `JacsDocumentProvider::query_by_agent()` | Exposed |
| `DocumentService::diff()` | -- | **Excluded**: Power-user feature for version diffing; use JACS directly for change tracking |
| `DocumentService::set_visibility()` | -- | **Excluded**: Semantics are versioned on routed backends; deferred until PART_2 doc/test alignment |

## Layer 3: Batch Operations (`JacsBatchProvider`)

| JACS Capability | SDK Method | Status |
|---|---|---|
| `simple::batch::sign_messages()` | `JacsBatchProvider::sign_messages()` | Exposed |
| Batch verify | `JacsBatchProvider::verify_batch()` | Exposed |

## Layer 4: Verification (`JacsVerificationProvider`)

| JACS Capability | SDK Method | Status |
|---|---|---|
| Document verification | `JacsVerificationProvider::verify_document()` | Exposed |
| Key-based verification | `JacsVerificationProvider::verify_with_key()` | Exposed |
| Storage-backed verification | `JacsVerificationProvider::verify_by_id()` | Exposed |
| `dns::bootstrap::verify_pubkey_via_dns_or_embedded()` | `JacsVerificationProvider::verify_dns()` | Exposed |
| `protocol::build_auth_header()` | `JacsVerificationProvider::build_auth_header_jacs()` | Exposed |
| `protocol::unwrap_signed_event()` | `JacsVerificationProvider::unwrap_signed_event()` | Exposed |

## Layer 5: Email (`JacsEmailProvider`)

| JACS Capability | SDK Method | Status |
|---|---|---|
| `email::sign_email()` | `JacsEmailProvider::sign_email()` | Exposed |
| `email::verify_email_document()` | `JacsEmailProvider::verify_email()` | Exposed |
| `email::add_jacs_attachment()` | `JacsEmailProvider::add_jacs_attachment()` | Exposed |
| `email::get_jacs_attachment()` | `JacsEmailProvider::get_jacs_attachment()` | Exposed |
| `email::remove_jacs_attachment()` | `JacsEmailProvider::remove_jacs_attachment()` | Exposed |
| `email::extract_email_parts()` | `JacsEmailProvider::extract_email_parts()` | Exposed |

## Layer 6: Agreements (`JacsAgreementProvider`, feature-gated)

| JACS Capability | SDK Method | Status |
|---|---|---|
| `agreements::create()` / `create_with_options()` | `JacsAgreementProvider::create_agreement()` | Exposed |
| `agreements::sign()` | `JacsAgreementProvider::sign_agreement()` | Exposed |
| `agreements::check()` | `JacsAgreementProvider::check_agreement()` | Exposed |
| `Agreement::add_agents_to_agreement()` | -- | **Excluded**: Low-level Agent trait method; covered by create + sign workflow |
| `Agreement::remove_agents_from_agreement()` | -- | **Excluded**: Low-level Agent trait method; covered by create + sign workflow |
| `Agreement::agreement_get_question_and_context()` | -- | **Excluded**: Available in check_agreement return value |

## Layer 7: Attestation (`JacsAttestationProvider`, feature-gated)

| JACS Capability | SDK Method | Status |
|---|---|---|
| `attestation::simple::create()` | `JacsAttestationProvider::create_attestation()` | Exposed |
| `attestation::simple::verify()` | `JacsAttestationProvider::verify_attestation()` | Exposed |
| `attestation::simple::verify_full()` | -- | **Excluded**: Full verification (evidence+chain) deferred; local-only exposed |
| `attestation::simple::lift()` | -- | **Excluded**: Convenience wrapper; consumers can create attestations referencing existing docs directly |
| `attestation::simple::export_dsse()` | -- | **Excluded**: DSSE export is interop-only; not needed for SDK core |

## Protocol

| JACS Capability | SDK Method | Status |
|---|---|---|
| `protocol::build_auth_header()` | `JacsVerificationProvider::build_auth_header_jacs()` | Exposed |
| `protocol::canonicalize_json()` | `JacsProvider::canonical_json()` | Exposed |
| `protocol::extract_document_id()` | `verify::extract_document_id()` (internal) | **Excluded**: Internal helper used by verify module, not a public SDK method |

## DNS

| JACS Capability | SDK Method | Status |
|---|---|---|
| `dns::bootstrap::verify_pubkey_via_dns_or_embedded()` | `JacsVerificationProvider::verify_dns()` | Exposed |
| `dns::bootstrap::resolve_dns_record()` | -- | **Excluded**: Low-level DNS resolution; verify_dns covers the SDK use case |

## A2A Provenance

| JACS Capability | SDK Method | Status |
|---|---|---|
| `a2a::provenance::verify_wrapped_artifact()` | `JacsProvider::verify_a2a_artifact()` | Exposed |

## Storage Internals

| JACS Capability | Status | Rationale |
|---|---|---|
| `StorageType` enum | **Excluded** | Internal routing concern; SDK uses string labels |
| `MultiStorage` struct | **Excluded** | Low-level storage layer; SDK uses DocumentService |
| `StorageDocumentTraits` | **Excluded** | Internal trait; SDK uses DocumentService |
| `VectorSearchTraits` | **Excluded** | Internal trait; exposed via search() on DocumentService |

## Crypto Internals

| JACS Capability | Status | Rationale |
|---|---|---|
| `crypt::KeyManager` trait | **Excluded** | Internal key management; exposed via sign/verify |
| `crypt::aes_encrypt` module | **Excluded** | Low-level encryption; exposed via reencrypt_key |
| `crypt::hash::hash_string()` | **Excluded** | Internal hashing utility |
| `crypt::hash::hash_public_key()` | **Excluded** | Internal hashing utility |
| `crypt::key_from_bytes()` | **Excluded** | Internal key parsing |

---

*Generated for JACS 0.9.4. Last updated: 2026-03-13.*
