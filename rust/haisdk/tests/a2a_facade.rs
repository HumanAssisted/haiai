use std::fs;
use std::path::PathBuf;

use haisdk::{
    A2AAgentCard, A2ATrustPolicy, HaiClient, HaiClientOptions, RegisterAgentOptions,
    StaticJacsProvider,
};
use serde_json::{json, Value};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/a2a")
        .join(name)
}

fn load_fixture_json(name: &str) -> Value {
    let data = fs::read_to_string(fixture_path(name)).expect("read fixture");
    serde_json::from_str(&data).expect("decode fixture")
}

fn make_client() -> HaiClient<StaticJacsProvider> {
    HaiClient::new(
        StaticJacsProvider::new("demo-agent"),
        HaiClientOptions {
            base_url: "https://hai.ai".to_string(),
            ..HaiClientOptions::default()
        },
    )
    .expect("client")
}

#[test]
fn loads_shared_a2a_fixtures() {
    let card_v04 = load_fixture_json("agent_card.v04.json");
    let card_v10 = load_fixture_json("agent_card.v10.json");
    let wrapped = load_fixture_json("wrapped_task.with_parents.json");
    let well_known = load_fixture_json("well_known_bundle.v10.json");

    assert_eq!(card_v04["name"], "HAISDK Demo Agent");
    assert_eq!(card_v04["protocolVersions"], json!(["0.4.0"]));
    assert_eq!(card_v10["supportedInterfaces"][0]["protocolVersion"], "1.0");
    assert_eq!(wrapped["jacsType"], "a2a-task-result");
    assert!(well_known["/.well-known/agent-card.json"].is_object());
}

#[test]
fn signs_and_verifies_artifact_roundtrip() {
    let client = make_client();
    let a2a = client.get_a2a(Some(A2ATrustPolicy::Verified));

    let wrapped = a2a
        .sign_artifact(json!({"taskId":"task-1","input":"hello"}), "task", None)
        .expect("sign");
    let verify = a2a.verify_artifact(&wrapped).expect("verify");

    assert!(verify.valid);
    assert_eq!(verify.signer_id, "demo-agent");
    assert_eq!(verify.artifact_type, "a2a-task");
}

#[test]
fn register_options_with_agent_card_embeds_metadata() {
    let client = make_client();
    let a2a = client.get_a2a(None);

    let card: A2AAgentCard =
        serde_json::from_value(load_fixture_json("agent_card.v10.json")).expect("card fixture");

    let opts = RegisterAgentOptions {
        agent_json: r#"{"jacsId":"demo-agent","name":"Demo Agent"}"#.to_string(),
        public_key_pem: None,
        owner_email: None,
        domain: None,
        description: None,
    };
    let merged = a2a
        .register_options_with_agent_card(opts, &card)
        .expect("merge register opts");

    let merged_json: Value = serde_json::from_str(&merged.agent_json).expect("decode merged");
    assert!(merged_json.get("a2aAgentCard").is_some());
    assert_eq!(merged_json["metadata"]["a2aProfile"], "1.0");
    assert_eq!(merged_json["metadata"]["a2aSkillsCount"], 1);
}

#[test]
fn assesses_trust_cases_fixture() {
    let client = make_client();
    let a2a = client.get_a2a(None);
    let cases = load_fixture_json("trust_assessment_cases.json")["cases"]
        .as_array()
        .expect("cases array")
        .clone();

    for case in cases {
        let policy = case["policy"].as_str().expect("policy");
        let expected = case["expected"]["allowed"].as_bool().expect("expected");
        let card_json = serde_json::to_string(&case["card"]).expect("card json");

        let assessment = a2a
            .assess_remote_agent(
                &card_json,
                Some(match policy {
                    "open" => A2ATrustPolicy::Open,
                    "verified" => A2ATrustPolicy::Verified,
                    "strict" => A2ATrustPolicy::Strict,
                    _ => A2ATrustPolicy::Verified,
                }),
            )
            .expect("assess");
        assert_eq!(assessment.allowed, expected, "policy={policy}");
    }
}
