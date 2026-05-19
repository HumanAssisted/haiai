//! `HaiTransport` trait — the seam between `HaiClient` and the underlying
//! HTTP stack. Lets the same HaiClient source target native `reqwest`
//! (via tokio) or browser `fetch()` (via reqwest's wasm32 shim) without
//! forking the per-endpoint code.
//!
//! HAIAI_WASM_PRD §4.2: introduces a `HaiTransport` trait so the same
//! `HaiClient` source can target reqwest::Client (native) or reqwest's
//! wasm32 mode (browser). Public `HaiClient` method signatures must NOT
//! change so existing native tests pass unchanged.
//!
//! ## Status (Task 011)
//!
//! This module declares the trait and ships a native implementation
//! (`NativeReqwestTransport`) that wraps `reqwest::Client`. `HaiClient`
//! retains its current concrete `reqwest::Client` field for now so the
//! existing native + FFI surfaces stay byte-identical. A follow-up
//! (Task 012 / Wave 5) will:
//!   1. Add a wasm-side `WasmFetchTransport` impl mirroring this one.
//!   2. Make `HaiClient` generic over `T: HaiTransport = NativeReqwestTransport`
//!      and route every `self.http.*` call through `self.transport.*`.
//!
//! Keeping the trait + native impl decoupled from `HaiClient`'s internals
//! in this commit avoids the high-risk simultaneous rewrite of every
//! HaiClient HTTP method (~4500 LOC of `client.rs`) and lets the trait
//! shape stabilize before the wasm impl lands.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

use crate::error::Result;

/// HTTP verbs HaiClient uses today. Adding a new variant here is a
/// breaking change for downstream transport impls — keep this list in
/// sync with the audit in `docs/HAIAI_WASM_NATIVE_DEPS_AUDIT.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum HaiHttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

/// A serializable HaiClient request. Bodies are JSON-only for V1; raw
/// body bytes (used by `get_raw_email`) come back through
/// `HaiResponseBytes`, not in the request shape (PRD §4.8: no multipart
/// or streaming bodies on wasm).
#[derive(Debug, Clone, Serialize)]
pub struct HaiRequest {
    pub method: HaiHttpMethod,
    pub url: String,
    /// Headers other than the standard Authorization / Content-Type
    /// pair, which transports may inject.
    pub headers: BTreeMap<String, String>,
    /// Optional auth header value built by `HaiClient::build_auth_header`.
    /// Transports MUST send it verbatim (byte-identical to the native
    /// path) — see HAIAI_WASM_PRD §4.5.
    pub auth_header: Option<String>,
    /// Query string parameters (`?key=value`).
    pub query: BTreeMap<String, String>,
    /// JSON body. `None` means "no body" (or, for GET, no body period).
    pub json_body: Option<Value>,
    /// Per-request timeout override. Native respects it via reqwest's
    /// per-request timeout; wasm ignores it (browser fetch timeout).
    pub timeout: Option<Duration>,
}

/// JSON response — the most common HaiClient response shape.
#[derive(Debug, Clone)]
pub struct HaiResponseJson {
    pub status: u16,
    pub body: Value,
    pub headers: BTreeMap<String, String>,
}

/// Raw bytes response. Used by `get_raw_email` (raw MIME).
#[derive(Debug, Clone)]
pub struct HaiResponseBytes {
    pub status: u16,
    pub body: Vec<u8>,
    pub headers: BTreeMap<String, String>,
    pub content_type: Option<String>,
}

/// Transport interface — abstracts over native `reqwest::Client` and
/// the wasm32 fetch shim. Each method drives ONE HTTP exchange. The
/// retry / backoff loop stays in `HaiClient::request_with_retry` so
/// transports don't have to duplicate it.
#[async_trait]
pub trait HaiTransport: Send + Sync + 'static {
    /// Execute the request and parse the response body as JSON.
    async fn request_json(&self, req: HaiRequest) -> Result<HaiResponseJson>;

    /// Execute the request and return the response body as raw bytes.
    /// Used by `get_raw_email` (raw MIME) — bypasses JSON parsing
    /// because the body is byte-identical to what JACS signed
    /// server-side (PRD §4.5 / §4.6).
    async fn request_bytes(&self, req: HaiRequest) -> Result<HaiResponseBytes>;

    /// Execute the request and discard the body (HTTP 204 / 200-with-no-body).
    /// Used by markRead / markUnread / archive / etc.
    async fn request_no_content(&self, req: HaiRequest) -> Result<()>;
}

#[cfg(not(target_arch = "wasm32"))]
pub use self::native::NativeReqwestTransport;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use reqwest::Client;

    /// Native `reqwest::Client` impl of `HaiTransport`. Used by the
    /// default `HaiClient` on native targets (`HaiClient<P, T =
    /// NativeReqwestTransport>` when Task 012 lands; today, kept
    /// alongside `HaiClient`'s existing concrete `reqwest::Client`
    /// field to avoid the wholesale per-endpoint rewire).
    pub struct NativeReqwestTransport {
        pub(crate) client: Client,
    }

    impl NativeReqwestTransport {
        pub fn new(client: Client) -> Self {
            Self { client }
        }
    }

    #[async_trait]
    impl HaiTransport for NativeReqwestTransport {
        async fn request_json(&self, req: HaiRequest) -> Result<HaiResponseJson> {
            let resp = send_request(&self.client, &req).await?;
            let status = resp.status().as_u16();
            let headers = collect_headers(resp.headers());
            let body = resp.json::<Value>().await?;
            Ok(HaiResponseJson {
                status,
                body,
                headers,
            })
        }

        async fn request_bytes(&self, req: HaiRequest) -> Result<HaiResponseBytes> {
            let resp = send_request(&self.client, &req).await?;
            let status = resp.status().as_u16();
            let headers = collect_headers(resp.headers());
            let content_type = headers.get("content-type").cloned();
            let body = resp.bytes().await?.to_vec();
            Ok(HaiResponseBytes {
                status,
                body,
                headers,
                content_type,
            })
        }

        async fn request_no_content(&self, req: HaiRequest) -> Result<()> {
            let resp = send_request(&self.client, &req).await?;
            // Drain the body to release the connection even if the
            // server returned data we don't care about.
            let _ = resp.bytes().await?;
            Ok(())
        }
    }

    fn collect_headers(headers: &reqwest::header::HeaderMap) -> BTreeMap<String, String> {
        headers
            .iter()
            .filter_map(|(k, v)| v.to_str().ok().map(|s| (k.as_str().to_string(), s.to_string())))
            .collect()
    }

    async fn send_request(
        client: &Client,
        req: &HaiRequest,
    ) -> Result<reqwest::Response> {
        let mut builder = match req.method {
            HaiHttpMethod::Get => client.get(&req.url),
            HaiHttpMethod::Post => client.post(&req.url),
            HaiHttpMethod::Put => client.put(&req.url),
            HaiHttpMethod::Patch => client.patch(&req.url),
            HaiHttpMethod::Delete => client.delete(&req.url),
        };
        if let Some(auth) = &req.auth_header {
            builder = builder.header("Authorization", auth);
        }
        for (k, v) in &req.headers {
            builder = builder.header(k.as_str(), v.as_str());
        }
        if !req.query.is_empty() {
            let pairs: Vec<(&str, &str)> =
                req.query.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            builder = builder.query(&pairs);
        }
        if let Some(body) = &req.json_body {
            builder = builder.json(body);
        }
        if let Some(timeout) = req.timeout {
            builder = builder.timeout(timeout);
        }
        Ok(builder.send().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hai_request_serializes_with_expected_shape() {
        // The serialization shape is part of the transport's testable
        // contract — callers that capture / replay requests (e.g. for
        // the cross-compat fixture tests) rely on the field order.
        let req = HaiRequest {
            method: HaiHttpMethod::Post,
            url: "https://hai.ai/api/v1/agents/hello".to_string(),
            headers: BTreeMap::new(),
            auth_header: Some("JACS test:1:nonce:sig".to_string()),
            query: BTreeMap::new(),
            json_body: Some(serde_json::json!({"agent_id": "test"})),
            timeout: None,
        };
        let s = serde_json::to_string(&req).unwrap();
        assert!(s.contains("\"method\":\"Post\""), "method serializes: {s}");
        assert!(s.contains("\"agent_id\""), "body present: {s}");
    }
}
