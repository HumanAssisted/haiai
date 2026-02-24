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

    let client = make_client(&server.base_url(), "agent/with/slash");
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
