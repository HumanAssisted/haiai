//! Cross-SDK email conformance tests.
//!
//! Validates the Rust SDK against the shared `fixtures/email_conformance.json`
//! fixture to ensure structural equivalence with Go, Node, and Python SDKs.

#![cfg(feature = "jacs-crate")]

use haiai::{EmailVerificationResultV2, FieldStatus};
use serde_json::Value;
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
// Content hash golden vector conformance (TASK_013)
// ---------------------------------------------------------------------------

#[test]
fn conformance_content_hash_golden_vectors() {
    let fixture = load_conformance();
    let vectors = fixture["content_hash_golden"]["vectors"]
        .as_array()
        .expect("expected vectors array");

    for vector in vectors {
        let name = vector["name"].as_str().unwrap_or("unnamed");
        let subject = vector["subject"].as_str().unwrap();
        let body = vector["body"].as_str().unwrap();
        let expected_hash = vector["expected_hash"].as_str().unwrap();

        let attachments: Vec<haiai::AttachmentInput> = vector["attachments"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .map(|a| haiai::AttachmentInput {
                filename: a["filename"].as_str().unwrap().to_string(),
                content_type: a["content_type"].as_str().unwrap().to_string(),
                data: a["data_utf8"].as_str().unwrap().as_bytes().to_vec(),
            })
            .collect();

        let actual = haiai::compute_content_hash(subject, body, &attachments);
        assert_eq!(
            actual, expected_hash,
            "Content hash mismatch for vector '{}': expected {}, got {}",
            name, expected_hash, actual
        );
    }
}

// ---------------------------------------------------------------------------
// MIME round-trip conformance (TASK_014)
// ---------------------------------------------------------------------------

#[test]
fn conformance_mime_round_trip_content_hash() {
    let fixture = load_conformance();
    let rt = &fixture["mime_round_trip"];
    let input = &rt["input"];
    let expected_hash = rt["expected_content_hash"].as_str().unwrap();

    let subject = input["subject"].as_str().unwrap();
    let body = input["body"].as_str().unwrap();
    let attachments: Vec<haiai::AttachmentInput> = input["attachments"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|a| haiai::AttachmentInput {
            filename: a["filename"].as_str().unwrap().to_string(),
            content_type: a["content_type"].as_str().unwrap().to_string(),
            data: a["data_utf8"].as_str().unwrap().as_bytes().to_vec(),
        })
        .collect();

    let actual = haiai::compute_content_hash(subject, body, &attachments);
    assert_eq!(
        actual, expected_hash,
        "MIME round-trip content hash mismatch: expected {}, got {}",
        expected_hash, actual
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
