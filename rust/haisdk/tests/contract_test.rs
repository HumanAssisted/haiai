//! Contract tests that validate the Rust SDK deserializes shared JSON fixtures
//! identically to every other SDK (Python, Node, Go). These fixtures live in
//! `haisdk/contract/` and are the single source of truth for API shape.

use serde::Deserialize;
use sha2::{Digest, Sha256};

use haisdk::types::{EmailMessage, EmailStatus, KeyRegistryResponse, EmailVerificationResult};

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

const EMAIL_MESSAGE_JSON: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../contract/email_message.json"));
const LIST_MESSAGES_JSON: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../contract/list_messages_response.json"));
const EMAIL_STATUS_JSON: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../contract/email_status_response.json"));
const CONTENT_HASH_JSON: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../contract/content_hash_example.json"));
const KEY_REGISTRY_JSON: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../contract/key_registry_response.json"));
const VERIFICATION_RESULT_JSON: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../contract/verification_result.json"));

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
}

#[test]
fn contract_deserialize_list_messages_response() {
    let resp: ListMessagesResponse =
        serde_json::from_str(LIST_MESSAGES_JSON).expect("ListMessagesResponse deserialization failed");

    assert_eq!(resp.messages.len(), 1);
    assert_eq!(resp.total, 1);
    assert_eq!(resp.unread, 1);

    // Spot-check the embedded message matches the standalone fixture.
    let msg = &resp.messages[0];
    assert_eq!(msg.id, "550e8400-e29b-41d4-a716-446655440000");
    assert_eq!(msg.subject, "Test Subject");
    assert_eq!(msg.body_text, "Hello, this is a test email body.");
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
    let resp: KeyRegistryResponse =
        serde_json::from_str(KEY_REGISTRY_JSON).expect("KeyRegistryResponse deserialization failed");

    assert_eq!(resp.email, "testbot@hai.ai");
    assert_eq!(resp.jacs_id, "test-agent-jacs-id");
    assert_eq!(resp.public_key, "MCowBQYDK2VwAyEAExampleBase64PublicKeyData1234567890ABCDEF");
    assert_eq!(resp.algorithm, "ed25519");
    assert_eq!(resp.reputation_tier, "new");
    assert_eq!(resp.registered_at, "2026-01-15T00:00:00Z");
}

#[test]
fn contract_deserialize_verification_result() {
    let result: EmailVerificationResult =
        serde_json::from_str(VERIFICATION_RESULT_JSON).expect("EmailVerificationResult deserialization failed");

    assert_eq!(result.valid, true);
    assert_eq!(result.jacs_id, "test-agent-jacs-id");
    assert_eq!(result.reputation_tier, "established");
    assert!(result.error.is_none());
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
    let expected_sign_input = fixture["sign_input_example"].as_str().expect("sign_input_example");

    let sign_input = format!("{expected_hash}:{from_email}:{timestamp}");

    assert_eq!(
        sign_input, expected_sign_input,
        "Sign input format mismatch.\n  computed: {sign_input}\n  expected: {expected_sign_input}"
    );
}
