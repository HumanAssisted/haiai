//! Fixture-driven tests for crypto_delegation_contract.json.
//!
//! Validates that the Rust SDK's canonicalization produces the same output as
//! the other SDKs, ensuring cross-language consistency.

use haiai::jacs::canonicalize_json_rfc8785;
use serde_json::Value;

#[derive(serde::Deserialize)]
struct CryptoDelegationFixture {
    description: String,
    canonicalization: CanonicalizationSection,
}

#[derive(serde::Deserialize)]
struct CanonicalizationSection {
    test_vectors: Vec<CanonicalizationVector>,
}

#[derive(serde::Deserialize)]
struct CanonicalizationVector {
    input: Value,
    expected: String,
}

fn load_fixture() -> CryptoDelegationFixture {
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/crypto_delegation_contract.json");
    let data = std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", fixture_path, e));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Failed to parse fixture: {}", e))
}

#[test]
fn crypto_delegation_fixture_loads() {
    let fixture = load_fixture();
    assert!(
        !fixture.description.is_empty(),
        "fixture should have a description"
    );
    assert!(
        !fixture.canonicalization.test_vectors.is_empty(),
        "fixture should have canonicalization test vectors"
    );
}

#[test]
fn canonicalization_matches_cross_language_vectors() {
    let fixture = load_fixture();

    for (i, vec) in fixture.canonicalization.test_vectors.iter().enumerate() {
        let result = canonicalize_json_rfc8785(&vec.input);
        assert_eq!(
            result, vec.expected,
            "canonicalization vector[{}]: input={}, expected={}, got={}",
            i, vec.input, vec.expected, result
        );
    }
}
