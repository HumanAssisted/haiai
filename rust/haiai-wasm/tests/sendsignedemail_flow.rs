//! HAIAI_WASM_PRD §5.4 + §6 / Issues 006 + 010 — drive the actual
//! `BrowserAgentHandle::sendSignedEmail` wrapper through a mocked
//! `fetch`, then walk localStorage to assert no plaintext password,
//! raw private key, or PEM private block leaked across the
//! sign-and-send flow.
//!
//! Why this test exists:
//!
//! - Issue 006 follow-up explicitly required that the real
//!   `sendSignedEmail` wrapper be exercised against a mocked `fetch`,
//!   not a simulated `sign_message_json(canonical_body)` call (which
//!   bypasses the HTTP code path). The wrapper produces an
//!   `Authorization: JACS …` header via `JacsProvider::sign_raw` and
//!   POSTs the locally-built RFC 5322 raw MIME to
//!   `/api/agents/{id}/email/send-signed`. The mock here asserts
//!   exactly that.
//!
//! - The leak scan after the send asserts that even after the agent
//!   has signed a real outbound message and persisted via
//!   `BrowserAgent.save`, no private-key material reaches
//!   localStorage.
//!
//! Tip: this test mounts a process-wide fake `fetch` on
//! `globalThis.fetch` for the test duration and restores the original
//! at the end. Each test installs + tears down its own mock to avoid
//! cross-test bleed. wasm-bindgen-test runs each `#[wasm_bindgen_test]`
//! in its own JS context, but the panic restorer is still belt-and-
//! braces correctness.

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use base64::Engine as _;
use serde_json::Value;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;
use web_sys::Response;

use haiai_wasm::BrowserAgentHandle;

wasm_bindgen_test_configure!(run_in_browser);

const STORAGE_KEY: &str = "haiai-wasm-sendsignedemail-test-agent";
const TEST_PASSWORD: &str = "haiai-wasm-sendsignedemail-test-PASSWORD-9876543210!";
const AGENT_EMAIL: &str = "send-signed-test-agent@hai.ai";
const RECIPIENT_EMAIL: &str = "recipient@example.com";
const SUBJECT: &str = "leak-test-subject";
const BODY: &str = "<p>send-signed mocked fetch body</p>";

const FORBIDDEN_PEM_NEEDLES: &[&str] = &[
    "-----BEGIN PRIVATE KEY-----",
    "-----BEGIN ENCRYPTED PRIVATE KEY-----",
    "-----BEGIN EC PRIVATE KEY-----",
    "-----BEGIN OPENSSH PRIVATE KEY-----",
];

// ---------------------------------------------------------------------------
// Mock fetch — captures (method, url, headers, body) per call and returns a
// canned 200 OK. Tests install via `install_mock_fetch`, then drop the
// returned guard to restore the original `globalThis.fetch`.
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
struct CapturedRequest {
    method: String,
    url: String,
    authorization: Option<String>,
    content_type: Option<String>,
    body_text: String,
    body_bytes: Vec<u8>,
}

struct FetchMock {
    /// All requests captured by the mock, in invocation order.
    requests: Rc<RefCell<Vec<CapturedRequest>>>,
    /// Original `globalThis.fetch` — restored on drop.
    original: JsValue,
    /// Hold the closure alive for the duration of the mock so JS can
    /// keep calling it. Dropped after the mock is uninstalled.
    _closure: Closure<dyn FnMut(JsValue, JsValue) -> js_sys::Promise>,
}

impl FetchMock {
    fn captured(&self) -> Vec<CapturedRequest> {
        self.requests.borrow().clone()
    }
}

impl Drop for FetchMock {
    fn drop(&mut self) {
        let global = js_sys::global();
        // Restore the original fetch reference (or leave undefined if
        // none was present originally — wasm-pack always provides one,
        // but be defensive).
        let _ = js_sys::Reflect::set(&global, &JsValue::from_str("fetch"), &self.original);
    }
}

/// Install a mock `fetch` that records every call and answers with the
/// supplied JSON body. The mock returns the same JSON for every request
/// (the send-signed flow we exercise only fires one mock-eligible call).
fn install_mock_fetch(canned_response_json: &'static str) -> FetchMock {
    let global = js_sys::global();
    let original = js_sys::Reflect::get(&global, &JsValue::from_str("fetch"))
        .unwrap_or(JsValue::UNDEFINED);
    let requests: Rc<RefCell<Vec<CapturedRequest>>> = Rc::new(RefCell::new(Vec::new()));
    let requests_clone = requests.clone();

    let closure: Closure<dyn FnMut(JsValue, JsValue) -> js_sys::Promise> =
        Closure::new(move |input: JsValue, init: JsValue| -> js_sys::Promise {
            let requests = requests_clone.clone();
            wasm_bindgen_futures::future_to_promise(async move {
                let captured =
                    capture_request(&input, &init).await.unwrap_or_else(|e| {
                        // If capture fails (mock bug), record an empty entry
                        // tagged with the error so the test assertion can
                        // surface the real cause.
                        CapturedRequest {
                            method: format!("CAPTURE_FAILED: {e}"),
                            ..Default::default()
                        }
                    });
                requests.borrow_mut().push(captured);

                let init = web_sys::ResponseInit::new();
                init.set_status(200);
                let response = Response::new_with_opt_str_and_init(
                    Some(canned_response_json),
                    &init,
                )
                .map_err(|e| JsValue::from_str(&format!("Response build failed: {e:?}")))?;
                Ok(JsValue::from(response))
            })
        });

    js_sys::Reflect::set(
        &global,
        &JsValue::from_str("fetch"),
        closure.as_ref().unchecked_ref(),
    )
    .expect("install mock fetch on globalThis");

    FetchMock {
        requests,
        original,
        _closure: closure,
    }
}

/// Pull (method, url, headers, body) out of a `fetch(input, init?)` call.
///
/// reqwest's wasm32 backend passes a `Request` object as `input` with
/// no `init`; the body lives on the `Request`. The native browser shim
/// can pass a string URL + `init` object instead. We handle both.
async fn capture_request(input: &JsValue, init: &JsValue) -> Result<CapturedRequest, String> {
    let mut out = CapturedRequest::default();
    if let Ok(req) = input.clone().dyn_into::<web_sys::Request>() {
        out.method = req.method();
        out.url = req.url();
        let headers = req.headers();
        out.authorization = header_get(&headers, "Authorization");
        out.content_type = header_get(&headers, "Content-Type");
        // Body: use the wasm-bindgen JS-side `Request::clone(&req)` (NOT
        // the derived Rust `Clone` impl — that just clones the JsValue
        // pointer and leaves the body stream consumed after the first
        // read). We need two JS clones: one for text(), one for
        // arrayBuffer(). Both consume the body stream.
        if let Ok(cloned_text) = web_sys::Request::clone(&req) {
            if let Ok(text_promise) = cloned_text.text() {
                if let Ok(text_value) = JsFuture::from(text_promise).await {
                    out.body_text = text_value.as_string().unwrap_or_default();
                }
            }
        }
        if let Ok(cloned_buf) = web_sys::Request::clone(&req) {
            if let Ok(buf_promise) = cloned_buf.array_buffer() {
                if let Ok(buf_value) = JsFuture::from(buf_promise).await {
                    let array = js_sys::Uint8Array::new(&buf_value);
                    out.body_bytes = array.to_vec();
                }
            }
        }
        return Ok(out);
    }
    // Fallback: input is a string URL, init carries method/headers/body.
    out.url = input.as_string().unwrap_or_default();
    if !init.is_undefined() && !init.is_null() {
        if let Some(method) =
            js_sys::Reflect::get(init, &JsValue::from_str("method"))
                .ok()
                .and_then(|v| v.as_string())
        {
            out.method = method;
        }
        if let Some(body) =
            js_sys::Reflect::get(init, &JsValue::from_str("body"))
                .ok()
                .and_then(|v| v.as_string())
        {
            out.body_text.clone_from(&body);
            out.body_bytes = body.into_bytes();
        }
        if let Ok(headers) = js_sys::Reflect::get(init, &JsValue::from_str("headers")) {
            if let Ok(h) = headers.dyn_into::<web_sys::Headers>() {
                out.authorization = header_get(&h, "Authorization");
                out.content_type = header_get(&h, "Content-Type");
            }
        }
    }
    Ok(out)
}

fn header_get(headers: &web_sys::Headers, name: &str) -> Option<String> {
    headers.get(name).ok().flatten()
}

// ---------------------------------------------------------------------------
// localStorage walker — copy of the helper in `secret_leak.rs`. Kept
// inline so the two test files do not need to share a helper crate.
// ---------------------------------------------------------------------------

fn walk_local_storage() -> Vec<(String, String)> {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return Vec::new(),
    };
    let storage = match window.local_storage() {
        Ok(Some(s)) => s,
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    let len = storage.length().unwrap_or(0);
    for i in 0..len {
        let Ok(Some(key)) = storage.key(i) else { continue };
        if let Ok(Some(value)) = storage.get_item(&key) {
            out.push((key, value));
        }
    }
    out
}

fn clear_test_keys() {
    if let Some(w) = web_sys::window() {
        if let Ok(Some(s)) = w.local_storage() {
            let len = s.length().unwrap_or(0);
            let mut to_remove = Vec::new();
            for i in 0..len {
                if let Ok(Some(k)) = s.key(i) {
                    if k.contains(STORAGE_KEY) {
                        to_remove.push(k);
                    }
                }
            }
            for k in to_remove {
                let _ = s.remove_item(&k);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The flow test — Issue 006 follow-up real-wrapper variant.
// ---------------------------------------------------------------------------

/// Drive the actual `sendSignedEmail` wrapper:
///
///   1. `createEphemeral("ed25519")`
///   2. `setAgentEmail(...)` — required precondition for sendSignedEmail
///      (matches what the native HaiClient::set_agent_email contract
///      promises).
///   3. install a mock `fetch` that captures the call.
///   4. `sendSignedEmail({to, subject, body, ...})`
///   5. assert the captured request:
///        - method POST,
///        - url ends with `/api/agents/{jacs_id}/email/send-signed`,
///        - `Authorization` header starts with `JACS `,
///        - `Content-Type` is `message/rfc822`,
///        - body bytes contain the agent's `From: <email>` header, the
///          recipient `To:`, the subject, and a JACS signature header.
///   6. `exportEncrypted(password)` + `local_store::save_encrypted_agent`
///      (same code path TS `BrowserAgent.save` takes).
///   7. walk localStorage; assert no password, no raw private key bytes
///      (in any encoding), and no PEM private block.
#[wasm_bindgen_test]
async fn send_signed_email_via_mocked_fetch_then_save_has_no_leaks() {
    clear_test_keys();

    // (1) Create ephemeral agent.
    let handle = BrowserAgentHandle::create_ephemeral("ed25519", None)
        .expect("createEphemeral ed25519");
    let jacs_id = handle.jacs_id();
    assert!(!jacs_id.is_empty(), "ephemeral agent has a jacsId");

    // (2) Set the agent_email (required by HaiClient::send_signed_email).
    handle.set_agent_email(AGENT_EMAIL).expect("setAgentEmail");

    // (3) Install mocked fetch.
    let mock_response = r#"{"message_id":"smoke-msg-1","status":"sent"}"#;
    let mock = install_mock_fetch(mock_response);

    // (4) sendSignedEmail. SendEmailOptions::to is a single String
    // (see rust/haiai/src/types.rs::SendEmailOptions). Many fields have
    // serde defaults / skip_serializing_if so the minimal payload is
    // {to, subject, body}.
    let options_json = serde_json::json!({
        "to": RECIPIENT_EMAIL,
        "subject": SUBJECT,
        "body": BODY,
    })
    .to_string();

    let result = handle
        .send_signed_email(&options_json)
        .await
        .expect("sendSignedEmail resolves");

    // The wrapper returns { message_id, status } as a JS object.
    let result_value: Value = serde_wasm_bindgen::from_value(result).expect("result is JSON");
    assert_eq!(
        result_value.get("message_id").and_then(Value::as_str),
        Some("smoke-msg-1"),
        "mock response surfaces through wrapper"
    );
    assert_eq!(
        result_value.get("status").and_then(Value::as_str),
        Some("sent"),
    );

    // (5) Assert captured request shape.
    let captured = mock.captured();
    assert_eq!(captured.len(), 1, "exactly one fetch call expected, got {}", captured.len());
    let req = &captured[0];
    assert_eq!(req.method, "POST", "send-signed POSTs");
    assert!(
        req.url.contains("/email/send-signed"),
        "url should be the send-signed endpoint, got: {}",
        req.url
    );
    assert!(
        req.url.contains(&jacs_id) || req.url.contains(&urlencode(&jacs_id)),
        "url should embed the agent's jacsId (or its percent-encoded form), got: {}",
        req.url
    );
    let auth = req
        .authorization
        .as_deref()
        .expect("Authorization header present");
    assert!(
        auth.starts_with("JACS "),
        "Authorization must start with `JACS `, got: {auth}"
    );
    assert!(
        auth.contains(&jacs_id) || auth.contains(&urlencode(&jacs_id)),
        "Authorization header should embed jacsId, got: {auth}"
    );
    let content_type = req
        .content_type
        .as_deref()
        .expect("Content-Type header present");
    assert!(
        content_type.starts_with("message/rfc822"),
        "Content-Type must be message/rfc822 for raw MIME, got: {content_type}"
    );

    // Body assertions — the raw RFC 5322 envelope.
    assert!(
        req.body_text.contains(&format!("From: <{AGENT_EMAIL}>"))
            || req.body_text.contains(&format!("From: {AGENT_EMAIL}")),
        "body should carry the agent's From: header; first 200 bytes: {}",
        &req.body_text.chars().take(200).collect::<String>()
    );
    assert!(
        req.body_text.contains(&format!("To: {RECIPIENT_EMAIL}")),
        "body should carry the recipient's To: header"
    );
    assert!(
        req.body_text.contains(SUBJECT),
        "body should carry the subject"
    );
    // Either a JACS-signed header (X-Jacs-Signature) or the JACS payload
    // (the wrapper supports multiple generation types). Just assert
    // *some* JACS marker appears in the bytes.
    let lower = req.body_text.to_lowercase();
    assert!(
        lower.contains("jacs") || lower.contains("x-jacs"),
        "signed body should contain a JACS marker (signature header / payload); \
         first 400 bytes: {}",
        &req.body_text.chars().take(400).collect::<String>()
    );

    // Capture the actual decrypted private-key bytes for the leak scan.
    let material_json = handle
        .export_encrypted(TEST_PASSWORD)
        .expect("exportEncrypted");
    let actual_private_key = decrypt_private_key_from_material(&material_json, TEST_PASSWORD);
    assert!(
        !actual_private_key.is_empty(),
        "decrypt_private_key must yield non-empty bytes"
    );
    let priv_b64 = base64::engine::general_purpose::STANDARD.encode(&actual_private_key);
    let priv_b64_url = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&actual_private_key);
    let priv_hex = actual_private_key
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let priv_hex_upper = priv_hex.to_ascii_uppercase();

    // (6) Persist via the same code path TS BrowserAgent.save uses.
    jacs_wasm::local_store::save_encrypted_agent(STORAGE_KEY, &material_json)
        .expect("save_encrypted_agent");

    // (7) Walk localStorage and assert the secret-hygiene property.
    let entries = walk_local_storage();
    assert!(
        entries.iter().any(|(k, _)| k.contains(STORAGE_KEY)),
        "save_encrypted_agent must have written under our test key; \
         keys: {:?}",
        entries.iter().map(|(k, _)| k).collect::<Vec<_>>()
    );

    for (k, v) in &entries {
        for needle in FORBIDDEN_PEM_NEEDLES {
            assert!(
                !k.contains(needle) && !v.contains(needle),
                "localStorage[{k}] leaked PEM marker `{needle}`"
            );
        }
        assert!(
            !k.contains(TEST_PASSWORD) && !v.contains(TEST_PASSWORD),
            "localStorage[{k}] leaked the literal password"
        );
        assert!(
            !v.contains(&priv_b64),
            "localStorage[{k}] leaked actual private-key bytes (standard base64)"
        );
        assert!(
            !v.contains(&priv_b64_url),
            "localStorage[{k}] leaked actual private-key bytes (url-safe base64)"
        );
        assert!(
            !v.contains(&priv_hex),
            "localStorage[{k}] leaked actual private-key bytes (lowercase hex)"
        );
        assert!(
            !v.contains(&priv_hex_upper),
            "localStorage[{k}] leaked actual private-key bytes (uppercase hex)"
        );
    }

    // Also assert the captured outbound body did not leak the private
    // key — defense-in-depth against a future change accidentally
    // serializing the key into the signed envelope.
    for needle in FORBIDDEN_PEM_NEEDLES {
        assert!(
            !req.body_text.contains(needle),
            "outbound mocked-fetch body leaked PEM marker `{needle}`"
        );
    }
    assert!(
        !req.body_text.contains(&priv_b64),
        "outbound mocked-fetch body leaked actual private-key bytes (base64)"
    );

    clear_test_keys();
}

fn decrypt_private_key_from_material(material_json: &str, password: &str) -> Vec<u8> {
    let material: jacs_core::AgentMaterial =
        serde_json::from_str(material_json).expect("AgentMaterial parses");
    let zeroizing = jacs_core::envelope::decrypt_private_key(
        &material.encrypted_private_key,
        password,
    )
    .expect("decrypt envelope");
    zeroizing.as_slice().to_vec()
}

/// Minimal percent-encode for the small set of characters that appear
/// in JACS IDs but require escaping in URL paths. Used only to make the
/// jacs_id assertion robust against either encoded or raw forms.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            ':' => out.push_str("%3A"),
            '/' => out.push_str("%2F"),
            ' ' => out.push_str("%20"),
            _ => out.push(c),
        }
    }
    out
}
