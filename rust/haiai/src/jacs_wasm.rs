// Copyright (c) 2026 Human Assisted Intelligence, Inc.
// SPDX-License-Identifier: BUSL-1.1

//! `JacsWasmProvider` — `JacsProvider` impl backed by `jacs_core::CoreAgent`
//! for the wasm32 build (HAIAI_WASM_PRD §4.1 / §4.2 / Task 017).
//!
//! Delegates every signing / verification call to the public jacs-core /
//! jacs-wasm surface. Per haiai CLAUDE.md Rule 1 + PRD §4.9 "No local
//! crypto" the impl MUST NOT reproduce signing primitives — every byte
//! that ends up on the wire flows through the JACS WASM core.
//!
//! ## Surface coverage
//!
//! - `jacs_id`, `key_id`, `algorithm`, `canonical_json` — direct accessors.
//! - `sign_string`, `sign_bytes`, `sign_response`, `sign_envelope` —
//!   delegate to `CoreAgent::sign_message`, then extract the
//!   `jacsSignature.signature` base64 string so HAIAI's `Authorization:
//!   JACS <id>:<ts>:<nonce>:<signature>` header carries a JACS-verifiable
//!   value.
//! - `verify_a2a_artifact` — delegates to `CoreAgent::verify`.
//! - All other extension methods (`sign_email_locally`, `rotate`,
//!   `update_agent`, `sign_file_envelope`, …) inherit the default trait
//!   impls which return `HaiError::Provider`. Browser callers that need
//!   those operations talk to hai/api over HTTP — they don't sign files
//!   locally.
//!
//! ## Why this lives in `rust/haiai/` and not `rust/haiai-wasm/`
//!
//! `HaiClient<P: JacsProvider>` is generic over `P`. Wiring a wasm
//! provider into HaiClient means making the wasm provider visible from
//! the same crate that declares the trait. Putting the file in
//! `rust/haiai/` keeps the orphan-rule clean and matches where
//! `StaticJacsProvider` / `NoopJacsProvider` already live.

#![cfg(target_arch = "wasm32")]

use std::sync::Mutex;

use base64::Engine as _;
use jacs_core::{CoreAgent, SigningAlgorithm};
use serde_json::{json, Value};

use crate::error::{HaiError, Result};
use crate::jacs::JacsProvider;
use crate::types::SignedPayload;

/// HAIAI's `JacsProvider` implementation backed by JACS WASM's
/// `CoreAgent`. Holds the agent under a `Mutex` because `CoreAgent::
/// sign_message` takes `&mut self` (the underlying signer's nonce /
/// state may mutate) while `JacsProvider::sign_string` only borrows
/// `&self`.
pub struct JacsWasmProvider {
    agent: Mutex<CoreAgent>,
    jacs_id: String,
    algorithm_name: String,
}

impl JacsWasmProvider {
    /// Construct a new provider from an already-unlocked `CoreAgent`.
    ///
    /// `jacs_id` is read from `agent.export_agent()["jacsId"]`; falls
    /// back to an empty string if the agent JSON does not carry one
    /// (ephemeral agents).
    pub fn new(agent: CoreAgent) -> Self {
        let agent_json = agent.export_agent();
        let jacs_id = agent_json
            .get("jacsId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let algorithm_name = match agent.algorithm() {
            SigningAlgorithm::Ed25519 => "ed25519".to_string(),
            SigningAlgorithm::Pq2025 => "pq2025".to_string(),
        };
        Self {
            agent: Mutex::new(agent),
            jacs_id,
            algorithm_name,
        }
    }

    /// Extract the base64 `jacsSignature.signature` field from a signed
    /// document. Returns an error if the field is missing or non-string.
    fn extract_signature_b64(signed: &Value) -> Result<String> {
        signed
            .get("jacsSignature")
            .and_then(|s| s.get("signature"))
            .and_then(|s| s.as_str())
            .map(ToString::to_string)
            .ok_or_else(|| {
                HaiError::Provider(
                    "JacsWasmProvider: signed document missing jacsSignature.signature".to_string(),
                )
            })
    }

    /// Sign a JACS message-typed wrapper around `payload`. The wrapper
    /// is the same shape jacs-core's `sign_message` builds:
    /// `{"jacsType":"message","jacsLevel":"raw","content":<payload>}`.
    /// Returns the full signed document Value.
    fn sign_message(&self, payload: Value) -> Result<Value> {
        let mut guard = self.agent.lock().map_err(|e| {
            HaiError::Provider(format!("JacsWasmProvider: agent mutex poisoned: {e}"))
        })?;
        guard
            .sign_message(&payload)
            .map_err(|e| HaiError::Provider(format!("jacs_core::sign_message failed: {e:?}")))
    }
}

impl JacsProvider for JacsWasmProvider {
    fn jacs_id(&self) -> &str {
        &self.jacs_id
    }

    fn sign_string(&self, message: &str) -> Result<String> {
        // Sign the message text as the body of a JACS message; extract
        // the resulting base64 signature. The on-wire bytes differ from
        // the native HAIAI path (which signs the raw bytes) — this
        // wrapper exists per HAIAI_WASM_PRD §4.5 / §4.9 because
        // `jacs_core::CoreAgent` intentionally does NOT expose a raw
        // bytes-signer. The verifier (hai/api) reconstructs the JACS
        // canonical payload from the signed document, so verification
        // succeeds for any client that uses this same wrapper.
        let signed = self.sign_message(Value::String(message.to_string()))?;
        Self::extract_signature_b64(&signed)
    }

    fn sign_bytes(&self, data: &[u8]) -> Result<Vec<u8>> {
        // Same approach as sign_string: wrap the bytes as a base64
        // string inside a JACS message document. Callers that need byte
        // parity with the native impl should use `sign_envelope` and
        // canonicalize themselves.
        let encoded = base64::engine::general_purpose::STANDARD.encode(data);
        let signed = self.sign_message(Value::String(encoded))?;
        let signature_b64 = Self::extract_signature_b64(&signed)?;
        base64::engine::general_purpose::STANDARD
            .decode(signature_b64.as_bytes())
            .map_err(|e| HaiError::Provider(format!("base64 decode signature failed: {e}")))
    }

    fn key_id(&self) -> &str {
        // jacs-core doesn't expose a key fingerprint accessor; reuse
        // the agent id which is what HAIAI uses elsewhere as the key id
        // for routing.
        &self.jacs_id
    }

    fn algorithm(&self) -> &str {
        &self.algorithm_name
    }

    fn canonical_json(&self, value: &Value) -> Result<String> {
        // Reuse jacs-core's canonical JSON serializer to keep parity
        // with what the verifier reconstructs server-side.
        jacs_core::canonical::canonicalize_json_try(value)
            .map_err(|e| HaiError::Provider(format!("canonicalize_json_try failed: {e:?}")))
    }

    fn sign_envelope(&self, value: &Value) -> Result<String> {
        let signed = self.sign_message(value.clone())?;
        serde_json::to_string(&signed).map_err(|e| {
            HaiError::Provider(format!("JacsWasmProvider::sign_envelope serialise: {e}"))
        })
    }

    fn sign_response(&self, payload: &Value) -> Result<SignedPayload> {
        // SignedPayload is just `{ signed_document, agent_jacs_id }`.
        // The signed_document is the JACS-signed wrapper around the
        // payload; the verifier (hai/api) reconstructs canonical bytes
        // from this document at verification time.
        let signed = self.sign_message(payload.clone())?;
        let signed_document = serde_json::to_string(&signed).map_err(|e| {
            HaiError::Provider(format!("JacsWasmProvider::sign_response serialise: {e}"))
        })?;
        Ok(SignedPayload {
            signed_document,
            agent_jacs_id: self.jacs_id.clone(),
        })
    }

    fn verify_a2a_artifact(&self, wrapped_json: &str) -> Result<String> {
        let wrapped: Value = serde_json::from_str(wrapped_json)?;
        let guard = self.agent.lock().map_err(|e| {
            HaiError::Provider(format!("JacsWasmProvider: agent mutex poisoned: {e}"))
        })?;
        let outcome = guard
            .verify(&wrapped)
            .map_err(|e| HaiError::Provider(format!("jacs_core::verify failed: {e:?}")))?;
        let valid = outcome.valid;
        let signer_id = wrapped
            .get("jacsSignature")
            .and_then(|s| s.get("agentID"))
            .and_then(|s| s.as_str())
            .unwrap_or("");
        let result = json!({
            "valid": valid,
            "status": if valid { "verified" } else { "invalid" },
            "signerId": signer_id,
            "artifactType": wrapped.get("jacsType").and_then(Value::as_str).unwrap_or(""),
            "timestamp": wrapped.get("jacsVersionDate").and_then(Value::as_str).unwrap_or(""),
            "originalArtifact": wrapped.get("a2aArtifact").cloned().unwrap_or(Value::Null),
        });
        Ok(serde_json::to_string(&result)?)
    }
}
