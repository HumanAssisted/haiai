use std::fs;
use std::path::PathBuf;

use haiai::{HaiClient, HaiClientOptions, RegisterAgentOptions, StaticJacsProvider};
use httpmock::Method::{GET, POST};
use httpmock::MockServer;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
struct EndpointContract {
    method: String,
    path: String,
    auth_required: bool,
}

#[derive(Debug, Deserialize)]
struct ContractFixture {
    base_url: String,
    hello: EndpointContract,
    submit_response: EndpointContract,
}

fn load_contract_fixture() -> ContractFixture {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/contract_endpoints.json");
    let raw = fs::read_to_string(path).expect("read contract fixture");
    serde_json::from_str(&raw).expect("decode fixture")
}

fn make_client(base_url: &str) -> HaiClient<StaticJacsProvider> {
    let provider = StaticJacsProvider::new("test-agent-001");
    HaiClient::new(
        provider,
        HaiClientOptions {
            base_url: base_url.to_string(),
            ..HaiClientOptions::default()
        },
    )
    .expect("client")
}

fn method_from_fixture(method: &str) -> httpmock::Method {
    match method {
        "GET" => GET,
        "POST" => POST,
        other => panic!("unsupported method in fixture: {other}"),
    }
}

#[tokio::test]
async fn hello_uses_shared_method_path_auth_contract() {
    let fixture = load_contract_fixture();
    assert_eq!(fixture.base_url, "https://beta.hai.ai");
    let server = MockServer::start_async().await;

    let hello = server
        .mock_async(|when, then| {
            let when = when
                .method(method_from_fixture(&fixture.hello.method))
                .path(fixture.hello.path.clone());
            let _when = if fixture.hello.auth_required {
                when.header_exists("authorization")
            } else {
                when
            };
            then.status(200).json_body(json!({
                "timestamp": "2026-01-01T00:00:00Z",
                "client_ip": "127.0.0.1",
                "hai_public_key_fingerprint": "fp",
                "message": "ok",
                "hello_id": "h1"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    client.hello(false).await.expect("hello response");

    hello.assert_async().await;
}

#[tokio::test]
async fn submit_response_uses_shared_method_path_auth_contract() {
    let fixture = load_contract_fixture();
    let server = MockServer::start_async().await;

    let expected_path = fixture.submit_response.path.replace("{job_id}", "job-123");

    let mock = server
        .mock_async(|when, then| {
            let when = when
                .method(method_from_fixture(&fixture.submit_response.method))
                .path(expected_path);
            let _when = if fixture.submit_response.auth_required {
                when.header_exists("authorization")
            } else {
                when
            };
            then.status(200).json_body(json!({
                "success": true,
                "job_id": "job-123",
                "message": "ok"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    client
        .submit_response("job-123", "response body", None, 0)
        .await
        .expect("submit response");

    mock.assert_async().await;
}

#[tokio::test]
async fn register_posts_bootstrap_payload() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/v1/agents/register")
                .json_body(json!({
                    "agent_json": "{\"jacsId\":\"agent-1\"}",
                    "public_key": "cHVibGljLWtleS1wZW0=",
                    "owner_email": "owner@example.com",
                    "domain": "agent.example.com",
                    "description": "Agent registered via Rust test"
                }));
            then.status(201).json_body(json!({
                "agent_id": "agent-1",
                "jacs_id": "agent-1",
                "dns_verified": false,
                "registrations": [],
                "registered_at": "2026-01-01T00:00:00Z"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let result = client
        .register(&RegisterAgentOptions {
            agent_json: "{\"jacsId\":\"agent-1\"}".to_string(),
            public_key_pem: Some("public-key-pem".to_string()),
            owner_email: Some("owner@example.com".to_string()),
            domain: Some("agent.example.com".to_string()),
            ..Default::default()
        })
        .await
        .expect("register");

    assert_eq!(result.jacs_id, "agent-1");
    mock.assert_async().await;
}

/// Registration must NOT include an Authorization header (unauthenticated endpoint).
#[tokio::test]
async fn register_is_unauthenticated() {
    let server = MockServer::start_async().await;

    // This mock only matches if there is NO Authorization header.
    // httpmock: if the request has an Authorization header, it won't match
    // this mock and the client will get a 404, causing the test to fail.
    let mock_no_auth = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/v1/agents/register")
                .header("Authorization", "JACS this-should-never-appear");
            then.status(500).body("should never match");
        })
        .await;

    let mock_ok = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/v1/agents/register");
            then.status(201).json_body(json!({
                "agent_id": "agent-1",
                "jacs_id": "agent-1",
                "dns_verified": false,
                "registrations": [],
                "registered_at": "2026-01-01T00:00:00Z"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let result = client
        .register(&RegisterAgentOptions {
            agent_json: "{}".to_string(),
            public_key_pem: Some("pub-key".to_string()),
            owner_email: Some("owner@hai.ai".to_string()),
            domain: None,
            ..Default::default()
        })
        .await
        .expect("register should succeed without auth");

    assert_eq!(result.jacs_id, "agent-1");
    // The JACS auth mock should have zero hits (register doesn't send auth)
    assert_eq!(mock_no_auth.hits_async().await, 0, "register must NOT send Authorization header");
    mock_ok.assert_async().await;
}

/// Registration POST body must not contain private key material.
/// This test verifies at the source code level that `register()` only sends
/// agent_json, public_key (base64), owner_email, domain, and description.
#[tokio::test]
async fn register_omits_private_key() {
    let server = MockServer::start_async().await;

    // If the body contained "PRIVATE KEY", this mock would match
    let private_key_trap = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/v1/agents/register")
                .body_includes("PRIVATE KEY");
            then.status(500).body("private key leaked!");
        })
        .await;

    let mock_ok = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/v1/agents/register");
            then.status(201).json_body(json!({
                "agent_id": "agent-1",
                "jacs_id": "agent-1",
                "dns_verified": false,
                "registrations": [],
                "registered_at": "2026-01-01T00:00:00Z"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let result = client
        .register(&RegisterAgentOptions {
            agent_json: "{\"jacsId\":\"agent-1\"}".to_string(),
            public_key_pem: Some("-----BEGIN PUBLIC KEY-----\nfake\n-----END PUBLIC KEY-----".to_string()),
            owner_email: Some("owner@hai.ai".to_string()),
            domain: None,
            ..Default::default()
        })
        .await
        .expect("register should succeed without private key");

    assert_eq!(result.jacs_id, "agent-1");
    // The private key trap mock should have zero hits
    assert_eq!(private_key_trap.hits_async().await, 0, "register body must NOT contain PRIVATE KEY");
    mock_ok.assert_async().await;
}
