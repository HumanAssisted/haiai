//! A2A Verification Contract Tests
//!
//! These tests validate that the Rust SDK's A2A struct serialization
//! matches the canonical contract fixture at fixtures/a2a_verification_contract.json.
//! They catch schema drift across languages by verifying field names, types,
//! and roundtrip values.

use std::fs;
use std::path::PathBuf;

use haiai::{A2AAgentCard, A2AArtifactVerificationResult, A2ATrustAssessment, A2AWrappedArtifact};
use serde_json::Value;

fn contract_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/a2a_verification_contract.json")
}

fn a2a_fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/a2a")
        .join(name)
}

fn load_contract() -> Value {
    let data = fs::read_to_string(contract_path()).expect("read contract fixture");
    serde_json::from_str(&data).expect("decode contract fixture")
}

fn load_a2a_fixture(name: &str) -> Value {
    let data = fs::read_to_string(a2a_fixture_path(name)).expect("read a2a fixture");
    serde_json::from_str(&data).expect("decode a2a fixture")
}

/// Assert that all fields from required_fields exist in obj.
fn assert_fields_present(label: &str, obj: &Value, required_fields: &Value) {
    let obj_map = obj.as_object().unwrap_or_else(|| {
        panic!("{label}: expected object");
    });
    let fields = required_fields.as_object().unwrap_or_else(|| {
        panic!("{label}: required_fields not an object");
    });

    for key in fields.keys() {
        if key == "_comment" {
            continue;
        }
        assert!(
            obj_map.contains_key(key),
            "{label}: missing required field '{key}'"
        );
    }
}

/// Assert a value has the expected JSON type.
fn assert_field_type(label: &str, field: &str, expected_type: &str, value: &Value) {
    match expected_type {
        "string" => assert!(
            value.is_string(),
            "{label}.{field}: expected string, got {value}"
        ),
        "boolean" => assert!(
            value.is_boolean(),
            "{label}.{field}: expected boolean, got {value}"
        ),
        "object" => assert!(
            value.is_object(),
            "{label}.{field}: expected object, got {value}"
        ),
        "array" => assert!(
            value.is_array(),
            "{label}.{field}: expected array, got {value}"
        ),
        "number" => assert!(
            value.is_number(),
            "{label}.{field}: expected number, got {value}"
        ),
        _ => {} // types like "string|null" or "object|null" are optional
    }
}

// ---------------------------------------------------------------------------
// WrappedArtifact schema tests
// ---------------------------------------------------------------------------

#[test]
fn contract_wrapped_artifact_roundtrip_fields() {
    let contract = load_contract();
    let schema = &contract["wrappedArtifactSchema"];
    let required = &schema["requiredFields"];
    let sig_fields = &schema["signatureFields"];

    // Deserialize the fixture into A2AWrappedArtifact and re-serialize.
    let wrapped_fixture = &contract["wrappedArtifact"];
    let wrapped: A2AWrappedArtifact =
        serde_json::from_value(wrapped_fixture.clone()).expect("deserialize A2AWrappedArtifact");

    let reserialized = serde_json::to_value(&wrapped).expect("re-serialize A2AWrappedArtifact");

    // Check all required fields are present after roundtrip.
    assert_fields_present("A2AWrappedArtifact", &reserialized, required);

    // Check types.
    for (field, expected_type) in required.as_object().unwrap() {
        if field == "_comment" {
            continue;
        }
        assert_field_type(
            "A2AWrappedArtifact",
            field,
            expected_type.as_str().unwrap(),
            &reserialized[field],
        );
    }

    // Check signature sub-fields.
    let sig = &reserialized["jacsSignature"];
    assert!(sig.is_object(), "jacsSignature should be an object");
    assert_fields_present("A2AArtifactSignature", sig, sig_fields);

    // Verify the critical agentID casing.
    assert!(
        sig.get("agentID").is_some(),
        "Signature must use 'agentID' (uppercase ID)"
    );

    // Verify roundtrip values.
    assert_eq!(
        wrapped.jacs_id,
        "contract-00000000-0000-4000-8000-000000000001"
    );
    assert_eq!(wrapped.jacs_type, "a2a-task");
    assert_eq!(wrapped.jacs_level, "artifact");
    assert_eq!(wrapped.jacs_version, "1.0.0");
    let sig_ref = wrapped.jacs_signature.as_ref().expect("signature");
    assert_eq!(sig_ref.agent_id, "contract-agent");
}

// ---------------------------------------------------------------------------
// VerificationResult schema tests
// ---------------------------------------------------------------------------

#[test]
fn contract_verification_result_roundtrip_fields() {
    let contract = load_contract();
    let schema = &contract["verificationResultSchema"];
    let required = &schema["requiredFields"];

    let example = &contract["verificationResultExample"];
    let result: A2AArtifactVerificationResult =
        serde_json::from_value(example.clone()).expect("deserialize verification result");

    let reserialized = serde_json::to_value(&result).expect("re-serialize verification result");

    assert_fields_present("A2AArtifactVerificationResult", &reserialized, required);

    for (field, expected_type) in required.as_object().unwrap() {
        if field == "_comment" {
            continue;
        }
        assert_field_type(
            "A2AArtifactVerificationResult",
            field,
            expected_type.as_str().unwrap(),
            &reserialized[field],
        );
    }

    // Verify camelCase field names (not snake_case).
    let obj = reserialized.as_object().unwrap();
    assert!(
        obj.contains_key("signerId"),
        "must use 'signerId' not 'signer_id'"
    );
    assert!(!obj.contains_key("signer_id"));
    assert!(
        obj.contains_key("artifactType"),
        "must use 'artifactType' not 'artifact_type'"
    );
    assert!(!obj.contains_key("artifact_type"));
    assert!(
        obj.contains_key("originalArtifact"),
        "must use 'originalArtifact' not 'original_artifact'"
    );
    assert!(!obj.contains_key("original_artifact"));

    // Check specific values.
    assert!(!result.valid);
    assert_eq!(result.signer_id, "contract-agent");
    assert_eq!(result.artifact_type, "a2a-task");
    assert_eq!(result.timestamp, "2026-03-01T00:00:00Z");
    assert_eq!(
        result.error.as_deref(),
        Some("signature verification failed")
    );
}

// ---------------------------------------------------------------------------
// TrustAssessment schema tests
// ---------------------------------------------------------------------------

#[test]
fn contract_trust_assessment_roundtrip_fields() {
    let contract = load_contract();
    let schema = &contract["trustAssessmentSchema"];
    let required = &schema["requiredFields"];

    let example = &contract["trustAssessmentExample"];
    let assessment: A2ATrustAssessment =
        serde_json::from_value(example.clone()).expect("deserialize trust assessment");

    let reserialized = serde_json::to_value(&assessment).expect("re-serialize trust assessment");

    assert_fields_present("A2ATrustAssessment", &reserialized, required);

    for (field, expected_type) in required.as_object().unwrap() {
        if field == "_comment" {
            continue;
        }
        assert_field_type(
            "A2ATrustAssessment",
            field,
            expected_type.as_str().unwrap(),
            &reserialized[field],
        );
    }

    // Verify camelCase field names.
    let obj = reserialized.as_object().unwrap();
    assert!(
        obj.contains_key("trustLevel"),
        "must use 'trustLevel' not 'trust_level'"
    );
    assert!(!obj.contains_key("trust_level"));
    assert!(
        obj.contains_key("jacsRegistered"),
        "must use 'jacsRegistered' not 'jacs_registered'"
    );
    assert!(!obj.contains_key("jacs_registered"));
    assert!(
        obj.contains_key("inTrustStore"),
        "must use 'inTrustStore' not 'in_trust_store'"
    );
    assert!(!obj.contains_key("in_trust_store"));

    // Check specific values.
    assert!(assessment.allowed);
    assert_eq!(assessment.trust_level, "jacs_verified");
    assert!(assessment.jacs_registered);
    assert!(!assessment.in_trust_store);
    assert_eq!(assessment.reason, "open policy: all agents accepted");
}

// ---------------------------------------------------------------------------
// AgentCard schema tests
// ---------------------------------------------------------------------------

#[test]
fn contract_agent_card_roundtrip_fields() {
    let contract = load_contract();
    let schema = &contract["agentCardSchema"];
    let required = &schema["requiredFields"];

    // Use the existing v04 card fixture.
    let card_fixture = load_a2a_fixture("agent_card.v04.json");
    let card: A2AAgentCard =
        serde_json::from_value(card_fixture.clone()).expect("deserialize agent card");

    let reserialized = serde_json::to_value(&card).expect("re-serialize agent card");

    assert_fields_present("A2AAgentCard", &reserialized, required);

    for (field, expected_type) in required.as_object().unwrap() {
        if field == "_comment" {
            continue;
        }
        assert_field_type(
            "A2AAgentCard",
            field,
            expected_type.as_str().unwrap(),
            &reserialized[field],
        );
    }

    // Verify camelCase field names.
    let obj = reserialized.as_object().unwrap();
    assert!(obj.contains_key("supportedInterfaces"));
    assert!(!obj.contains_key("supported_interfaces"));
    assert!(obj.contains_key("defaultInputModes"));
    assert!(!obj.contains_key("default_input_modes"));
    assert!(obj.contains_key("defaultOutputModes"));
    assert!(!obj.contains_key("default_output_modes"));

    // Verify skill sub-fields.
    let skills = reserialized["skills"].as_array().expect("skills array");
    assert!(!skills.is_empty());
    let skill_fields = &schema["skillFields"];
    assert_fields_present("A2AAgentSkill", &skills[0], skill_fields);

    // Verify extension uri field.
    let extensions = reserialized["capabilities"]["extensions"]
        .as_array()
        .expect("extensions array");
    assert!(!extensions.is_empty());
    assert!(
        extensions[0].get("uri").is_some(),
        "extension must have 'uri'"
    );
}

// ---------------------------------------------------------------------------
// ChainOfCustody schema tests
// ---------------------------------------------------------------------------

#[test]
fn contract_chain_of_custody_entry_fields() {
    let contract = load_contract();
    let schema = &contract["chainOfCustodySchema"];
    let entry_fields = &schema["entryFields"];

    let chain_fixture = load_a2a_fixture("golden_chain_of_custody.json");
    let expected = &chain_fixture["expected"];
    let entries = expected["entries"].as_array().expect("entries array");
    assert!(!entries.is_empty());

    assert_fields_present("A2AChainEntry", &entries[0], entry_fields);

    for (field, expected_type) in entry_fields.as_object().unwrap() {
        if field == "_comment" {
            continue;
        }
        assert_field_type(
            "A2AChainEntry",
            field,
            expected_type.as_str().unwrap(),
            &entries[0][field],
        );
    }

    // Verify camelCase field names.
    let entry = entries[0].as_object().unwrap();
    assert!(entry.contains_key("artifactId"));
    assert!(!entry.contains_key("artifact_id"));
    assert!(entry.contains_key("artifactType"));
    assert!(!entry.contains_key("artifact_type"));
    assert!(entry.contains_key("signaturePresent"));
    assert!(!entry.contains_key("signature_present"));
}
