//! Cross-SDK email conformance tests.
//!
//! Validates the Rust SDK against the shared `fixtures/email_conformance.json`
//! fixture to ensure structural equivalence with Go, Node, and Python SDKs.

#![cfg(feature = "jacs-crate")]

use base64::Engine;
use haiai::{EmailVerificationResultV2, FieldStatus, HaiClient, HaiClientOptions, StaticJacsProvider};
use httpmock::{Method::GET, MockServer};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures")
}

fn load_conformance() -> Value {
    let path = fixtures_dir().join("email_conformance.json");
    let data = fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "Failed to read email_conformance.json at {}: {e}",
            path.display()
        )
    });
    serde_json::from_str(&data).expect("Failed to parse email_conformance.json")
}

// ---------------------------------------------------------------------------
// EmailVerificationResultV2 structural conformance
// ---------------------------------------------------------------------------

#[test]
fn conformance_mock_verify_response_deserialization() {
    let fixture = load_conformance();
    let mock_json = &fixture["mock_verify_response"]["json"];

    let result: EmailVerificationResultV2 =
        serde_json::from_value(mock_json.clone()).expect("Failed to deserialize mock response");

    assert!(result.valid, "expected valid=true");
    assert_eq!(result.jacs_id, "conformance-test-agent-001");
    assert_eq!(result.algorithm, "ed25519");
    assert_eq!(result.reputation_tier, "established");
    assert_eq!(result.dns_verified, Some(true));
    assert!(result.error.is_none(), "expected error=None");

    // field_results
    assert_eq!(result.field_results.len(), 4, "expected 4 field_results");
    assert_eq!(result.field_results[0].field, "subject");
    assert_eq!(result.field_results[0].status, FieldStatus::Pass);
    assert_eq!(result.field_results[3].field, "date");
    assert_eq!(result.field_results[3].status, FieldStatus::Modified);

    // chain
    assert_eq!(result.chain.len(), 1, "expected 1 chain entry");
    assert_eq!(result.chain[0].signer, "agent@hai.ai");
    assert_eq!(result.chain[0].jacs_id, "conformance-test-agent-001");
    assert!(result.chain[0].valid);
    assert!(!result.chain[0].forwarded);

    // agent_status and benchmarks_completed (TASK_012)
    assert_eq!(
        result.agent_status,
        Some("active".to_string()),
        "expected agent_status='active'"
    );
    assert_eq!(
        result.benchmarks_completed,
        vec!["free_chaotic".to_string()],
        "expected benchmarks_completed=['free_chaotic']"
    );
}

// ---------------------------------------------------------------------------
// FieldStatus enum conformance
// ---------------------------------------------------------------------------

#[test]
fn conformance_field_status_values() {
    let fixture = load_conformance();
    let expected_values: Vec<String> = fixture["verification_result_v2_schema"]
        ["field_status_values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();

    // Verify all expected values can be deserialized into FieldStatus
    for val in &expected_values {
        let json = format!("\"{}\"", val);
        let status: FieldStatus = serde_json::from_str(&json)
            .unwrap_or_else(|_| panic!("FieldStatus {:?} not valid", val));
        // Verify round-trip
        let serialized = serde_json::to_string(&status).unwrap();
        assert_eq!(
            serialized, json,
            "FieldStatus round-trip failed for {}",
            val
        );
    }

    // Verify count matches: pass, modified, fail, unverifiable = 4
    assert_eq!(
        expected_values.len(),
        4,
        "Expected 4 FieldStatus values, got {}",
        expected_values.len()
    );
}

// ---------------------------------------------------------------------------
// EmailVerificationResultV2 round-trip serialization
// ---------------------------------------------------------------------------

#[test]
fn conformance_verification_result_round_trip() {
    let fixture = load_conformance();
    let mock_json = &fixture["mock_verify_response"]["json"];

    // Deserialize
    let result: EmailVerificationResultV2 =
        serde_json::from_value(mock_json.clone()).expect("Failed to deserialize");

    // Re-serialize
    let serialized = serde_json::to_value(&result).expect("Failed to serialize");

    // Key fields must survive round-trip
    assert_eq!(serialized["valid"], mock_json["valid"]);
    assert_eq!(serialized["jacs_id"], mock_json["jacs_id"]);
    assert_eq!(serialized["algorithm"], mock_json["algorithm"]);
    assert_eq!(serialized["reputation_tier"], mock_json["reputation_tier"]);
    assert_eq!(serialized["dns_verified"], mock_json["dns_verified"]);

    // field_results count
    assert_eq!(
        serialized["field_results"].as_array().unwrap().len(),
        mock_json["field_results"].as_array().unwrap().len()
    );
}

// ---------------------------------------------------------------------------
// Error type conformance
// ---------------------------------------------------------------------------

#[test]
fn conformance_error_types_exist() {
    use haiai::HaiError;

    // Verify the error types referenced in the conformance fixture can be constructed
    let e1 = HaiError::Provider("email not active".to_string());
    let e2 = HaiError::Provider("recipient not found".to_string());
    let e3 = HaiError::Provider("rate limited".to_string());

    // They should all implement Display/Error
    assert!(!format!("{}", e1).is_empty());
    assert!(!format!("{}", e2).is_empty());
    assert!(!format!("{}", e3).is_empty());
}

// ---------------------------------------------------------------------------
// raw_email_roundtrip conformance (PRD §5.4, Issue 003)
//
// Two assertions per the PRD:
//   1. get_raw_email returns bytes byte-identical to the fixture's
//      input_raw_b64 (R2 byte-fidelity).
//   2. verify_email(fetched_bytes).valid == true when the HAI registry is
//      mocked with the fixture's `verify_registry` data.
//
// This is the ONLY consumer across the 4 SDKs that actually exercises the
// full JACS crypto verify path against real signed bytes. Python / Node / Go
// consumers mock FFI output, so they cannot catch a signature-verify
// regression inside JACS itself.
// ---------------------------------------------------------------------------

fn make_test_client(base_url: &str, jacs_id: &str) -> HaiClient<StaticJacsProvider> {
    let provider = StaticJacsProvider::new(jacs_id);
    HaiClient::new(
        provider,
        HaiClientOptions {
            base_url: base_url.to_string(),
            ..HaiClientOptions::default()
        },
    )
    .expect("build test client")
}

#[tokio::test]
async fn raw_email_roundtrip_fixture_byte_identity_and_verify_valid() {
    let fixture = load_conformance();
    let scenario = &fixture["raw_email_roundtrip"];

    // Issue 017: Rust is the only SDK declared as running real JACS crypto
    // verify in the shared fixture. If this key changes, update the PRD §5.4
    // contract and the non-Rust SDK tests.
    assert_eq!(
        scenario["verify_implemented_by"].as_str(),
        Some("rust_only"),
        "fixture verify_implemented_by must be \"rust_only\" (Issue 017)"
    );

    let expected_b64 = scenario["input_raw_b64"]
        .as_str()
        .expect("input_raw_b64");
    let expected_bytes = base64::engine::general_purpose::STANDARD
        .decode(expected_b64)
        .expect("decode input_raw_b64");

    // Belt-and-braces: sha256 matches fixture.
    let sha_hex = format!("{:x}", Sha256::digest(&expected_bytes));
    assert_eq!(
        sha_hex,
        scenario["input_sha256"].as_str().expect("input_sha256"),
        "input_sha256 mismatch — fixture corrupted or regen needed"
    );

    let registry = scenario["verify_registry"].clone();
    let registry_email = registry["email"].as_str().expect("email");
    let registry_jacs_id = registry["jacs_id"].as_str().expect("jacs_id");
    let registry_algorithm = registry["algorithm"].as_str().expect("algorithm");
    let registry_public_key = registry["public_key"].as_str().expect("public_key");

    let server = MockServer::start_async().await;

    // Mock the raw-email endpoint.
    let raw_mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path_includes("/email/messages/")
                .path_includes("/raw");
            then.status(200).json_body(serde_json::json!({
                "message_id": "conf-001",
                "available": scenario["expected_available"],
                "raw_email_b64": scenario["expected_raw_b64"],
                "size_bytes": scenario["expected_size_bytes"],
                "omitted_reason": scenario["expected_omitted_reason"],
            }));
        })
        .await;

    // Mock the HAI key registry for the sender email. The path pattern
    // matches `fetch_public_key_from_registry`: GET /api/agents/keys/{email}.
    let registry_mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path_includes("/api/agents/keys/");
            then.status(200).json_body(serde_json::json!({
                "email": registry_email,
                "jacs_id": registry_jacs_id,
                "algorithm": registry_algorithm,
                "public_key": registry_public_key,
                "reputation_tier": registry["reputation_tier"],
                "agent_status": registry["agent_status"],
                "benchmarks_completed": registry["benchmarks_completed"],
                "registered_at": registry["registered_at"],
            }));
        })
        .await;

    // Step 1: fetch raw bytes.
    let client = make_test_client(&server.base_url(), "conf-agent");
    let resp = client
        .get_raw_email("conf-001")
        .await
        .expect("get_raw_email");
    raw_mock.assert_async().await;

    // Assertion 1 (PRD §5.4): full byte equality.
    assert!(resp.available);
    assert_eq!(
        resp.raw_email.as_deref(),
        Some(expected_bytes.as_slice()),
        "byte-identity broken: get_raw_email did not return the fixture bytes verbatim"
    );
    assert_eq!(
        resp.size_bytes,
        scenario["expected_size_bytes"].as_u64().map(|n| n as usize),
    );

    // Step 2: verify_email against the same mocked registry.
    let raw = resp.raw_email.expect("raw bytes present");
    let verify = haiai::verify_email(&raw, &server.base_url()).await;
    registry_mock.assert_async().await;

    // Assertion 2 (PRD §5.4): signature verification must succeed.
    assert!(
        verify.valid,
        "verify_email returned valid=false for fixture bytes: error={:?}, field_results={:?}, chain={:?}",
        verify.error, verify.field_results, verify.chain
    );
    assert_eq!(verify.jacs_id, registry_jacs_id);
    assert_eq!(
        scenario["expected_verify_valid"].as_bool(),
        Some(true),
        "fixture must declare expected_verify_valid: true"
    );
}

#[tokio::test]
async fn raw_email_not_stored_fixture_matches_available_false() {
    let fixture = load_conformance();
    let scenario = &fixture["raw_email_not_stored"];

    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path_includes("/email/messages/")
                .path_includes("/raw");
            then.status(200).json_body(serde_json::json!({
                "message_id": "legacy",
                "available": scenario["expected_available"],
                "raw_email_b64": scenario["expected_raw_b64"],
                "size_bytes": scenario["expected_size_bytes"],
                "omitted_reason": scenario["expected_omitted_reason"],
            }));
        })
        .await;

    let client = make_test_client(&server.base_url(), "a");
    let resp = client.get_raw_email("legacy").await.expect("ok");
    mock.assert_async().await;

    assert!(!resp.available);
    assert_eq!(resp.raw_email, None);
    assert_eq!(resp.omitted_reason.as_deref(), Some("not_stored"));
    assert_eq!(resp.size_bytes, None);
}

#[tokio::test]
async fn raw_email_oversize_fixture_matches_available_false() {
    let fixture = load_conformance();
    let scenario = &fixture["raw_email_oversize"];

    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path_includes("/email/messages/")
                .path_includes("/raw");
            then.status(200).json_body(serde_json::json!({
                "message_id": "big",
                "available": scenario["expected_available"],
                "raw_email_b64": scenario["expected_raw_b64"],
                "size_bytes": scenario["expected_size_bytes"],
                "omitted_reason": scenario["expected_omitted_reason"],
            }));
        })
        .await;

    let client = make_test_client(&server.base_url(), "a");
    let resp = client.get_raw_email("big").await.expect("ok");
    mock.assert_async().await;

    assert!(!resp.available);
    assert_eq!(resp.raw_email, None);
    assert_eq!(resp.omitted_reason.as_deref(), Some("oversize"));
}

/// Issue 012 conformance: the reconstructed-source sentinel flows
/// through the fixture → mock server → SDK decode pipeline with the
/// `omitted_reason: "reconstructed"` signal intact so cross-language
/// SDKs all see the same wire shape.
#[tokio::test]
async fn raw_email_reconstructed_fixture_matches_available_false() {
    let fixture = load_conformance();
    let scenario = &fixture["raw_email_reconstructed"];

    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path_includes("/email/messages/")
                .path_includes("/raw");
            then.status(200).json_body(serde_json::json!({
                "message_id": "recon",
                "available": scenario["expected_available"],
                "raw_email_b64": scenario["expected_raw_b64"],
                "size_bytes": scenario["expected_size_bytes"],
                "omitted_reason": scenario["expected_omitted_reason"],
            }));
        })
        .await;

    let client = make_test_client(&server.base_url(), "a");
    let resp = client.get_raw_email("recon").await.expect("ok");
    mock.assert_async().await;

    assert!(!resp.available);
    assert_eq!(resp.raw_email, None);
    assert_eq!(resp.omitted_reason.as_deref(), Some("reconstructed"));
    assert_eq!(resp.size_bytes, None);
}
