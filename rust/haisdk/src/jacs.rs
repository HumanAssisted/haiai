use std::collections::BTreeMap;

use base64::Engine;
use serde_json::{Map, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::{HaiError, Result};
use crate::types::SignedPayload;

/// Bridge trait for JACS operations that HAI SDK depends on.
///
/// Implement this trait by adapting the canonical JACS Rust package (or a
/// local wrapper around it). HAISDK runtime code should not implement crypto
/// primitives directly.
pub trait JacsProvider: Send + Sync {
    fn jacs_id(&self) -> &str;
    fn sign_string(&self, message: &str) -> Result<String>;

    /// Return canonical JSON text for `value` in the same way JACS signs.
    fn canonical_json(&self, value: &Value) -> Result<String>;

    /// Return a signed payload accepted by `/api/v1/agents/jobs/{job_id}/response`.
    fn sign_response(&self, payload: &Value) -> Result<SignedPayload>;
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

    fn canonical_json(&self, value: &Value) -> Result<String> {
        Ok(canonical_json_sorted(value))
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
}

impl StaticJacsProvider {
    pub fn new(jacs_id: impl Into<String>) -> Self {
        Self {
            jacs_id: jacs_id.into(),
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

    fn canonical_json(&self, value: &Value) -> Result<String> {
        Ok(canonical_json_sorted(value))
    }

    fn sign_response(&self, payload: &Value) -> Result<SignedPayload> {
        let canonical_payload = canonical_json_sorted(payload);
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

pub fn canonical_json_sorted(value: &Value) -> String {
    let canonical = canonicalize_value(value);
    serde_json::to_string(&canonical).unwrap_or_else(|_| "null".to_string())
}

fn canonicalize_value(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sorted: BTreeMap<String, Value> = BTreeMap::new();
            for (key, val) in map {
                sorted.insert(key.clone(), canonicalize_value(val));
            }
            let mut out = Map::new();
            for (key, val) in sorted {
                out.insert(key, val);
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(canonicalize_value).collect()),
        _ => value.clone(),
    }
}
