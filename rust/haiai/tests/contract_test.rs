//! Contract tests that validate the Rust SDK deserializes shared JSON fixtures
//! identically to every other SDK (Python, Node, Go). These fixtures live in
//! `haiai/contract/` and are the single source of truth for API shape.

use serde::Deserialize;
use sha2::{Digest, Sha256};

use haiai::types::{EmailMessage, EmailStatus, KeyRegistryResponse, PublicKeyInfo};

/// Wrapper struct for the `list_messages_response.json` contract.
/// The SDK client unpacks this internally, but the contract test validates
/// the full envelope shape that the API returns.
#[derive(Debug, Deserialize)]
struct ListMessagesResponse {
    messages: Vec<EmailMessage>,
    total: i64,
    unread: i64,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const EMAIL_MESSAGE_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contract/email_message.json"
));
const LIST_MESSAGES_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contract/list_messages_response.json"
));
const EMAIL_STATUS_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contract/email_status_response.json"
));
const CONTENT_HASH_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contract/content_hash_example.json"
));
const KEY_REGISTRY_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contract/key_registry_response.json"
));
const KEY_LOOKUP_VERSIONED_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../contract/key_lookup_versioned_response.json"
));

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn contract_deserialize_email_message() {
    let msg: EmailMessage =
        serde_json::from_str(EMAIL_MESSAGE_JSON).expect("EmailMessage deserialization failed");

    assert_eq!(msg.id, "550e8400-e29b-41d4-a716-446655440000");
    assert_eq!(msg.direction, "inbound");
    assert_eq!(msg.from_address, "sender@hai.ai");
    assert_eq!(msg.to_address, "recipient@hai.ai");
    assert_eq!(msg.subject, "Test Subject");
    assert_eq!(msg.body_text, "Hello, this is a test email body.");
    assert_eq!(msg.message_id.as_deref(), Some("<550e8400@hai.ai>"));
    assert!(msg.in_reply_to.is_none());
    assert!(!msg.is_read);
    assert_eq!(msg.delivery_status, "delivered");
    assert_eq!(msg.created_at, "2026-02-24T12:00:00Z");
    assert!(msg.read_at.is_none());
    assert_eq!(msg.jacs_verified, Some(true));
    assert!(
        (msg.trust_score.unwrap() - 92.4).abs() < 0.01,
        "trust_score should be ~92.4, got {:?}",
        msg.trust_score
    );
}

#[test]
fn contract_deserialize_list_messages_response() {
    let resp: ListMessagesResponse = serde_json::from_str(LIST_MESSAGES_JSON)
        .expect("ListMessagesResponse deserialization failed");

    assert_eq!(resp.messages.len(), 2);
    assert_eq!(resp.total, 2);
    assert_eq!(resp.unread, 1);

    // Spot-check the inbound message.
    let msg = &resp.messages[0];
    assert_eq!(msg.id, "550e8400-e29b-41d4-a716-446655440000");
    assert_eq!(msg.subject, "Test Subject");
    assert_eq!(msg.body_text, "Hello, this is a test email body.");
    assert!(
        (msg.trust_score.unwrap() - 92.4).abs() < 0.01,
        "inbound trust_score should be ~92.4"
    );

    // Outbound message omits trust_score.
    let outbound = &resp.messages[1];
    assert_eq!(outbound.id, "660e8400-e29b-41d4-a716-446655440001");
    assert_eq!(outbound.direction, "outbound");
    assert!(outbound.trust_score.is_none(), "outbound trust_score should be None");
}

#[test]
fn contract_deserialize_email_status() {
    let status: EmailStatus =
        serde_json::from_str(EMAIL_STATUS_JSON).expect("EmailStatus deserialization failed");

    assert_eq!(status.email, "testbot@hai.ai");
    assert_eq!(status.status, "active");
    assert_eq!(status.tier, "new");
    assert_eq!(status.billing_tier, "free");
    assert_eq!(status.messages_sent_24h, 5);
    assert_eq!(status.daily_limit, 10);
    assert_eq!(status.daily_used, 5);
    assert_eq!(status.resets_at, "2026-02-25T00:00:00Z");
    assert_eq!(status.messages_sent_total, 42);
    assert_eq!(status.external_enabled, false);
    assert_eq!(status.external_sends_today, 0);
    assert!(status.last_tier_change.is_none());
}

#[test]
fn contract_deserialize_key_registry_response() {
    let resp: KeyRegistryResponse = serde_json::from_str(KEY_REGISTRY_JSON)
        .expect("KeyRegistryResponse deserialization failed");

    assert_eq!(resp.email, "testbot@hai.ai");
    assert_eq!(resp.jacs_id, "test-agent-jacs-id");
    assert_eq!(
        resp.public_key,
        "MCowBQYDK2VwAyEAExampleBase64PublicKeyData1234567890ABCDEF"
    );
    assert_eq!(resp.algorithm, "ed25519");
    assert_eq!(resp.reputation_tier, "new");
    assert_eq!(resp.registered_at, "2026-01-15T00:00:00Z");
}

#[test]
fn contract_content_hash_computation() {
    let fixture: serde_json::Value =
        serde_json::from_str(CONTENT_HASH_JSON).expect("content_hash_example.json parse failed");

    let subject = fixture["subject"].as_str().expect("subject");
    let body = fixture["body"].as_str().expect("body");
    let expected_hash = fixture["expected_hash"].as_str().expect("expected_hash");

    // Compute sha256 using the same canonical format as HaiClient::send_email:
    //   sha256("{subject}\n{body}")
    let mut hasher = Sha256::new();
    hasher.update(subject.as_bytes());
    hasher.update(b"\n");
    hasher.update(body.as_bytes());
    let computed = format!("sha256:{:x}", hasher.finalize());

    assert_eq!(
        computed, expected_hash,
        "Content hash mismatch.\n  computed: {computed}\n  expected: {expected_hash}"
    );
}

#[test]
fn contract_sign_input_format() {
    let fixture: serde_json::Value =
        serde_json::from_str(CONTENT_HASH_JSON).expect("content_hash_example.json parse failed");

    let expected_hash = fixture["expected_hash"].as_str().expect("expected_hash");
    let from_email = fixture["from_email"].as_str().expect("from_email");
    let timestamp = fixture["timestamp"].as_i64().expect("timestamp");
    let expected_sign_input = fixture["sign_input_example"]
        .as_str()
        .expect("sign_input_example");

    let sign_input = format!("{expected_hash}:{from_email}:{timestamp}");

    assert_eq!(
        sign_input, expected_sign_input,
        "Sign input format mismatch.\n  computed: {sign_input}\n  expected: {expected_sign_input}"
    );
}

#[test]
fn contract_deserialize_key_lookup_versioned_response() {
    let fixture: serde_json::Value = serde_json::from_str(KEY_LOOKUP_VERSIONED_JSON)
        .expect("key_lookup_versioned_response.json parse failed");

    let resp_json = &fixture["response"];
    let info: PublicKeyInfo =
        serde_json::from_value(resp_json.clone()).expect("PublicKeyInfo deserialization failed");

    assert_eq!(
        info.jacs_id,
        "fixture-agent-00000000-0000-0000-0000-000000000001"
    );
    assert_eq!(
        info.version,
        "fixture-version-00000000-0000-0000-0000-000000000001"
    );
    assert!(
        info.public_key.starts_with("-----BEGIN PUBLIC KEY-----"),
        "public_key should be PEM-formatted"
    );
    assert!(
        info.public_key.ends_with("-----END PUBLIC KEY-----"),
        "public_key should end with PEM footer"
    );
    assert_eq!(info.algorithm, "ed25519");
    assert!(
        info.public_key_hash.starts_with("sha256:"),
        "public_key_hash should start with sha256:"
    );
    assert_eq!(
        info.public_key_hash.len(),
        7 + 64,
        "public_key_hash should be sha256: + 64 hex chars"
    );
    assert_eq!(info.status, "active");
    assert!(info.dns_verified);
    assert_eq!(info.created_at, "2026-01-01T00:00:00Z");
    assert!(
        !info.public_key_raw_b64.is_empty(),
        "public_key_raw_b64 should not be empty"
    );
}

#[test]
fn contract_trust_score_present() {
    let msg: EmailMessage =
        serde_json::from_str(EMAIL_MESSAGE_JSON).expect("EmailMessage deserialization failed");
    assert!(
        (msg.trust_score.unwrap() - 92.4).abs() < 0.01,
        "trust_score should be ~92.4, got {:?}",
        msg.trust_score
    );
}

#[test]
fn contract_trust_score_absent() {
    let resp: ListMessagesResponse = serde_json::from_str(LIST_MESSAGES_JSON)
        .expect("ListMessagesResponse deserialization failed");
    assert!(resp.messages.len() >= 2, "expected at least 2 messages");
    let outbound = &resp.messages[1];
    assert!(
        outbound.trust_score.is_none(),
        "outbound trust_score should be None"
    );
}

#[test]
fn contract_trust_score_round_trip() {
    // Build a minimal EmailMessage with trust_score set
    let json_with = r#"{"trust_score": 75.0}"#;
    let msg: EmailMessage = serde_json::from_str(json_with).expect("deser with trust_score");
    assert!(
        (msg.trust_score.unwrap() - 75.0).abs() < 0.01,
        "trust_score should be 75.0"
    );

    // Serialize and verify the key is present
    let serialized = serde_json::to_string(&msg).expect("serialize");
    assert!(
        serialized.contains("\"trust_score\""),
        "serialized JSON should contain trust_score key"
    );

    // Deserialize back
    let restored: EmailMessage = serde_json::from_str(&serialized).expect("deser round-trip");
    assert!(
        (restored.trust_score.unwrap() - 75.0).abs() < 0.01,
        "round-trip trust_score should be 75.0"
    );

    // Build a minimal EmailMessage without trust_score
    let json_without = r#"{}"#;
    let msg_none: EmailMessage = serde_json::from_str(json_without).expect("deser without trust_score");
    assert!(msg_none.trust_score.is_none(), "absent trust_score should be None");

    // Verify absent trust_score is not serialized
    let serialized_none = serde_json::to_string(&msg_none).expect("serialize none");
    assert!(
        !serialized_none.contains("trust_score"),
        "serialized JSON should not contain trust_score key when None"
    );
}
