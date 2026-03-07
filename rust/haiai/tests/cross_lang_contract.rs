#![cfg(any(feature = "jacs-crate", feature = "jacs-local"))]

use base64::Engine;
use haiai::{HaiClient, HaiClientOptions, StaticJacsProvider};
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct CrossLangFixture {
    auth_header: AuthHeaderFixture,
    canonical_json_cases: Vec<CanonicalJsonCase>,
}

#[derive(Debug, Deserialize)]
struct AuthHeaderFixture {
    scheme: String,
    parts: Vec<String>,
    signed_message_template: String,
    example: AuthHeaderExample,
}

#[derive(Debug, Deserialize)]
struct AuthHeaderExample {
    jacs_id: String,
    timestamp: i64,
}

#[derive(Debug, Deserialize)]
struct CanonicalJsonCase {
    name: String,
    input: Value,
    expected: String,
}

fn load_fixture() -> CrossLangFixture {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/cross_lang_test.json");
    let raw = fs::read_to_string(path).expect("read cross_lang_test fixture");
    serde_json::from_str(&raw).expect("decode cross_lang_test fixture")
}

#[test]
fn canonical_json_matches_shared_cases() {
    let fixture = load_fixture();
    let client = HaiClient::new(
        StaticJacsProvider::new("fixture-agent"),
        HaiClientOptions::default(),
    )
    .expect("client");

    for case in fixture.canonical_json_cases {
        let got = client.canonical_json(&case.input).expect("canonical json");
        assert_eq!(got, case.expected, "case {}", case.name);
    }
}

#[test]
fn auth_header_matches_shared_shape() {
    let fixture = load_fixture();
    let client = HaiClient::new(
        StaticJacsProvider::new(fixture.auth_header.example.jacs_id.clone()),
        HaiClientOptions::default(),
    )
    .expect("client");

    let header = client.build_auth_header().expect("auth header");
    let token = header.strip_prefix("JACS ").expect("auth header prefix");
    let parts: Vec<&str> = token.splitn(3, ':').collect();

    assert_eq!(fixture.auth_header.scheme, "JACS");
    assert_eq!(
        fixture.auth_header.parts,
        vec!["jacs_id", "timestamp", "signature_base64"]
    );
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], fixture.auth_header.example.jacs_id);
    assert_eq!(
        fixture.auth_header.signed_message_template,
        "{jacs_id}:{timestamp}"
    );

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(parts[2])
        .expect("decode static provider signature");
    let signed_message = String::from_utf8(decoded).expect("utf8 signature payload");
    assert_eq!(signed_message, format!("sig:{}:{}", parts[0], parts[1]));

    let parsed_timestamp = parts[1].parse::<i64>().expect("timestamp");
    assert!(
        parsed_timestamp >= fixture.auth_header.example.timestamp,
        "timestamp should be unix seconds"
    );
}
