use std::fs;
use std::path::PathBuf;

use haiai::{
    resolve_private_key_candidates, AgentConfig, HaiClient, HaiClientOptions, RegisterAgentOptions,
    StaticJacsProvider,
};
use httpmock::Method::POST;
use httpmock::MockServer;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
struct BootstrapRegisterContract {
    method: String,
    path: String,
    auth_required: bool,
    public_key_encoding: String,
}

#[derive(Debug, Deserialize)]
struct InitContractFixture {
    bootstrap_register: BootstrapRegisterContract,
    private_key_candidate_order: Vec<String>,
    config_discovery_order: Vec<String>,
}

fn load_init_contract_fixture() -> InitContractFixture {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/init_contract.json");
    let raw = fs::read_to_string(path).expect("read init contract fixture");
    serde_json::from_str(&raw).expect("decode init fixture")
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

#[test]
fn private_key_candidate_order_matches_shared_fixture() {
    let fixture = load_init_contract_fixture();
    assert_eq!(
        fixture.config_discovery_order,
        vec!["explicit_path", "JACS_CONFIG_PATH", "./jacs.config.json"]
    );

    let cfg = AgentConfig {
        jacs_agent_name: "agent-alpha".to_string(),
        jacs_agent_version: "1.0.0".to_string(),
        jacs_key_dir: PathBuf::from("/tmp/shared-key-order"),
        jacs_id: Some("agent-alpha-id".to_string()),
        jacs_private_key_path: None,
        source_path: PathBuf::from("/tmp/shared-key-order/jacs.config.json"),
    };

    let candidates = resolve_private_key_candidates(&cfg);
    let names: Vec<String> = candidates
        .iter()
        .map(|p| {
            p.file_name()
                .expect("filename")
                .to_string_lossy()
                .to_string()
        })
        .collect();

    let expected: Vec<String> = fixture
        .private_key_candidate_order
        .iter()
        .map(|v| v.replace("{agentName}", &cfg.jacs_agent_name))
        .collect();

    assert_eq!(names, expected);
}

#[tokio::test]
async fn register_bootstrap_matches_shared_fixture() {
    let fixture = load_init_contract_fixture();
    assert_eq!(fixture.bootstrap_register.method, "POST");
    assert!(!fixture.bootstrap_register.auth_required);
    assert_eq!(fixture.bootstrap_register.public_key_encoding, "base64");

    let server = MockServer::start_async().await;

    let auth_guard = server
        .mock_async(|when, then| {
            when.method(POST)
                .path(fixture.bootstrap_register.path.clone())
                .header_exists("authorization");
            then.status(418);
        })
        .await;

    let expected = server
        .mock_async(|when, then| {
            when.method(POST)
                .path(fixture.bootstrap_register.path.clone())
                .json_body(json!({
                    "agent_json": "{\"jacsId\":\"agent-1\"}",
                    "public_key": "cHVibGljLWtleS1wZW0=",
                    "owner_email": "owner@example.com",
                    "domain": "agent.example.com",
                    "description": "Cross-language bootstrap contract"
                }));
            then.status(201).json_body(json!({
                "agent_id": "agent-1",
                "jacs_id": "agent-1",
                "registered_at": "2026-01-01T00:00:00Z",
                "registrations": [],
                "dns_verified": false
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let _ = client
        .register(&RegisterAgentOptions {
            agent_json: "{\"jacsId\":\"agent-1\"}".to_string(),
            public_key_pem: Some("public-key-pem".to_string()),
            owner_email: Some("owner@example.com".to_string()),
            domain: Some("agent.example.com".to_string()),
            ..Default::default()
        })
        .await
        .expect("register");

    expected.assert_async().await;
    assert_eq!(auth_guard.calls_async().await, 0);
}
