// Copyright (c) 2026 Human Assisted Intelligence, Inc.
// SPDX-License-Identifier: BUSL-1.1

//! `HaiaiWasmError` — JS-facing error mapping (HAIAI_WASM_PRD §3.1 /
//! Task 021). Every rejection from a `BrowserAgentHandle` method is a
//! `JsError` whose message is a JSON payload
//! `{ code, message, details? }`. JS callers do
//! `JSON.parse(err.message).code` to dispatch on the stable code.

#![cfg(target_arch = "wasm32")]

use serde::Serialize;
use wasm_bindgen::prelude::*;

/// Stable error payload shape. Every method goes through `to_js_error`
/// to surface this shape.
#[derive(Debug, Serialize)]
struct WirePayload<'a> {
    code: &'a str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<serde_json::Value>,
}

/// Map a `HaiError` into a `JsError` carrying the JACS-style code.
///
/// Codes are stabilised per PRD §3.1. Unknown variants fall back to
/// `Internal`. Callers receive the JSON form so they can `.code` switch
/// on it.
pub fn map_hai_error(err: haiai::HaiError) -> JsError {
    let code = hai_error_code(&err);
    to_js_error(code, format!("{err}"), None)
}

/// Construct a `JsError` for a string source (e.g. failed JSON
/// serialization at the JS-bridge boundary).
pub fn js_error(code: &str, message: impl Into<String>) -> JsError {
    to_js_error(code, message.into(), None)
}

/// Construct a `JsError` with optional structured details.
pub fn to_js_error(code: &str, message: String, details: Option<serde_json::Value>) -> JsError {
    let payload = WirePayload {
        code,
        message,
        details,
    };
    let json = serde_json::to_string(&payload)
        .unwrap_or_else(|_| format!("{{\"code\":\"{code}\",\"message\":\"<unserializable>\"}}"));
    JsError::new(&json)
}

/// Map a `HaiError` variant to the PRD §3.1 stable string code.
fn hai_error_code(err: &haiai::HaiError) -> &'static str {
    use haiai::HaiError as E;
    match err {
        E::Validation { .. } => "Validation",
        E::Api { status, .. } => match *status {
            400 => "BadRequest",
            401 | 403 => "Unauthorized",
            404 => "NotFound",
            408 => "Timeout",
            429 => "RateLimited",
            500..=599 => "ServerError",
            _ => "HttpStatus",
        },
        E::Http(_) => "Network",
        E::Json(_) => "MalformedResponse",
        E::ConfigNotFound { .. } | E::ConfigInvalid { .. } => "ConfigInvalid",
        E::MissingJacsId => "MissingJacsId",
        E::Provider(_) => "Provider",
        E::Message(_) => "Internal",
        E::VerifyUrlTooLong { .. } => "VerifyLinkTooLarge",
        E::MissingHostedDocumentId => "MissingHostedDocumentId",
        E::BackendUnsupported { .. } => "Unsupported",
    }
}
