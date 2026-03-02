use base64::Engine;
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::{HaiError, Result};
use crate::types::{RotationResult, SignedPayload};

/// Bridge trait for JACS operations that HAI SDK depends on.
///
/// Implement this trait by adapting the canonical JACS Rust package (or a
/// local wrapper around it). HAISDK runtime code should not implement crypto
/// primitives directly.
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

    /// Rotate the agent's keys locally.
    ///
    /// Archives old keys, generates a new keypair, builds a new self-signed
    /// agent document, and updates config on disk. Returns a RotationResult
    /// with old/new versions and the signed agent document.
    ///
    /// Default implementation returns an error; override in providers
    /// that support local key management (e.g., LocalJacsProvider).
    fn rotate(&self) -> Result<RotationResult> {
        Err(HaiError::Provider(
            "key rotation not supported by this provider; use LocalJacsProvider".to_string(),
        ))
    }
}

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

/// Canonical JSON per RFC 8785 (JSON Canonicalization Scheme / JCS).
///
/// Uses the `serde_json_canonicalizer` crate for full compliance including:
/// - Sorted keys
/// - IEEE 754 number serialization
/// - Minimal Unicode escape handling
/// - No unnecessary whitespace
pub fn canonicalize_json_rfc8785(value: &Value) -> String {
    serde_json_canonicalizer::to_string(value).unwrap_or_else(|_| "null".to_string())
}
