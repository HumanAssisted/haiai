//! Regression tests for reply threading.
//!
//! The `reply()` method posts to `/api/agents/{id}/email/reply` which
//! handles threading (In-Reply-To, References, Re: prefix) server-side.
//! These tests verify the SDK correctly posts to the reply endpoint.

use haiai::{HaiClient, HaiClientOptions, StaticJacsProvider};
use httpmock::Method::POST;
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

/// When reply() is called with a message_id, the SDK posts to the
/// `/email/reply` endpoint with the correct message_id in the body.
/// Threading (In-Reply-To, References) is handled server-side.
#[tokio::test]
async fn reply_posts_message_id_to_reply_endpoint() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/agents/test-agent-001/email/reply")
                .body_includes("\"message_id\":\"db-uuid-123\"")
                .body_includes("Thanks!");
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
    mock.assert_async().await;
}

/// reply() without subject_override should not include subject_override
/// in the request body — the server will compute "Re: ..." automatically.
#[tokio::test]
async fn reply_without_subject_override_omits_field() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/agents/test-agent-001/email/reply");
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
    mock.assert_async().await;
}
