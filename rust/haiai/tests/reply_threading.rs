//! Regression tests for reply threading.
//!
//! The `reply()` method fetches the original message client-side, sanitizes the
//! subject (stripping CR/LF from email header folding), and sends a JACS-signed
//! email via `send_signed_email` (POST to `/email/send-signed` with `message/rfc822`).

use haiai::{HaiClient, HaiClientOptions, StaticJacsProvider};
use httpmock::Method::{GET, POST};
use httpmock::MockServer;
use serde_json::json;

fn make_client(base_url: &str) -> HaiClient<StaticJacsProvider> {
    let provider = StaticJacsProvider::new("test-agent-001");
    let mut client = HaiClient::new(
        provider,
        HaiClientOptions {
            base_url: base_url.to_string(),
            ..HaiClientOptions::default()
        },
    )
    .expect("client");
    client.set_agent_email("test-agent-001@hai.ai".to_string());
    client
}

/// reply() fetches the original message and sends a signed reply.
#[tokio::test]
async fn reply_posts_message_id_to_reply_endpoint() {
    let server = MockServer::start_async().await;

    let get_mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/test-agent-001/email/messages/db-uuid-123");
            then.status(200).json_body(json!({
                "id": "db-uuid-123",
                "direction": "inbound",
                "from_address": "alice@hai.ai",
                "to_address": "test-agent-001@hai.ai",
                "subject": "Hello",
                "body_text": "Hi there",
                "is_read": true,
                "delivery_status": "delivered",
                "created_at": "2026-03-25T00:00:00Z"
            }));
        })
        .await;

    let send_mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/agents/test-agent-001/email/send-signed")
                .header("content-type", "message/rfc822");
            then.status(200).json_body(json!({
                "message_id": "reply-msg-001",
                "status": "queued"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let result = client
        .reply("db-uuid-123", "Thanks!", None)
        .await
        .expect("reply should succeed");

    assert_eq!(result.message_id, "reply-msg-001");
    assert_eq!(result.status, "queued");
    get_mock.assert_async().await;
    send_mock.assert_async().await;
}

/// reply() without subject_override auto-generates "Re: <original subject>".
/// The subject is sanitized (CR/LF stripped) and the email is JACS-signed.
#[tokio::test]
async fn reply_without_subject_override_omits_field() {
    let server = MockServer::start_async().await;

    let get_mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/test-agent-001/email/messages/db-uuid-456");
            then.status(200).json_body(json!({
                "id": "db-uuid-456",
                "direction": "inbound",
                "from_address": "bob@hai.ai",
                "to_address": "test-agent-001@hai.ai",
                "subject": "Important\r\n topic",
                "body_text": "Details",
                "is_read": true,
                "delivery_status": "delivered",
                "created_at": "2026-03-25T00:00:00Z"
            }));
        })
        .await;

    let send_mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/agents/test-agent-001/email/send-signed")
                .header("content-type", "message/rfc822");
            then.status(200).json_body(json!({
                "message_id": "reply-msg-002",
                "status": "queued"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let result = client
        .reply("db-uuid-456", "Thanks!", None)
        .await
        .expect("reply should succeed with no override");

    assert_eq!(result.message_id, "reply-msg-002");
    get_mock.assert_async().await;
    send_mock.assert_async().await;
}
