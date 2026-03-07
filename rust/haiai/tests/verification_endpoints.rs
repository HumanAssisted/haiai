use haiai::{HaiClient, HaiClientOptions, StaticJacsProvider, VerifyAgentDocumentRequest};
use httpmock::Method::{GET, POST};
use httpmock::MockServer;
use serde_json::json;

fn make_client(base_url: &str) -> HaiClient<StaticJacsProvider> {
    let provider = StaticJacsProvider::new("agent/with/slash");
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
async fn get_verification_uses_public_endpoint_without_auth() {
    let server = MockServer::start_async().await;

    let with_auth = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/v1/agents/agent%2F..%2Fescape/verification")
                .header_exists("authorization");
            then.status(418);
        })
        .await;

    let expected = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/v1/agents/agent%2F..%2Fescape/verification");
            then.status(200).json_body(json!({
                "agent_id": "agent/../escape",
                "verification": {
                    "jacs_valid": true,
                    "dns_valid": true,
                    "hai_registered": false,
                    "badge": "domain"
                },
                "hai_signatures": ["ed25519:abc..."],
                "verified_at": "2026-01-02T00:00:00Z",
                "errors": []
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let result = client
        .get_verification("agent/../escape")
        .await
        .expect("get verification");
    assert_eq!(result.agent_id, "agent/../escape");
    assert_eq!(result.verification.badge, "domain");
    assert!(!result.verification.hai_registered);

    expected.assert_async().await;
    assert_eq!(with_auth.calls_async().await, 0);
}

#[tokio::test]
async fn verify_agent_document_posts_public_payload_without_auth() {
    let server = MockServer::start_async().await;

    let with_auth = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/v1/agents/verify")
                .header_exists("authorization");
            then.status(418);
        })
        .await;

    let expected = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/v1/agents/verify")
                .json_body(json!({
                    "agent_json": "{\"jacsId\":\"agent-1\"}",
                    "domain": "example.com"
                }));
            then.status(200).json_body(json!({
                "agent_id": "agent-1",
                "verification": {
                    "jacs_valid": true,
                    "dns_valid": true,
                    "hai_registered": true,
                    "badge": "attested"
                },
                "hai_signatures": ["ed25519:def..."],
                "verified_at": "2026-01-02T00:00:00Z",
                "errors": []
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let result = client
        .verify_agent_document(&VerifyAgentDocumentRequest {
            agent_json: "{\"jacsId\":\"agent-1\"}".to_string(),
            public_key: None,
            domain: Some("example.com".to_string()),
        })
        .await
        .expect("verify agent document");

    assert_eq!(result.agent_id, "agent-1");
    assert_eq!(result.verification.badge, "attested");
    assert!(result.verification.hai_registered);

    expected.assert_async().await;
    assert_eq!(with_auth.calls_async().await, 0);
}
