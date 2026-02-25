//! Regression tests for reply threading.
//!
//! The `reply()` method must set `in_reply_to` to the RFC 5322 `message_id`
//! (e.g. `<uuid.bot@hai.ai>`) when available, falling back to the database
//! `id` only when `message_id` is `None`.

use haisdk::{HaiClient, HaiClientOptions, StaticJacsProvider};
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

/// When the original message has a populated `message_id` field, `reply()`
/// must use that value (not the database `id`) as the `in_reply_to` header
/// on the outgoing email.
#[tokio::test]
async fn reply_uses_message_id_for_threading_when_present() {
    let server = MockServer::start_async().await;

    // GET /email/messages/db-uuid-123 returns a message whose RFC 5322
    // message_id differs from the database id.
    let get_mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/test-agent-001/email/messages/db-uuid-123");
            then.status(200).json_body(json!({
                "id": "db-uuid-123",
                "message_id": "<db-uuid-123.bot@hai.ai>",
                "from_address": "alice@hai.ai",
                "to_address": "test-agent-001@hai.ai",
                "subject": "Hello",
                "body_text": "Hi there",
                "created_at": "2026-02-24T10:00:00Z"
            }));
        })
        .await;

    // POST /email/send must carry `in_reply_to` equal to the RFC 5322
    // message_id, NOT the database id.
    let send_mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/agents/test-agent-001/email/send")
                .json_body_partial(
                    r#"{"in_reply_to": "<db-uuid-123.bot@hai.ai>"}"#,
                );
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

/// When the original message has `message_id: null` (None), `reply()` must
/// fall back to using the database `id` as the `in_reply_to` value.
#[tokio::test]
async fn reply_falls_back_to_id_when_message_id_is_none() {
    let server = MockServer::start_async().await;

    // GET returns a message with no message_id (null / missing).
    let get_mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/test-agent-001/email/messages/db-uuid-456");
            then.status(200).json_body(json!({
                "id": "db-uuid-456",
                "message_id": null,
                "from_address": "bob@hai.ai",
                "to_address": "test-agent-001@hai.ai",
                "subject": "Question",
                "body_text": "Any updates?",
                "created_at": "2026-02-24T11:00:00Z"
            }));
        })
        .await;

    // POST must carry `in_reply_to` equal to the database id fallback.
    let send_mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/agents/test-agent-001/email/send")
                .json_body_partial(r#"{"in_reply_to": "db-uuid-456"}"#);
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
        .expect("reply should succeed with fallback id");

    assert_eq!(result.message_id, "reply-msg-002");
    assert_eq!(result.status, "queued");

    get_mock.assert_async().await;
    send_mock.assert_async().await;
}
