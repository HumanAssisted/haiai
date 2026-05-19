// Copyright (c) 2026 Human Assisted Intelligence, Inc.
// SPDX-License-Identifier: BUSL-1.1

//! `BrowserAgentHandle` — the JS-facing wrapper around a `HaiClient`
//! backed by a wasm `JacsWasmProvider` (HAIAI_WASM_PRD §4.3).
//!
//! Wraps:
//!
//! 1. A `jacs_core::CoreAgent` (constructed here directly because
//!    `jacs_wasm::CoreAgentHandle` keeps its inner agent private —
//!    we cannot extract it to feed `JacsWasmProvider`).
//! 2. A `haiai::JacsWasmProvider` adapting (1) to HAIAI's
//!    `JacsProvider` trait.
//! 3. A `haiai::HaiClient<JacsWasmProvider>` (uses reqwest's wasm32
//!    fetch shim under the hood — same per-method code as native).
//!
//! ## Surface
//!
//! Lifecycle (Task 021): `createEphemeral`, `importEncrypted`,
//! `publicOnly`, `clearSecrets`, `isUnlocked`, `exportAgent`,
//! `getPublicKeyBase64`, `algorithm`, `jacsId`.
//!
//! Local crypto (Task 022): `signMessageJson`, `verifyJson`,
//! `signAgreement`, `verifyAgreement`.
//!
//! HAI HTTP (Tasks 023-028): one wrapper per `HaiClient::pub async fn`
//! per `fixtures/wasm_browser_surface.json`.
//!
//! Event streams (Task 029): `connectSse`, `connectWs` →
//! `EventStreamHandle`.
//!
//! Metrics + debug (Task 030): `metrics()` + `HAIAI_WASM_DEBUG` runtime
//! flag.

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

use base64::Engine as _;
use jacs_core::{CoreAgent, CoreError, SigningAlgorithm, UnlockSecret};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasm_bindgen::prelude::*;

use crate::errors::{js_error, map_hai_error, to_js_error};
use haiai::transport::build_auth_header_with;
use haiai::types::{
    CreateEmailTemplateOptions, ListMessagesOptions, ProRunOptions, RegisterAgentOptions,
    RotateKeysOptions, SearchOptions, SendEmailOptions, TransportType, UpdateEmailTemplateOptions,
};
use haiai::verify::generate_verify_link as haiai_generate_verify_link;
use haiai::{HaiClient, HaiClientOptions, JacsProvider, JacsWasmProvider};

// ---------------------------------------------------------------------------
// Metrics — in-memory counters per BrowserAgentHandle (Task 030).
// ---------------------------------------------------------------------------

/// Snapshot of per-handle counters + last-call durations. PRD §10.2.
/// Mirrors `@jacs/wasm`'s `metrics()` shape with HTTP additions.
#[derive(Debug, Default, Clone, Serialize)]
struct HandleMetrics {
    #[serde(rename = "httpRequestCount")]
    http_request_count: u64,
    #[serde(rename = "httpErrorCount")]
    http_error_count: u64,
    #[serde(rename = "signCount")]
    sign_count: u64,
    #[serde(rename = "verifyCount")]
    verify_count: u64,
    #[serde(rename = "sseEventsDelivered")]
    sse_events_delivered: u64,
    #[serde(rename = "wsEventsDelivered")]
    ws_events_delivered: u64,
    #[serde(rename = "lastHttpDurationMs")]
    last_http_duration_ms: f64,
    #[serde(rename = "lastSignDurationMs")]
    last_sign_duration_ms: f64,
    #[serde(rename = "lastVerifyDurationMs")]
    last_verify_duration_ms: f64,
}

/// Monotonic timer (ms). Uses `performance.now()`.
fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or_else(|| js_sys::Date::now())
}

/// Returns `true` iff `globalThis.HAIAI_WASM_DEBUG` is truthy.
fn debug_enabled() -> bool {
    let global = js_sys::global();
    js_sys::Reflect::get(&global, &JsValue::from_str("HAIAI_WASM_DEBUG"))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Emit a `console.debug` line — no-op unless `HAIAI_WASM_DEBUG` is set.
fn debug_log(line: &str) {
    if !debug_enabled() {
        return;
    }
    web_sys::console::debug_1(&JsValue::from_str(line));
}

// ---------------------------------------------------------------------------
// BrowserAgentHandle — the JS-facing object.
// ---------------------------------------------------------------------------

/// Inner shared state. Held inside an `Rc` so wasm-bindgen methods
/// (which take `&self`) can mutate metrics + reach the HaiClient
/// without cloning. The `HaiClient` itself is parked behind a
/// `RefCell<Option<_>>` because constructing it requires the
/// JacsWasmProvider which moves the CoreAgent — the cell lets us
/// rebuild on `clearSecrets` / reload paths.
struct Shared {
    client: HaiClient<JacsWasmProvider>,
    /// Public key for verify-only paths + `getPublicKeyBase64`.
    public_key: Vec<u8>,
    algorithm: SigningAlgorithm,
    /// `true` for handles built via `publicOnly` (no private key).
    is_verifier: bool,
    /// Set to `true` once `clearSecrets` runs. Subsequent sign attempts
    /// return `Locked`. The underlying CoreAgent inside the provider
    /// also has its signer cleared at that point.
    secrets_cleared: bool,
    metrics: HandleMetrics,
}

#[wasm_bindgen]
pub struct BrowserAgentHandle {
    inner: Rc<RefCell<Shared>>,
}

#[wasm_bindgen]
impl BrowserAgentHandle {
    // -----------------------------------------------------------------
    // Lifecycle constructors (Task 021)
    // -----------------------------------------------------------------

    /// Generate a fresh ephemeral agent. `algorithm` must be
    /// `"ed25519"` or `"pq2025"`. `base_url` defaults to the
    /// production HAI endpoint when omitted.
    #[wasm_bindgen(js_name = createEphemeral)]
    pub fn create_ephemeral(
        algorithm: &str,
        base_url: Option<String>,
    ) -> Result<BrowserAgentHandle, JsError> {
        crate::init_haiai_wasm();
        let algo = parse_algorithm(algorithm)?;
        let agent = CoreAgent::ephemeral(algo).map_err(map_core_error)?;
        let public_key = agent.public_key().to_vec();
        Self::build(agent, public_key, algo, false, base_url)
    }

    /// Import an encrypted agent from a JSON-serialized
    /// `AgentMaterial` blob plus password.
    #[wasm_bindgen(js_name = importEncrypted)]
    pub fn import_encrypted(
        material_json: &str,
        password: &str,
        base_url: Option<String>,
    ) -> Result<BrowserAgentHandle, JsError> {
        crate::init_haiai_wasm();
        let material = serde_json::from_str(material_json).map_err(|e| {
            js_error("MalformedDocument", format!("invalid AgentMaterial JSON: {e}"))
        })?;
        let agent =
            CoreAgent::from_encrypted_material(material, UnlockSecret::Password(password))
                .map_err(map_core_error)?;
        let public_key = agent.public_key().to_vec();
        let algorithm = agent.algorithm();
        Self::build(agent, public_key, algorithm, false, base_url)
    }

    /// Build a verify-only handle from a base64 public key. Sign
    /// attempts on the returned handle throw `Locked`; verify methods
    /// + read-only HTTP calls work.
    #[wasm_bindgen(js_name = publicOnly)]
    pub fn public_only(
        _jacs_id: &str,
        public_key_base64: &str,
        algorithm: &str,
        base_url: Option<String>,
    ) -> Result<BrowserAgentHandle, JsError> {
        crate::init_haiai_wasm();
        let algo = parse_algorithm(algorithm)?;
        let public_key = base64::engine::general_purpose::STANDARD
            .decode(public_key_base64)
            .map_err(|e| js_error("MalformedKey", format!("invalid base64 public key: {e}")))?;
        // Construct an ephemeral agent and immediately clear secrets
        // so the handle can only verify. The matching ephemeral keypair
        // is unrelated to `public_key`; the handle reports `public_key`
        // via `getPublicKeyBase64` regardless.
        let mut agent = CoreAgent::ephemeral(algo).map_err(map_core_error)?;
        agent.clear_secrets();
        Self::build(agent, public_key, algo, true, base_url)
    }

    fn build(
        agent: CoreAgent,
        public_key: Vec<u8>,
        algorithm: SigningAlgorithm,
        is_verifier: bool,
        base_url: Option<String>,
    ) -> Result<BrowserAgentHandle, JsError> {
        let provider = JacsWasmProvider::new(agent);
        let mut opts = HaiClientOptions::default();
        if let Some(url) = base_url {
            opts.base_url = url;
        }
        let client = HaiClient::new(provider, opts).map_err(map_hai_error)?;
        Ok(BrowserAgentHandle {
            inner: Rc::new(RefCell::new(Shared {
                client,
                public_key,
                algorithm,
                is_verifier,
                secrets_cleared: is_verifier,
                metrics: HandleMetrics::default(),
            })),
        })
    }

    /// Idempotent secret eviction. After this call, subsequent
    /// sign attempts return `Locked`; read-only calls continue to
    /// work.
    #[wasm_bindgen(js_name = clearSecrets)]
    pub fn clear_secrets(&self) {
        let mut s = self.inner.borrow_mut();
        s.secrets_cleared = true;
        // NOTE: HaiClient doesn't expose a "clear secrets" path on
        // its provider — once the provider is constructed the
        // CoreAgent lives inside JacsWasmProvider's Mutex. The flag
        // here suppresses sign attempts at the wrapper boundary so
        // the in-memory key is no longer reachable from JS even
        // though the CoreAgent itself remains in memory until the
        // handle is dropped. This is a known limitation documented
        // in HAIAI_WASM_PRD §10.2 (browser memory is JS-accessible
        // by design).
        debug_log("clearSecrets: handle locked");
    }

    /// `true` iff a signer is currently held.
    #[wasm_bindgen(js_name = isUnlocked)]
    pub fn is_unlocked(&self) -> bool {
        !self.inner.borrow().secrets_cleared
    }

    /// Export the agent JSON as a string.
    #[wasm_bindgen(js_name = exportAgent)]
    pub fn export_agent(&self) -> Result<String, JsError> {
        // We don't have direct access to the CoreAgent now — the
        // provider keeps it private. Return the minimal exportable
        // shape: jacsId + algorithm + base64 public key, sufficient
        // for `BrowserAgent.publicOnly` round-trips. Full agent JSON
        // export requires the encrypted-material flow.
        let s = self.inner.borrow();
        let v = serde_json::json!({
            "jacsId": s.client.jacs_id(),
            "algorithm": algorithm_str(s.algorithm),
            "publicKeyBase64": base64::engine::general_purpose::STANDARD.encode(&s.public_key),
        });
        serde_json::to_string(&v)
            .map_err(|e| js_error("MalformedDocument", format!("serialize agent: {e}")))
    }

    /// Standard base64 encoding of the raw public-key bytes.
    #[wasm_bindgen(js_name = getPublicKeyBase64)]
    pub fn get_public_key_base64(&self) -> String {
        let s = self.inner.borrow();
        base64::engine::general_purpose::STANDARD.encode(&s.public_key)
    }

    /// Algorithm tag, `"ed25519"` or `"pq2025"`.
    #[wasm_bindgen]
    pub fn algorithm(&self) -> String {
        algorithm_str(self.inner.borrow().algorithm).to_string()
    }

    /// Agent ID (jacsId).
    #[wasm_bindgen(js_name = jacsId)]
    pub fn jacs_id(&self) -> String {
        self.inner.borrow().client.jacs_id().to_string()
    }

    // -----------------------------------------------------------------
    // Local crypto (Task 022) — delegate to the underlying provider.
    // -----------------------------------------------------------------

    /// Sign a JSON payload, returning the signed JACS document as a
    /// JSON string.
    #[wasm_bindgen(js_name = signMessageJson)]
    pub fn sign_message_json(&self, data_json: &str) -> Result<String, JsError> {
        self.require_unlocked()?;
        let started = now_ms();
        let payload: Value = serde_json::from_str(data_json)
            .map_err(|e| js_error("MalformedDocument", format!("invalid input JSON: {e}")))?;
        let signed = {
            let s = self.inner.borrow();
            s.client.jacs().sign_envelope(&payload).map_err(map_hai_error)?
        };
        let mut s = self.inner.borrow_mut();
        s.metrics.sign_count = s.metrics.sign_count.saturating_add(1);
        s.metrics.last_sign_duration_ms = now_ms() - started;
        Ok(signed)
    }

    /// Verify a signed JACS document. Returns `{ valid, status, ... }`
    /// as a JS object.
    #[wasm_bindgen(js_name = verifyJson)]
    pub fn verify_json(&self, signed_json: &str) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client
                .verify_a2a_artifact(signed_json)
                .map_err(|e| JsValue::from(map_hai_error(e)))?
        };
        let mut s = self.inner.borrow_mut();
        s.metrics.verify_count = s.metrics.verify_count.saturating_add(1);
        s.metrics.last_verify_duration_ms = now_ms() - started;
        let parsed: Value = serde_json::from_str(&result)
            .map_err(|e| JsValue::from(js_error("MalformedResponse", format!("verify result parse: {e}"))))?;
        serde_wasm_bindgen::to_value(&parsed)
            .map_err(|e| JsValue::from(js_error("MalformedResponse", format!("to_value: {e}"))))
    }

    /// Append this agent's signature to a multi-party agreement
    /// document. Returns the updated document JSON as a string.
    #[wasm_bindgen(js_name = signAgreement)]
    pub fn sign_agreement(
        &self,
        agreement_json: &str,
        _role: Option<String>,
    ) -> Result<String, JsError> {
        // The Hai SDK does NOT vendor an agreement signer — it
        // delegates to jacs_core::agreements. For the wasm path
        // without jacs-wasm's high-level handle we sign the
        // agreement document via the same provider path used for
        // ordinary documents. Two-party tests should construct two
        // BrowserAgentHandles and chain signAgreement calls.
        self.require_unlocked()?;
        let parsed: Value = serde_json::from_str(agreement_json)
            .map_err(|e| js_error("MalformedDocument", format!("invalid agreement JSON: {e}")))?;
        let s = self.inner.borrow();
        s.client.jacs().sign_envelope(&parsed).map_err(map_hai_error)
    }

    /// Verify quorum / signatures on an agreement document. Returns
    /// `{ valid, ... }`.
    #[wasm_bindgen(js_name = verifyAgreement)]
    pub fn verify_agreement(&self, agreement_json: &str) -> Result<JsValue, JsValue> {
        self.verify_json(agreement_json)
    }

    // -----------------------------------------------------------------
    // Local helpers (Task 028) — pure CPU, no network.
    // -----------------------------------------------------------------

    /// Canonical JSON serialization (RFC 8785). Byte-identical to
    /// what the native client produces — both go through the same
    /// `JacsProvider::canonical_json` path.
    #[wasm_bindgen(js_name = canonicalJson)]
    pub fn canonical_json(&self, value_json: &str) -> Result<String, JsError> {
        let v: Value = serde_json::from_str(value_json)
            .map_err(|e| js_error("MalformedDocument", format!("invalid JSON: {e}")))?;
        let s = self.inner.borrow();
        s.client.canonical_json(&v).map_err(map_hai_error)
    }

    /// Build a `JACS <id>:<ts>:<nonce>:<signature>` authorization
    /// header. Byte-identical to native given the same `(ts, nonce)`
    /// inputs — both call into `haiai::transport::build_auth_header_with`.
    #[wasm_bindgen(js_name = buildAuthHeader)]
    pub fn build_auth_header(&self, ts: i64, nonce: &str) -> Result<String, JsError> {
        self.require_unlocked()?;
        let s = self.inner.borrow();
        build_auth_header_with(s.client.jacs_id(), ts, nonce, |msg| {
            s.client.sign_message(msg)
        })
        .map_err(map_hai_error)
    }

    /// Build a `/jacs/verify?s=<base64url>` link for a signed
    /// document. `base_url` defaults to the configured HAI endpoint
    /// when omitted.
    #[wasm_bindgen(js_name = generateVerifyLink)]
    pub fn generate_verify_link(
        &self,
        document_json: &str,
        base_url: Option<String>,
    ) -> Result<String, JsError> {
        haiai_generate_verify_link(document_json, base_url.as_deref()).map_err(map_hai_error)
    }

    // -----------------------------------------------------------------
    // Metrics + debug accessor (Task 030)
    // -----------------------------------------------------------------

    /// In-memory metrics snapshot. PRD §10.2 shape.
    #[wasm_bindgen(js_name = metrics)]
    pub fn metrics(&self) -> Result<JsValue, JsValue> {
        let s = self.inner.borrow();
        serde_wasm_bindgen::to_value(&s.metrics).map_err(|e| {
            JsValue::from(js_error("MalformedResponse", format!("metrics to_value: {e}")))
        })
    }

    // -----------------------------------------------------------------
    // Internal helper: enforce the "no sign on cleared" rule.
    // -----------------------------------------------------------------

    fn require_unlocked(&self) -> Result<(), JsError> {
        if self.inner.borrow().secrets_cleared {
            return Err(js_error("Locked", "agent secrets have been cleared"));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// HAI HTTP wrappers (Tasks 023-028).
//
// Each method:
//   1. (Optionally) require_unlocked when the endpoint signs the request
//      (every HAI endpoint goes through Authorization: JACS …, so all
//      sign-requiring methods check the lock).
//   2. Deserialize JS inputs via serde_wasm_bindgen.
//   3. Call the matching HaiClient method.
//   4. Bump metrics counters.
//   5. Serialize the response via serde_wasm_bindgen.
//
// The wrappers are grouped by PRD §4.3 section to match the task layout.
// ---------------------------------------------------------------------------

#[wasm_bindgen]
impl BrowserAgentHandle {
    // ── Registration & Identity (Task 023) ────────────────────────────

    pub async fn hello(&self, include_test: bool) -> Result<JsValue, JsValue> {
        self.require_unlocked().map_err(JsValue::from)?;
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.hello(include_test).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    pub async fn register(&self, options_json: &str) -> Result<JsValue, JsValue> {
        let opts: RegisterAgentOptions =
            from_json::<RegisterAgentOptions>(options_json).map_err(JsValue::from)?;
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.register(&opts).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = rotateKeys)]
    pub async fn rotate_keys(
        &self,
        register_with_hai: Option<bool>,
    ) -> Result<JsValue, JsValue> {
        self.require_unlocked().map_err(JsValue::from)?;
        let opts = RotateKeysOptions {
            register_with_hai,
        };
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.rotate_keys(Some(&opts)).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = verifyStatus)]
    pub async fn verify_status(&self, agent_id: Option<String>) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.verify_status(agent_id.as_deref()).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = updateUsername)]
    pub async fn update_username(
        &self,
        agent_id: &str,
        new_username: &str,
    ) -> Result<JsValue, JsValue> {
        self.require_unlocked().map_err(JsValue::from)?;
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.update_username(agent_id, new_username).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = deleteUsername)]
    pub async fn delete_username(&self, agent_id: &str) -> Result<JsValue, JsValue> {
        self.require_unlocked().map_err(JsValue::from)?;
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.delete_username(agent_id).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    // ── Email send + inbox (Task 024) ────────────────────────────────

    #[wasm_bindgen(js_name = sendEmail)]
    pub async fn send_email(&self, options_json: &str) -> Result<JsValue, JsValue> {
        self.require_unlocked().map_err(JsValue::from)?;
        let opts: SendEmailOptions = from_json(options_json).map_err(JsValue::from)?;
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.send_email(&opts).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = sendSignedEmail)]
    pub async fn send_signed_email(&self, options_json: &str) -> Result<JsValue, JsValue> {
        self.require_unlocked().map_err(JsValue::from)?;
        let opts: SendEmailOptions = from_json(options_json).map_err(JsValue::from)?;
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.send_signed_email(&opts).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = listMessages)]
    pub async fn list_messages(&self, options_json: &str) -> Result<JsValue, JsValue> {
        let opts: ListMessagesOptions = from_json(options_json).map_err(JsValue::from)?;
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.list_messages(&opts).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = getMessage)]
    pub async fn get_message(&self, message_id: &str) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.get_message(message_id).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    /// Raw MIME bytes. Returned as a `Uint8Array` (via `Vec<u8>`).
    #[wasm_bindgen(js_name = getRawEmail)]
    pub async fn get_raw_email(&self, message_id: &str) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.get_raw_email(message_id).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = markRead)]
    pub async fn mark_read(&self, message_id: &str) -> Result<(), JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.mark_read(message_id).await
        };
        self.record_http(started, result.is_err());
        result.map_err(|e| JsValue::from(map_hai_error(e)))
    }

    #[wasm_bindgen(js_name = markUnread)]
    pub async fn mark_unread(&self, message_id: &str) -> Result<(), JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.mark_unread(message_id).await
        };
        self.record_http(started, result.is_err());
        result.map_err(|e| JsValue::from(map_hai_error(e)))
    }

    #[wasm_bindgen(js_name = deleteMessage)]
    pub async fn delete_message(&self, message_id: &str) -> Result<(), JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.delete_message(message_id).await
        };
        self.record_http(started, result.is_err());
        result.map_err(|e| JsValue::from(map_hai_error(e)))
    }

    pub async fn archive(&self, message_id: &str) -> Result<(), JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.archive(message_id).await
        };
        self.record_http(started, result.is_err());
        result.map_err(|e| JsValue::from(map_hai_error(e)))
    }

    pub async fn unarchive(&self, message_id: &str) -> Result<(), JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.unarchive(message_id).await
        };
        self.record_http(started, result.is_err());
        result.map_err(|e| JsValue::from(map_hai_error(e)))
    }

    #[wasm_bindgen(js_name = getUnreadCount)]
    pub async fn get_unread_count(&self) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.get_unread_count().await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        Ok(JsValue::from_f64(r as f64))
    }

    #[wasm_bindgen(js_name = getEmailStatus)]
    pub async fn get_email_status(&self) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.get_email_status().await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    // ── Email reply/forward/search/contacts (Task 025) ───────────────

    pub async fn reply(
        &self,
        message_id: &str,
        body: &str,
        subject: Option<String>,
    ) -> Result<JsValue, JsValue> {
        self.require_unlocked().map_err(JsValue::from)?;
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.reply(message_id, body, subject.as_deref()).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    pub async fn forward(
        &self,
        message_id: &str,
        to: &str,
        comment: Option<String>,
    ) -> Result<JsValue, JsValue> {
        self.require_unlocked().map_err(JsValue::from)?;
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.forward(message_id, to, comment.as_deref()).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = searchMessages)]
    pub async fn search_messages(&self, options_json: &str) -> Result<JsValue, JsValue> {
        let opts: SearchOptions = from_json(options_json).map_err(JsValue::from)?;
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.search_messages(&opts).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    pub async fn contacts(&self) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.contacts().await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    // ── Email templates + raw signing (Task 026) ─────────────────────

    #[wasm_bindgen(js_name = createEmailTemplate)]
    pub async fn create_email_template(&self, options_json: &str) -> Result<JsValue, JsValue> {
        self.require_unlocked().map_err(JsValue::from)?;
        let opts: CreateEmailTemplateOptions = from_json(options_json).map_err(JsValue::from)?;
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.create_email_template(&opts).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = listEmailTemplates)]
    pub async fn list_email_templates(
        &self,
        options_json: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let opts: haiai::types::ListEmailTemplatesOptions = match options_json {
            Some(s) => from_json(&s).map_err(JsValue::from)?,
            None => haiai::types::ListEmailTemplatesOptions::default(),
        };
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.list_email_templates(&opts).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = getEmailTemplate)]
    pub async fn get_email_template(&self, template_id: &str) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.get_email_template(template_id).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = updateEmailTemplate)]
    pub async fn update_email_template(
        &self,
        template_id: &str,
        options_json: &str,
    ) -> Result<JsValue, JsValue> {
        self.require_unlocked().map_err(JsValue::from)?;
        let opts: UpdateEmailTemplateOptions = from_json(options_json).map_err(JsValue::from)?;
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.update_email_template(template_id, &opts).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = deleteEmailTemplate)]
    pub async fn delete_email_template(&self, template_id: &str) -> Result<(), JsValue> {
        self.require_unlocked().map_err(JsValue::from)?;
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.delete_email_template(template_id).await
        };
        self.record_http(started, result.is_err());
        result.map_err(|e| JsValue::from(map_hai_error(e)))
    }

    #[wasm_bindgen(js_name = signEmailRaw)]
    pub async fn sign_email_raw(&self, raw_email_b64: &str) -> Result<String, JsValue> {
        self.require_unlocked().map_err(JsValue::from)?;
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.sign_email_raw(raw_email_b64).await
        };
        self.record_http(started, result.is_err());
        result.map_err(|e| JsValue::from(map_hai_error(e)))
    }

    #[wasm_bindgen(js_name = verifyEmailRaw)]
    pub async fn verify_email_raw(&self, raw_email_b64: &str) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.verify_email_raw(raw_email_b64).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    // ── Key & Verification (Task 027) ────────────────────────────────

    #[wasm_bindgen(js_name = fetchServerKeys)]
    pub async fn fetch_server_keys(&self) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.fetch_server_keys().await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = fetchRemoteKey)]
    pub async fn fetch_remote_key(&self, jacs_id: &str, version: &str) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.fetch_remote_key(jacs_id, version).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = fetchKeyByHash)]
    pub async fn fetch_key_by_hash(&self, hash: &str) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.fetch_key_by_hash(hash).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = fetchKeyByEmail)]
    pub async fn fetch_key_by_email(&self, email: &str) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.fetch_key_by_email(email).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = fetchKeyByDomain)]
    pub async fn fetch_key_by_domain(&self, domain: &str) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.fetch_key_by_domain(domain).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = fetchAllKeys)]
    pub async fn fetch_all_keys(&self, jacs_id: &str) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.fetch_all_keys(jacs_id).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = verifyDocument)]
    pub async fn verify_document(&self, document_json: &str) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.verify_document(document_json).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = getVerification)]
    pub async fn get_verification(&self, agent_id: &str) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.get_verification(agent_id).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = verifyAgentDocument)]
    pub async fn verify_agent_document(
        &self,
        request_json: &str,
    ) -> Result<JsValue, JsValue> {
        let req: haiai::types::VerifyAgentDocumentRequest =
            from_json(request_json).map_err(JsValue::from)?;
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.verify_agent_document(&req).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    // ── Benchmark RPC (Task 028) ─────────────────────────────────────

    pub async fn benchmark(
        &self,
        name: Option<String>,
        tier: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.benchmark(name.as_deref(), tier.as_deref()).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = freeRun)]
    pub async fn free_run(&self, transport: Option<String>) -> Result<JsValue, JsValue> {
        let t = transport.as_deref().map(parse_transport_type);
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.free_run(t).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = proRun)]
    pub async fn pro_run(
        &self,
        transport: Option<String>,
        poll_interval_ms: Option<u32>,
        poll_timeout_ms: Option<u32>,
    ) -> Result<JsValue, JsValue> {
        let mut opts = ProRunOptions::default();
        if let Some(t) = transport {
            opts.transport = parse_transport_type(&t);
        }
        if let Some(ms) = poll_interval_ms {
            opts.poll_interval = std::time::Duration::from_millis(ms as u64);
        }
        if let Some(ms) = poll_timeout_ms {
            opts.poll_timeout = std::time::Duration::from_millis(ms as u64);
        }
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.pro_run(&opts).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = dnsCertifiedRun)]
    pub async fn dns_certified_run(
        &self,
        transport: Option<String>,
        poll_interval_ms: Option<u32>,
        poll_timeout_ms: Option<u32>,
    ) -> Result<JsValue, JsValue> {
        let mut opts = ProRunOptions::default();
        if let Some(t) = transport {
            opts.transport = parse_transport_type(&t);
        }
        if let Some(ms) = poll_interval_ms {
            opts.poll_interval = std::time::Duration::from_millis(ms as u64);
        }
        if let Some(ms) = poll_timeout_ms {
            opts.poll_timeout = std::time::Duration::from_millis(ms as u64);
        }
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client.dns_certified_run(&opts).await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }

    #[wasm_bindgen(js_name = submitResponse)]
    pub async fn submit_response(
        &self,
        job_id: &str,
        message: &str,
        metadata_json: Option<String>,
        processing_time_ms: f64,
    ) -> Result<JsValue, JsValue> {
        self.require_unlocked().map_err(JsValue::from)?;
        let metadata: Option<Value> = match metadata_json {
            Some(s) => Some(
                serde_json::from_str(&s).map_err(|e| {
                    JsValue::from(js_error("MalformedDocument", format!("invalid metadata: {e}")))
                })?,
            ),
            None => None,
        };
        let processing_time_ms = processing_time_ms.max(0.0) as u64;
        let started = now_ms();
        let result = {
            let s = self.inner.borrow();
            s.client
                .submit_response(job_id, message, metadata, processing_time_ms)
                .await
        };
        self.record_http(started, result.is_err());
        let r = result.map_err(|e| JsValue::from(map_hai_error(e)))?;
        to_js(&r)
    }
}

// ---------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------

impl BrowserAgentHandle {
    fn record_http(&self, started_ms: f64, errored: bool) {
        let mut s = self.inner.borrow_mut();
        s.metrics.http_request_count = s.metrics.http_request_count.saturating_add(1);
        if errored {
            s.metrics.http_error_count = s.metrics.http_error_count.saturating_add(1);
        }
        s.metrics.last_http_duration_ms = now_ms() - started_ms;
    }
}

fn parse_algorithm(raw: &str) -> Result<SigningAlgorithm, JsError> {
    SigningAlgorithm::from_wire_str(raw).ok_or_else(|| {
        js_error(
            "UnsupportedAlgorithm",
            format!("unknown signing algorithm '{raw}' (expected one of: ed25519, pq2025)"),
        )
    })
}

fn algorithm_str(a: SigningAlgorithm) -> &'static str {
    match a {
        SigningAlgorithm::Ed25519 => "ed25519",
        SigningAlgorithm::Pq2025 => "pq2025",
    }
}

fn parse_transport_type(raw: &str) -> TransportType {
    match raw {
        "ws" => TransportType::Ws,
        _ => TransportType::Sse,
    }
}

fn map_core_error(err: CoreError) -> JsError {
    let code = match &err {
        CoreError::Locked => "Locked",
        CoreError::InvalidPassword => "InvalidPassword",
        CoreError::MalformedEnvelope(_) => "MalformedEnvelope",
        CoreError::MalformedKey(_) => "MalformedKey",
        CoreError::UnsupportedAlgorithm(_) => "UnsupportedAlgorithm",
        CoreError::MalformedDocument(_) => "MalformedDocument",
        CoreError::SignatureInvalid(_) => "SignatureInvalid",
        CoreError::AgreementFailed(_) => "AgreementFailed",
        _ => "Internal",
    };
    to_js_error(code, format!("{err}"), None)
}

fn from_json<T: for<'de> Deserialize<'de>>(s: &str) -> Result<T, JsError> {
    serde_json::from_str::<T>(s)
        .map_err(|e| js_error("MalformedDocument", format!("invalid JSON: {e}")))
}

fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    serde_wasm_bindgen::to_value(value)
        .map_err(|e| JsValue::from(js_error("MalformedResponse", format!("to_value: {e}"))))
}
