use haisdk::{HaiClient, HaiClientOptions, StaticJacsProvider};
use httpmock::Method::{DELETE, GET, POST, PUT};
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
async fn claim_username_escapes_agent_id_path_segment() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/v1/agents/agent%2F..%2Fescape/username");
            then.status(200).json_body(json!({
                "username": "agent",
                "email": "agent@hai.ai",
                "agent_id": "agent/../escape"
            }));
        })
        .await;

    let mut client = make_client(&server.base_url(), "agent/with/slash");
    client
        .claim_username("agent/../escape", "agent")
        .await
        .expect("claim username");

    mock.assert_async().await;
}

#[tokio::test]
async fn submit_response_escapes_job_id_path_segment() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/v1/agents/jobs/job%2Fwith%2Fslash/response");
            then.status(200).json_body(json!({
                "success": true,
                "job_id": "job/with/slash",
                "message": "ok"
            }));
        })
        .await;

    let client = make_client(&server.base_url(), "agent/with/slash");
    client
        .submit_response("job/with/slash", "response body", None, 0)
        .await
        .expect("submit response");

    mock.assert_async().await;
}

#[tokio::test]
async fn mark_read_escapes_jacs_id_and_message_id_segments() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/agents/agent%2Fwith%2Fslash/email/messages/msg%2Fwith%2Fslash/read");
            then.status(204);
        })
        .await;

    let client = make_client(&server.base_url(), "agent/with/slash");
    client.mark_read("msg/with/slash").await.expect("mark read");

    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_remote_key_escapes_jacs_id_and_version_segments() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/jacs/v1/agents/agent%2Fwith%2Fslash/keys/2026%2F01");
            then.status(200).json_body(json!({
                "jacs_id": "agent/with/slash",
                "version": "2026/01",
                "public_key": "pem"
            }));
        })
        .await;

    let client = make_client(&server.base_url(), "agent/with/slash");
    client
        .fetch_remote_key("agent/with/slash", "2026/01")
        .await
        .expect("fetch key");

    mock.assert_async().await;
}

#[tokio::test]
async fn update_username_escapes_agent_id_path_segment() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(PUT)
                .path("/api/v1/agents/agent%2F..%2Fescape/username");
            then.status(200).json_body(json!({
                "username": "new-name",
                "email": "new-name@hai.ai",
                "previous_username": "old-name"
            }));
        })
        .await;

    let client = make_client(&server.base_url(), "agent/with/slash");
    client
        .update_username("agent/../escape", "new-name")
        .await
        .expect("update username");

    mock.assert_async().await;
}

#[tokio::test]
async fn delete_username_escapes_agent_id_path_segment() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/api/v1/agents/agent%2F..%2Fescape/username");
            then.status(200).json_body(json!({
                "released_username": "old-name",
                "cooldown_until": "2026-03-01T00:00:00Z",
                "message": "released"
            }));
        })
        .await;

    let client = make_client(&server.base_url(), "agent/with/slash");
    client
        .delete_username("agent/../escape")
        .await
        .expect("delete username");

    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_key_by_hash_calls_correct_endpoint() {
    let server = MockServer::start_async().await;

    // httpmock matches against the decoded path, so we use the decoded form.
    // The SDK correctly percent-encodes the colon: sha256:abc -> sha256%3Aabc
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/jacs/v1/keys/by-hash/sha256:abc123");
            then.status(200).json_body(json!({
                "jacs_id": "agent-1",
                "version": "v1",
                "public_key": "pem",
                "algorithm": "ed25519",
                "public_key_hash": "sha256:abc123",
                "status": "active",
                "dns_verified": true,
                "created_at": "2026-01-01T00:00:00Z"
            }));
        })
        .await;

    let client = make_client(&server.base_url(), "test-agent");
    let info = client
        .fetch_key_by_hash("sha256:abc123")
        .await
        .expect("fetch key by hash");

    assert_eq!(info.jacs_id, "agent-1");
    assert_eq!(info.public_key_hash, "sha256:abc123");
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_key_by_email_calls_correct_endpoint() {
    let server = MockServer::start_async().await;

    // httpmock matches against the decoded path.
    // The SDK percent-encodes the @: bot@hai.ai -> bot%40hai.ai
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/keys/bot@hai.ai");
            then.status(200).json_body(json!({
                "jacs_id": "agent-2",
                "version": "v1",
                "public_key": "pem",
                "algorithm": "ed25519",
                "public_key_hash": "sha256:def456",
                "status": "active",
                "dns_verified": false,
                "created_at": "2026-01-01T00:00:00Z"
            }));
        })
        .await;

    let client = make_client(&server.base_url(), "test-agent");
    let info = client
        .fetch_key_by_email("bot@hai.ai")
        .await
        .expect("fetch key by email");

    assert_eq!(info.jacs_id, "agent-2");
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_key_by_domain_calls_correct_endpoint() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/jacs/v1/agents/by-domain/example.com");
            then.status(200).json_body(json!({
                "jacs_id": "agent-3",
                "version": "v2",
                "public_key": "pem",
                "algorithm": "ed25519",
                "public_key_hash": "sha256:ghi789",
                "status": "active",
                "dns_verified": true,
                "created_at": "2026-01-01T00:00:00Z"
            }));
        })
        .await;

    let client = make_client(&server.base_url(), "test-agent");
    let info = client
        .fetch_key_by_domain("example.com")
        .await
        .expect("fetch key by domain");

    assert_eq!(info.jacs_id, "agent-3");
    assert!(info.dns_verified);
    mock.assert_async().await;
}

#[tokio::test]
async fn fetch_all_keys_calls_correct_endpoint() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/jacs/v1/agents/agent-4/keys");
            then.status(200).json_body(json!({
                "jacs_id": "agent-4",
                "keys": [
                    {
                        "jacs_id": "agent-4",
                        "version": "v2",
                        "public_key": "pem2",
                        "algorithm": "ed25519",
                        "public_key_hash": "sha256:hash2",
                        "status": "active",
                        "dns_verified": true,
                        "created_at": "2026-02-01T00:00:00Z"
                    },
                    {
                        "jacs_id": "agent-4",
                        "version": "v1",
                        "public_key": "pem1",
                        "algorithm": "ed25519",
                        "public_key_hash": "sha256:hash1",
                        "status": "active",
                        "dns_verified": false,
                        "created_at": "2026-01-01T00:00:00Z"
                    }
                ],
                "total": 2
            }));
        })
        .await;

    let client = make_client(&server.base_url(), "test-agent");
    let history = client
        .fetch_all_keys("agent-4")
        .await
        .expect("fetch all keys");

    assert_eq!(history.jacs_id, "agent-4");
    assert_eq!(history.total, 2);
    assert_eq!(history.keys.len(), 2);
    assert_eq!(history.keys[0].version, "v2");
    assert_eq!(history.keys[1].version, "v1");
    mock.assert_async().await;
}
