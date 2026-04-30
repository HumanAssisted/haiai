//! Integration tests for `HaiClient::get_raw_email`.
//!
//! These exercise the wire contract with a mock server (httpmock) and
//! assert byte-identity round-trip of the raw MIME bytes (PRD R2).

use base64::Engine;
use haiai::{HaiClient, HaiClientOptions, HaiError, StaticJacsProvider};
use httpmock::Method::GET;
use httpmock::MockServer;
use serde_json::json;

fn make_client(base_url: &str, jacs_id: &str) -> HaiClient<StaticJacsProvider> {
    let provider = StaticJacsProvider::new(jacs_id);
    HaiClient::new(
        provider,
        HaiClientOptions {
            base_url: base_url.to_string(),
            ..HaiClientOptions::default()
        },
    )
    .expect("client")
}

#[tokio::test]
async fn get_raw_email_returns_bytes_and_metadata() {
    // Bytes include CRLF, NUL, and non-ASCII — exactly what R2 mandates
    // must survive unchanged through the wire.
    let bytes: Vec<u8> = b"From: alice@example.com\r\nTo: bob@hai.ai\r\nSubject: R2\r\n\r\nBody with \x00 NUL and \xc3\xa9\r\n".to_vec();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);

    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/agent-1/email/messages/m.abc/raw")
                .header_exists("Authorization");
            then.status(200).json_body(json!({
                "message_id": "m.abc",
                "rfc_message_id": "<abc@hai.ai>",
                "available": true,
                "raw_email_b64": b64,
                "size_bytes": bytes.len(),
                "omitted_reason": serde_json::Value::Null,
            }));
        })
        .await;

    let client = make_client(&server.base_url(), "agent-1");
    let resp = client.get_raw_email("m.abc").await.expect("get_raw_email");
    mock.assert_async().await;

    assert!(resp.available);
    assert_eq!(resp.raw_email.as_deref(), Some(bytes.as_slice()));
    assert_eq!(resp.size_bytes, Some(bytes.len()));
    assert_eq!(resp.message_id, "m.abc");
    assert_eq!(resp.rfc_message_id.as_deref(), Some("<abc@hai.ai>"));
    assert_eq!(resp.omitted_reason, None);
}

#[tokio::test]
async fn get_raw_email_auth_header_present() {
    let server = MockServer::start_async().await;
    // Assert the JACS auth header is sent with the request.
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/agent-2/email/messages/x/raw")
                .header_matches("authorization", r"^JACS [^:]+:\d+:[A-Za-z0-9+/=_\-]+$");
            then.status(200).json_body(json!({
                "message_id": "x",
                "available": false,
                "raw_email_b64": serde_json::Value::Null,
                "omitted_reason": "not_stored",
            }));
        })
        .await;

    let client = make_client(&server.base_url(), "agent-2");
    let resp = client.get_raw_email("x").await.expect("ok");
    mock.assert_async().await;
    assert!(!resp.available);
    assert_eq!(resp.omitted_reason.as_deref(), Some("not_stored"));
    assert_eq!(resp.raw_email, None);
}

#[tokio::test]
async fn get_raw_email_available_false_not_stored() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/a/email/messages/legacy-id/raw");
            then.status(200).json_body(json!({
                "message_id": "legacy-id",
                "available": false,
                "raw_email_b64": serde_json::Value::Null,
                "size_bytes": serde_json::Value::Null,
                "omitted_reason": "not_stored",
            }));
        })
        .await;

    let client = make_client(&server.base_url(), "a");
    let resp = client.get_raw_email("legacy-id").await.expect("ok");
    mock.assert_async().await;

    assert!(!resp.available);
    assert_eq!(resp.raw_email, None);
    assert_eq!(resp.omitted_reason.as_deref(), Some("not_stored"));
    assert_eq!(resp.size_bytes, None);
}

#[tokio::test]
async fn get_raw_email_available_false_oversize() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/a/email/messages/big-id/raw");
            then.status(200).json_body(json!({
                "message_id": "big-id",
                "available": false,
                "raw_email_b64": serde_json::Value::Null,
                "size_bytes": serde_json::Value::Null,
                "omitted_reason": "oversize",
            }));
        })
        .await;

    let client = make_client(&server.base_url(), "a");
    let resp = client.get_raw_email("big-id").await.expect("ok");
    mock.assert_async().await;

    assert!(!resp.available);
    assert_eq!(resp.raw_email, None);
    assert_eq!(resp.omitted_reason.as_deref(), Some("oversize"));
}

/// Issue 012: the server's SMTP DATA path cannot deliver byte-identical
/// wire bytes, so it writes `raw_mime_omitted_reason = 'reconstructed'`
/// and the handler surfaces `omitted_reason: "reconstructed"`. The SDK
/// MUST surface that sentinel verbatim so callers can distinguish it
/// from `"not_stored"` (legacy pre-feature row) and fall back to
/// IMAP/JMAP for bit-exact bytes.
#[tokio::test]
async fn get_raw_email_available_false_reconstructed() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/a/email/messages/recon-id/raw");
            then.status(200).json_body(json!({
                "message_id": "recon-id",
                "available": false,
                "raw_email_b64": serde_json::Value::Null,
                "size_bytes": serde_json::Value::Null,
                "omitted_reason": "reconstructed",
            }));
        })
        .await;

    let client = make_client(&server.base_url(), "a");
    let resp = client.get_raw_email("recon-id").await.expect("ok");
    mock.assert_async().await;

    assert!(!resp.available);
    assert_eq!(resp.raw_email, None);
    assert_eq!(resp.omitted_reason.as_deref(), Some("reconstructed"));
    assert_eq!(resp.size_bytes, None);
}

#[tokio::test]
async fn get_raw_email_404_is_api_error() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/a/email/messages/missing/raw");
            then.status(404)
                .json_body(json!({"error": "message not found"}));
        })
        .await;

    let client = make_client(&server.base_url(), "a");
    let err = client
        .get_raw_email("missing")
        .await
        .expect_err("not found");
    mock.assert_async().await;
    match err {
        HaiError::Api { status, .. } => assert_eq!(status, 404),
        other => panic!("expected HaiError::Api{{status:404}}, got: {other:?}"),
    }
}

#[tokio::test]
async fn get_raw_email_url_escapes_message_id() {
    let server = MockServer::start_async().await;
    // A message id containing `/` must be path-segment-escaped (`%2F`) to
    // avoid cleaving the URL into a new segment. This is the same escaping
    // used by `get_message` / `mark_read` (see path_escaping.rs).
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/agent%2Fwith%2Fslash/email/messages/msg%2Fwith%2Fslash/raw");
            then.status(200).json_body(json!({
                "message_id": "msg/with/slash",
                "available": true,
                "raw_email_b64": base64::engine::general_purpose::STANDARD.encode(b"hello"),
                "size_bytes": 5,
                "omitted_reason": serde_json::Value::Null,
            }));
        })
        .await;

    let client = make_client(&server.base_url(), "agent/with/slash");
    let resp = client.get_raw_email("msg/with/slash").await.expect("ok");
    mock.assert_async().await;
    assert!(resp.available);
    assert_eq!(resp.raw_email.as_deref(), Some(&b"hello"[..]));
}

#[tokio::test]
async fn get_raw_email_byte_identity_crlf_nul_non_ascii() {
    // The critical R2 assertion: bytes in == bytes out, no normalization.
    let bytes: Vec<u8> = {
        let mut v = Vec::new();
        v.extend_from_slice(b"\r\n"); // leading CRLF
        v.push(0x00); // embedded NUL
        v.extend_from_slice(b"mid"); // ASCII
        v.extend_from_slice(&[0xc3, 0xa9]); // é utf-8
        v.push(0xff); // lone 0xFF (invalid utf-8 on purpose)
        v.extend_from_slice(b"\r\n"); // trailing CRLF
        v
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);

    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/a/email/messages/byte-id/raw");
            then.status(200).json_body(json!({
                "message_id": "byte-id",
                "available": true,
                "raw_email_b64": b64,
                "size_bytes": bytes.len(),
                "omitted_reason": serde_json::Value::Null,
            }));
        })
        .await;

    let client = make_client(&server.base_url(), "a");
    let resp = client.get_raw_email("byte-id").await.expect("ok");
    mock.assert_async().await;

    // The load-bearing assertion: full Vec<u8> equality, no trim/lossy.
    assert_eq!(resp.raw_email.expect("present"), bytes);
}
