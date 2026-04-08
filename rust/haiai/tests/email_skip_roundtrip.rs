//! PRD Phase 5.1 & 5.3: Verify that GET /email/status is skipped when
//! agent_email is already cached (pre-set from config).
//!
//! These tests use httpmock to prove that the server is NOT contacted for
//! email status when the client already knows the agent's email address.

use haiai::{HaiClient, HaiClientOptions, SendEmailOptions, StaticJacsProvider};
use httpmock::Method::{GET, POST};
use httpmock::MockServer;
use serde_json::json;

fn make_client_with_email(base_url: &str) -> HaiClient<StaticJacsProvider> {
    let provider = StaticJacsProvider::new("skip-test-agent");
    let mut client = HaiClient::new(
        provider,
        HaiClientOptions {
            base_url: base_url.to_string(),
            ..HaiClientOptions::default()
        },
    )
    .expect("client");
    // Pre-set agent_email (simulates email loaded from config on init)
    client.set_agent_email("skip-test-agent@hai.ai".to_string());
    client
}

fn make_client_without_email(base_url: &str) -> HaiClient<StaticJacsProvider> {
    let provider = StaticJacsProvider::new("skip-test-agent");
    HaiClient::new(
        provider,
        HaiClientOptions {
            base_url: base_url.to_string(),
            ..HaiClientOptions::default()
        },
    )
    .expect("client")
}

/// PRD Phase 5.1: When agent_email is pre-set (from config), sending an email
/// must NOT trigger GET /email/status. The mock server should receive zero hits
/// on the email status endpoint.
#[tokio::test]
async fn cached_email_skips_get_email_status_on_send() {
    let server = MockServer::start_async().await;

    // Mock the send endpoint (should be called)
    let send_mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/agents/skip-test-agent/email/send");
            then.status(200).json_body(json!({
                "message_id": "msg-skip-001",
                "status": "queued"
            }));
        })
        .await;

    // Mock the email status endpoint (should NOT be called)
    let status_mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/skip-test-agent/email/status");
            then.status(200).json_body(json!({
                "email": "skip-test-agent@hai.ai",
                "status": "active",
                "tier": "free",
                "daily_used": 0,
                "daily_limit": 100
            }));
        })
        .await;

    let client = make_client_with_email(&server.base_url());

    // Verify email is pre-set
    assert_eq!(
        client.agent_email(),
        Some("skip-test-agent@hai.ai"),
        "agent_email must be pre-set before sending"
    );

    // Send an email -- this should NOT trigger get_email_status
    let result = client
        .send_email(&SendEmailOptions {
            to: "recipient@hai.ai".to_string(),
            subject: "Skip test".to_string(),
            body: "Testing round-trip elimination".to_string(),
            cc: Vec::new(),
            bcc: Vec::new(),
            in_reply_to: None,
            attachments: Vec::new(),
            labels: Vec::new(),
            append_footer: None,
        })
        .await
        .expect("send_email should succeed");

    assert_eq!(result.message_id, "msg-skip-001");

    // The send endpoint should have been called exactly once
    send_mock.assert_async().await;

    // The email status endpoint should NOT have been called at all
    assert_eq!(
        status_mock.calls_async().await,
        0,
        "GET /email/status must NOT be called when agent_email is pre-set"
    );
}

/// PRD Phase 5.1 (negative): When agent_email is NOT set, get_email_status
/// IS called when explicitly invoked.
#[tokio::test]
async fn no_cached_email_allows_get_email_status() {
    let server = MockServer::start_async().await;

    let status_mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/skip-test-agent/email/status");
            then.status(200).json_body(json!({
                "email": "skip-test-agent@hai.ai",
                "status": "active",
                "tier": "free",
                "daily_used": 0,
                "daily_limit": 100
            }));
        })
        .await;

    let client = make_client_without_email(&server.base_url());

    // Verify email is NOT set
    assert!(
        client.agent_email().is_none(),
        "agent_email must be None for this test"
    );

    // Explicitly call get_email_status -- this SHOULD hit the server
    let status = client
        .get_email_status()
        .await
        .expect("get_email_status should succeed");

    assert_eq!(status.email, "skip-test-agent@hai.ai");

    // The email status endpoint should have been called exactly once
    status_mock.assert_async().await;
}

/// PRD Phase 5.3: When agent_email is pre-set, calling get_email_status
/// still works (it's an explicit user request), but the point is that
/// send_email does NOT implicitly call it.
#[tokio::test]
async fn cached_email_does_not_implicitly_fetch_status_on_list() {
    let server = MockServer::start_async().await;

    // Mock list messages endpoint
    let list_mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/skip-test-agent/email/messages");
            then.status(200).json_body(json!([]));
        })
        .await;

    // Mock email status (should NOT be called)
    let status_mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/skip-test-agent/email/status");
            then.status(200).json_body(json!({
                "email": "skip-test-agent@hai.ai",
                "status": "active",
                "tier": "free",
                "daily_used": 0,
                "daily_limit": 100
            }));
        })
        .await;

    let client = make_client_with_email(&server.base_url());

    // List messages -- should NOT trigger get_email_status
    let _messages = client
        .list_messages(&haiai::ListMessagesOptions::default())
        .await
        .expect("list_messages");

    list_mock.assert_async().await;

    assert_eq!(
        status_mock.calls_async().await,
        0,
        "GET /email/status must NOT be called when agent_email is pre-set during list"
    );
}
