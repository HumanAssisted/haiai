use std::collections::HashMap;
use std::path::Path;

use base64::Engine;
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::{HaiError, Result};
use crate::types::{
    DocSearchResults, DocVerificationResult, MigrateAgentResult, RotationResult, SignedDocument,
    SignedPayload, StorageCapabilities, UpdateAgentResult,
};

// =============================================================================
// Layer 0: Core Signing (JacsProvider)
// =============================================================================

/// Bridge trait for JACS operations that HAI SDK depends on.
///
/// Implement this trait by adapting the canonical JACS Rust package (or a
/// local wrapper around it). HAIAI runtime code should not implement crypto
/// primitives directly.
///
/// # 0.2.0 Breaking Change
///
/// `rotate()`, `export_agent_json()`, `update_agent()`, and `sign_email_locally()`
/// are deprecated on this trait and will be removed in a future release.
/// Use [`JacsAgentLifecycle`] for lifecycle operations and [`JacsEmailProvider`]
/// for email operations instead.
pub trait JacsProvider: Send + Sync {
    fn jacs_id(&self) -> &str;
    fn sign_string(&self, message: &str) -> Result<String>;

    /// Sign raw bytes and return the signature bytes.
    /// Required for email signing where the payload is binary.
    fn sign_bytes(&self, data: &[u8]) -> Result<Vec<u8>>;

    /// Return the key identifier used for signing.
    fn key_id(&self) -> &str;

    /// Return the signing algorithm name (e.g., "ed25519", "rsa-pss-sha256").
    fn algorithm(&self) -> &str;

    /// Return canonical JSON text for `value` in the same way JACS signs.
    fn canonical_json(&self, value: &Value) -> Result<String>;

    /// Return a signed payload accepted by `/api/v1/agents/jobs/{job_id}/response`.
    fn sign_response(&self, payload: &Value) -> Result<SignedPayload>;

    /// Verify a wrapped A2A artifact using JACS cryptographic verification.
    ///
    /// Returns a JSON string containing the verification result with fields:
    /// `valid`, `status`, `signerId`, `artifactType`, `timestamp`, `originalArtifact`.
    ///
    /// Default implementation falls back to deterministic signature comparison
    /// (BROKEN for non-deterministic algorithms like pq2025). Providers that
    /// wrap a real JACS agent MUST override this.
    fn verify_a2a_artifact(&self, wrapped_json: &str) -> Result<String> {
        // Fallback: sign the same content and compare signatures.
        // This is WRONG for non-deterministic algorithms but preserved for
        // test providers (StaticJacsProvider) that use deterministic signing.
        let wrapped: Value = serde_json::from_str(wrapped_json)?;
        let signature = wrapped
            .get("jacsSignature")
            .and_then(|s| s.get("signature"))
            .and_then(|s| s.as_str())
            .unwrap_or("");
        let signer_id = wrapped
            .get("jacsSignature")
            .and_then(|s| s.get("agentID"))
            .and_then(|s| s.as_str())
            .unwrap_or("");

        // Strip signature for canonical form
        let mut clone = wrapped.clone();
        if let Some(obj) = clone.as_object_mut() {
            obj.remove("jacsSignature");
        }
        let canonical = self.canonical_json(&clone)?;
        let expected = self.sign_string(&canonical)?;
        let valid = signature == expected;

        let result = serde_json::json!({
            "valid": valid,
            "status": if valid { "verified" } else { "invalid" },
            "signerId": signer_id,
            "artifactType": wrapped.get("jacsType").and_then(|v| v.as_str()).unwrap_or(""),
            "timestamp": wrapped.get("jacsVersionDate").and_then(|v| v.as_str()).unwrap_or(""),
            "originalArtifact": wrapped.get("a2aArtifact").cloned().unwrap_or(Value::Null),
        });
        Ok(serde_json::to_string(&result)?)
    }

    /// Sign a raw RFC 5322 email locally using the agent's own JACS key.
    ///
    /// **Deprecated:** Use [`JacsEmailProvider::sign_email()`] instead.
    /// This method will be removed in a future release.
    fn sign_email_locally(&self, raw_email: &[u8]) -> Result<Vec<u8>> {
        let _ = raw_email;
        Err(HaiError::Provider(
            "local email signing not supported by this provider; use LocalJacsProvider".to_string(),
        ))
    }

    /// Rotate the agent's keys locally.
    ///
    /// **Deprecated:** Use [`JacsAgentLifecycle::rotate()`] instead.
    /// This method will be removed in a future release.
    fn rotate(&self) -> Result<RotationResult> {
        Err(HaiError::Provider(
            "key rotation not supported by this provider; use LocalJacsProvider".to_string(),
        ))
    }

    /// Export the current agent document as a JSON string.
    ///
    /// **Deprecated:** Use [`JacsAgentLifecycle::export_agent_json()`] instead.
    /// This method will be removed in a future release.
    fn export_agent_json(&self) -> Result<String> {
        Err(HaiError::Provider(
            "export_agent_json not supported by this provider; use LocalJacsProvider".to_string(),
        ))
    }

    /// Update agent metadata and re-sign with the existing key.
    ///
    /// **Deprecated:** Use [`JacsAgentLifecycle::update_agent()`] instead.
    /// This method will be removed in a future release.
    fn update_agent(&self, new_agent_data: &str) -> Result<UpdateAgentResult> {
        let _ = new_agent_data;
        Err(HaiError::Provider(
            "update_agent not supported by this provider; use LocalJacsProvider".to_string(),
        ))
    }
}

// =============================================================================
// Layer 1: Agent Lifecycle (JacsAgentLifecycle)
// =============================================================================

/// Extension trait for agent lifecycle operations.
///
/// Provides key rotation, agent migration, metadata update, diagnostics,
/// self-verification, quickstart creation, key re-encryption, and DNS
/// setup instructions.
pub trait JacsAgentLifecycle: JacsProvider {
    /// Rotate the agent's keys. Archives old keys, generates a new keypair,
    /// builds a new self-signed agent document, updates config on disk.
    fn lifecycle_rotate(&self) -> Result<RotationResult>;

    /// Migrate a legacy agent whose document predates a schema change.
    fn lifecycle_migrate(config_path: Option<&Path>) -> Result<MigrateAgentResult>
    where
        Self: Sized;

    /// Update agent metadata and re-sign with the existing key.
    fn lifecycle_update_agent(&self, new_data: &str) -> Result<UpdateAgentResult>;

    /// Export the current agent document as a JSON string.
    fn lifecycle_export_agent_json(&self) -> Result<String>;

    /// Return diagnostic information about the agent as a JSON value.
    fn diagnostics(&self) -> Result<Value>;

    /// Verify the agent's own signature integrity.
    fn verify_self(&self) -> Result<DocVerificationResult>;

    /// Create a new agent with zero-config onboarding.
    fn quickstart(
        name: &str,
        domain: &str,
        description: Option<&str>,
        algorithm: Option<&str>,
        config_path: Option<&str>,
    ) -> Result<Value>
    where
        Self: Sized;

    /// Re-encrypt the agent's private key with a new password.
    fn reencrypt_key(&self, old_password: &str, new_password: &str) -> Result<()>;

    /// Get DNS setup instructions for 5 cloud providers.
    fn get_setup_instructions(&self, domain: &str, ttl: u32) -> Result<Value>;
}

// =============================================================================
// Layer 2: Document Operations (JacsDocumentProvider)
// =============================================================================

/// Extension trait for document storage, retrieval, versioning, and search.
///
/// Wraps the JACS `DocumentService` into SDK-friendly signatures
/// (strings at the boundary, not internal JACS types).
pub trait JacsDocumentProvider: JacsProvider {
    /// Sign a document (returns signed JSON, does NOT store).
    fn sign_document(&self, data: &Value) -> Result<String>;

    /// Store a pre-signed document. Returns the document key (`id:version`).
    fn store_document(&self, signed_json: &str) -> Result<String>;

    /// Convenience: sign + store in one call.
    fn sign_and_store(&self, data: &Value) -> Result<SignedDocument>;

    /// Sign a file (with optional embedding). Returns the signed document.
    fn sign_file(&self, path: &str, embed: bool) -> Result<SignedDocument>;

    /// Get a document by key (`id:version`). Returns signed JSON.
    fn get_document(&self, key: &str) -> Result<String>;

    /// List document keys, optionally filtered by type.
    fn list_documents(&self, jacs_type: Option<&str>) -> Result<Vec<String>>;

    /// Get all versions of a document.
    fn get_document_versions(&self, doc_id: &str) -> Result<Vec<String>>;

    /// Get the latest version of a document.
    fn get_latest_document(&self, doc_id: &str) -> Result<String>;

    /// Remove (tombstone) a document.
    fn remove_document(&self, key: &str) -> Result<()>;

    /// Update a document, creating a new signed version.
    fn update_document(&self, doc_id: &str, data: &str) -> Result<SignedDocument>;

    /// Search documents (fulltext/hybrid depending on backend).
    fn search_documents(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<DocSearchResults>;

    /// Query documents by `jacsType`.
    fn query_by_type(
        &self,
        doc_type: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<String>>;

    /// Query documents by field value.
    fn query_by_field(
        &self,
        field: &str,
        value: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<String>>;

    /// Query documents signed by a specific agent.
    fn query_by_agent(
        &self,
        agent_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<String>>;

    /// Report the capabilities of the configured storage backend.
    fn storage_capabilities(&self) -> Result<StorageCapabilities>;
}

// =============================================================================
// Layer 3: Batch Operations (JacsBatchProvider)
// =============================================================================

/// Extension trait for batch sign/verify operations.
pub trait JacsBatchProvider: JacsProvider {
    /// Sign multiple messages in a single batch operation.
    fn sign_messages(&self, messages: &[&Value]) -> Result<Vec<SignedDocument>>;

    /// Verify multiple documents in a single batch operation.
    fn verify_batch(&self, documents: &[&str]) -> Vec<DocVerificationResult>;
}

// =============================================================================
// Layer 4: Verification (JacsVerificationProvider)
// =============================================================================

/// Extension trait for document verification, DNS trust, and auth headers.
pub trait JacsVerificationProvider: JacsProvider {
    /// Verify a signed document.
    fn verify_document(&self, document: &str) -> Result<DocVerificationResult>;

    /// Verify a document with an explicit public key.
    fn verify_with_key(&self, document: &str, key: Vec<u8>) -> Result<DocVerificationResult>;

    /// Verify a document by storage lookup (requires document service).
    fn verify_by_id(&self, doc_id: &str) -> Result<DocVerificationResult>;

    /// Verify an agent's public key via DNS (L2 trust).
    fn verify_dns(&self, domain: &str) -> Result<()>;

    /// Build a JACS Authorization header for HTTP requests.
    fn build_auth_header_jacs(&self) -> Result<String>;

    /// Unwrap a signed event, verifying its signature against known server public keys.
    ///
    /// Returns a tuple of (unwrapped data, was_verified). If the signer's key is found
    /// in `server_public_keys`, the signature is verified and `was_verified` is `true`.
    /// If the signer's key is not found, the data is returned unverified.
    fn unwrap_signed_event(
        &self,
        event: &Value,
        server_public_keys: &HashMap<String, Vec<u8>>,
    ) -> Result<(Value, bool)>;
}

// =============================================================================
// Layer 5: Email (JacsEmailProvider)
// =============================================================================

/// Extension trait for email signing, verification, and attachment management.
pub trait JacsEmailProvider: JacsProvider {
    /// Sign a raw RFC 5322 email.
    fn sign_email(&self, raw: &[u8]) -> Result<Vec<u8>>;

    /// Verify a signed email with the given public key.
    fn verify_email(&self, raw: &[u8], key: Vec<u8>) -> Result<Value>;

    /// Add a JACS attachment to an email.
    fn add_jacs_attachment(&self, email: &[u8], doc: &[u8]) -> Result<Vec<u8>>;

    /// Extract a JACS attachment from an email.
    fn get_jacs_attachment(&self, email: &[u8]) -> Result<Vec<u8>>;

    /// Remove a JACS attachment from an email.
    fn remove_jacs_attachment(&self, email: &[u8]) -> Result<Vec<u8>>;

    /// Parse an email into structured parts.
    fn extract_email_parts(&self, raw: &[u8]) -> Result<Value>;
}

// =============================================================================
// Layer 6: Agreements (feature-gated)
// =============================================================================

/// Extension trait for multi-party agreements.
#[cfg(feature = "agreements")]
pub trait JacsAgreementProvider: JacsProvider {
    /// Create an agreement with specified agents and optional quorum.
    fn create_agreement(
        &self,
        doc: &str,
        agent_ids: &[String],
        quorum: Option<&str>,
    ) -> Result<SignedDocument>;

    /// Sign an existing agreement.
    fn sign_agreement(&self, document: &str) -> Result<SignedDocument>;

    /// Check the status of an agreement.
    fn check_agreement(&self, document: &str) -> Result<Value>;
}

// =============================================================================
// Layer 7: Attestation (feature-gated)
// =============================================================================

/// Extension trait for verifiable attestation claims.
#[cfg(feature = "attestation")]
pub trait JacsAttestationProvider: JacsProvider {
    /// Create an attestation with subject and claims.
    fn create_attestation(&self, subject: &Value, claims: &[Value]) -> Result<String>;

    /// Verify an attestation document.
    fn verify_attestation(&self, doc_key: &str) -> Result<Value>;
}

// =============================================================================
// Box<dyn JacsProvider> delegation
// =============================================================================

/// Blanket implementation so `HaiClient<Box<dyn JacsProvider>>` works.
///
/// This allows hai-binding-core to erase the concrete provider type behind
/// a trait object while still satisfying the `JacsProvider` bound.
impl JacsProvider for Box<dyn JacsProvider> {
    fn jacs_id(&self) -> &str {
        (**self).jacs_id()
    }

    fn sign_string(&self, message: &str) -> Result<String> {
        (**self).sign_string(message)
    }

    fn sign_bytes(&self, data: &[u8]) -> Result<Vec<u8>> {
        (**self).sign_bytes(data)
    }

    fn key_id(&self) -> &str {
        (**self).key_id()
    }

    fn algorithm(&self) -> &str {
        (**self).algorithm()
    }

    fn canonical_json(&self, value: &Value) -> Result<String> {
        (**self).canonical_json(value)
    }

    fn sign_response(&self, payload: &Value) -> Result<SignedPayload> {
        (**self).sign_response(payload)
    }

    fn verify_a2a_artifact(&self, wrapped_json: &str) -> Result<String> {
        (**self).verify_a2a_artifact(wrapped_json)
    }

    fn sign_email_locally(&self, raw_email: &[u8]) -> Result<Vec<u8>> {
        (**self).sign_email_locally(raw_email)
    }

    fn rotate(&self) -> Result<RotationResult> {
        (**self).rotate()
    }

    fn export_agent_json(&self) -> Result<String> {
        (**self).export_agent_json()
    }

    fn update_agent(&self, new_agent_data: &str) -> Result<UpdateAgentResult> {
        (**self).update_agent(new_agent_data)
    }
}

// =============================================================================
// Providers
// =============================================================================

/// Provider that permits unauthenticated methods only.
#[derive(Debug, Clone)]
pub struct NoopJacsProvider {
    jacs_id: String,
}

impl NoopJacsProvider {
    pub fn new(jacs_id: impl Into<String>) -> Self {
        Self {
            jacs_id: jacs_id.into(),
        }
    }
}

impl JacsProvider for NoopJacsProvider {
    fn jacs_id(&self) -> &str {
        &self.jacs_id
    }

    fn sign_string(&self, _message: &str) -> Result<String> {
        Err(HaiError::Provider(
            "no JACS signer configured; provide a real JacsProvider".to_string(),
        ))
    }

    fn sign_bytes(&self, _data: &[u8]) -> Result<Vec<u8>> {
        Err(HaiError::Provider(
            "no JACS signer configured; provide a real JacsProvider".to_string(),
        ))
    }

    fn key_id(&self) -> &str {
        &self.jacs_id
    }

    fn algorithm(&self) -> &str {
        "none"
    }

    fn canonical_json(&self, value: &Value) -> Result<String> {
        Ok(canonicalize_json_rfc8785(value))
    }

    fn sign_response(&self, _payload: &Value) -> Result<SignedPayload> {
        Err(HaiError::Provider(
            "no JACS response signer configured; provide a real JacsProvider".to_string(),
        ))
    }
}

/// Simple deterministic test provider.
///
/// This exists for contract tests only. Replace in production with a JACS
/// adapter implementation.
#[derive(Debug, Clone)]
pub struct StaticJacsProvider {
    jacs_id: String,
    algorithm: String,
}

impl StaticJacsProvider {
    pub fn new(jacs_id: impl Into<String>) -> Self {
        Self {
            jacs_id: jacs_id.into(),
            algorithm: "ed25519".to_string(),
        }
    }

    /// Create a StaticJacsProvider with a specific algorithm for testing
    /// multi-algorithm behavior.
    pub fn with_algorithm(jacs_id: impl Into<String>, algorithm: impl Into<String>) -> Self {
        Self {
            jacs_id: jacs_id.into(),
            algorithm: algorithm.into(),
        }
    }
}

impl JacsProvider for StaticJacsProvider {
    fn jacs_id(&self) -> &str {
        &self.jacs_id
    }

    fn sign_email_locally(&self, raw_email: &[u8]) -> Result<Vec<u8>> {
        // Test-only: return the raw email as-is (no actual JACS attachment).
        // This is sufficient for integration tests that only verify the HTTP
        // flow, not the cryptographic content of the signature.
        Ok(raw_email.to_vec())
    }

    fn sign_string(&self, message: &str) -> Result<String> {
        let raw = format!("sig:{}", message);
        Ok(base64::engine::general_purpose::STANDARD.encode(raw))
    }

    fn sign_bytes(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut result = b"sig:".to_vec();
        result.extend_from_slice(data);
        Ok(result)
    }

    fn key_id(&self) -> &str {
        &self.jacs_id
    }

    fn algorithm(&self) -> &str {
        &self.algorithm
    }

    fn canonical_json(&self, value: &Value) -> Result<String> {
        Ok(canonicalize_json_rfc8785(value))
    }

    fn sign_response(&self, payload: &Value) -> Result<SignedPayload> {
        let canonical_payload = canonicalize_json_rfc8785(payload);
        let data = serde_json::from_str::<Value>(&canonical_payload)?;
        let now = OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|e| HaiError::Provider(format!("failed to format timestamp: {e}")))?;

        let doc = serde_json::json!({
            "version": "1.0.0",
            "document_type": "job_response",
            "data": data,
            "metadata": {
                "issuer": self.jacs_id,
                "document_id": Uuid::new_v4().to_string(),
                "created_at": now,
                "hash": "",
            },
            "jacsSignature": {
                "agentID": self.jacs_id,
                "date": now,
                "signature": self.sign_string(&canonical_payload)?,
            },
        });

        Ok(SignedPayload {
            signed_document: serde_json::to_string(&doc)?,
            agent_jacs_id: self.jacs_id.clone(),
        })
    }
}

// =============================================================================
// Canonical JSON
// =============================================================================

/// Canonical JSON per RFC 8785 (JSON Canonicalization Scheme / JCS).
///
/// When the `jacs-crate` feature is enabled, delegates to `jacs::protocol::canonicalize_json`.
#[cfg(feature = "jacs-crate")]
pub fn canonicalize_json_rfc8785(value: &Value) -> String {
    jacs::protocol::canonicalize_json(value)
}

/// Canonical JSON per RFC 8785 (JSON Canonicalization Scheme / JCS).
///
/// Local fallback when `jacs-crate` feature is not enabled.
/// Requires the `serde_json_canonicalizer` feature.
#[cfg(all(not(feature = "jacs-crate"), feature = "serde_json_canonicalizer"))]
pub fn canonicalize_json_rfc8785(value: &Value) -> String {
    serde_json_canonicalizer::to_string(value).unwrap_or_else(|_| "null".to_string())
}

#[cfg(all(not(feature = "jacs-crate"), not(feature = "serde_json_canonicalizer")))]
compile_error!(
    "Either `jacs-crate` or `serde_json_canonicalizer` feature must be enabled for JSON canonicalization"
);
