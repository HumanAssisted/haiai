//! `RemoteJacsProvider` end-to-end integration test against the hosted-stack
//! Docker compose (TASK_012).
//!
//! All tests in this file are `#[ignore]` by default — they require a live
//! hosted stack with `HAI_URL` pointing at it. Run with:
//!
//!     cargo test -p haiai --test jacs_remote_integration -- --ignored
//!
//! Per `~/.claude/projects/.../MEMORY.md::no_test_hacks` the tests exercise
//! real production paths — no FFI bypasses, no mocks, no monkey-patches. If
//! the SDK is missing a parameter, the SDK gets fixed.
//!
//! The hosted stack must:
//!   - run the api with `HAI_JACSDB_BUCKET=hai-jacsdb-test`
//!   - run localstack S3 reachable at the api's `AWS_ENDPOINT_URL`
//!   - register the test agent's public key via the standard
//!     `HaiClient::register` path
//!
//! Per `MEMORY.md::test_cleanup_required`, every test that creates records
//! tombstones them in a teardown so principals don't accumulate across runs.

use haiai::HaiError;
use haiai::config::resolve_storage_backend_label;
use haiai::jacs::{JacsDocumentProvider, JacsProvider};
use haiai::jacs_local::LocalJacsProvider;
use haiai::jacs_remote::{RemoteJacsProvider, RemoteJacsProviderOptions};

const TEST_BASE_URL_ENV: &str = "HAI_JACS_REMOTE_TEST_URL";

/// Issue 009: build a `RemoteJacsProvider` wrapping a real `LocalJacsProvider`
/// (real Ed25519 keys), not the previous `StaticJacsProvider` whose
/// `sign_string` returns fake `"sig:..."` bytes that fail server verification.
///
/// Pre-condition: the test rig must (1) generate the test agent's keys via
/// `LocalJacsProvider::create_agent`, (2) register them via
/// `HaiClient::register`, and (3) point `JACS_CONFIG_PATH` at the resulting
/// `jacs.config.json`. The hosted-stack `--profile hosted` Docker compose
/// from Issue 015 plus the `HOSTED_STACK_LOCAL.md` setup walk-through cover
/// these steps.
fn build_provider() -> Result<RemoteJacsProvider<LocalJacsProvider>, HaiError> {
    let base_url = std::env::var(TEST_BASE_URL_ENV)
        .or_else(|_| std::env::var("HAI_URL"))
        .expect("set HAI_JACS_REMOTE_TEST_URL or HAI_URL to point at the hosted stack");
    let local = LocalJacsProvider::from_config_path(None, None).map_err(|e| {
        HaiError::Provider(format!(
            "Issue 009: integration tests now require a real LocalJacsProvider — \
             generate test agent keys via LocalJacsProvider::create_agent and \
             register them with HaiClient::register before running --ignored. \
             Underlying error: {e}"
        ))
    })?;
    RemoteJacsProvider::new(
        local,
        RemoteJacsProviderOptions {
            base_url,
            ..Default::default()
        },
    )
}

// =============================================================================
// SDK ↔ server round-trip — generic CRUD
// =============================================================================

#[test]
#[ignore = "requires live hosted stack — run with: cargo test --test jacs_remote_integration -- --ignored"]
fn sign_and_store_round_trip() {
    let provider = build_provider().expect("provider");
    let value = serde_json::json!({"test": "round-trip", "marker": "alpha-beta-gamma"});
    let signed = provider.sign_and_store(&value).expect("sign+store");
    assert!(signed.key.contains(':'), "key shape is id:version");
    let recovered = provider.get_document(&signed.key).expect("get");
    assert!(recovered.contains("alpha-beta-gamma"));
    // Cleanup
    let _ = provider.remove_document(&signed.key);
}

#[test]
#[ignore = "requires live hosted stack"]
fn list_documents_filters_by_type() {
    let provider = build_provider().expect("provider");
    let key1 = provider
        .sign_and_store(&serde_json::json!({"jacsType": "test-list-A", "i": 1}))
        .expect("store");
    let key2 = provider
        .sign_and_store(&serde_json::json!({"jacsType": "test-list-A", "i": 2}))
        .expect("store");
    let key3 = provider
        .sign_and_store(&serde_json::json!({"jacsType": "test-list-B", "i": 3}))
        .expect("store");

    let a_keys = provider.list_documents(Some("test-list-A")).expect("list a");
    assert!(a_keys.iter().any(|k| k == &key1.key));
    assert!(a_keys.iter().any(|k| k == &key2.key));
    assert!(!a_keys.iter().any(|k| k == &key3.key));

    let _ = provider.remove_document(&key1.key);
    let _ = provider.remove_document(&key2.key);
    let _ = provider.remove_document(&key3.key);
}

#[test]
#[ignore = "requires live hosted stack"]
fn search_documents_finds_marker_text() {
    let provider = build_provider().expect("provider");
    let signed = provider
        .sign_and_store(&serde_json::json!({
            "jacsType": "search-test",
            "body": "marker-search-zzy123 hello world"
        }))
        .expect("store");

    let results = provider
        .search_documents("marker-search-zzy123", 10, 0)
        .expect("search");
    assert!(!results.results.is_empty());
    let _ = provider.remove_document(&signed.key);
}

#[test]
#[ignore = "requires live hosted stack"]
fn query_by_agent_self_returns_own_docs() {
    let provider = build_provider().expect("provider");
    let signed = provider
        .sign_and_store(&serde_json::json!({"jacsType": "self-query"}))
        .expect("store");
    let keys = provider.query_by_agent(provider.jacs_id(), 10, 0).expect("qba");
    assert!(keys.iter().any(|k| k == &signed.key));
    let _ = provider.remove_document(&signed.key);
}

#[test]
#[ignore = "requires live hosted stack"]
fn query_by_agent_other_returns_provider_error_d4() {
    let provider = build_provider().expect("provider");
    // D4: server rejects with 400 "search is owner-scoped..."
    let err = provider
        .query_by_agent("not-the-caller", 10, 0)
        .expect_err("must reject");
    let msg = format!("{err:?}");
    assert!(msg.contains("owner-scoped") || msg.contains("400"), "got: {msg}");
}

#[test]
#[ignore = "requires live hosted stack"]
fn remove_document_then_get_returns_error() {
    let provider = build_provider().expect("provider");
    let signed = provider
        .sign_and_store(&serde_json::json!({"jacsType": "remove-test"}))
        .expect("store");
    provider.remove_document(&signed.key).expect("remove");
    let res = provider.get_document(&signed.key);
    assert!(res.is_err(), "get after remove must fail");
}

#[test]
#[ignore = "requires live hosted stack"]
fn storage_capabilities_reports_remote_capabilities() {
    let provider = build_provider().expect("provider");
    let caps = provider.storage_capabilities().expect("caps");
    assert!(caps.fulltext);
    assert!(caps.query_by_field);
    assert!(caps.query_by_type);
    assert!(caps.pagination);
    assert!(caps.tombstone);
    assert!(!caps.vector, "vector search is deferred to a future PRD");
}

// =============================================================================
// D5 — MEMORY / SOUL wrapper round-trip
// =============================================================================

#[test]
#[ignore = "requires live hosted stack"]
fn save_memory_get_memory_round_trip() {
    let provider = build_provider().expect("provider");
    let body = "# MEMORY.md\n\nproject foo / marker-mem-zzy987";
    let key = provider.save_memory(Some(body)).expect("save_memory");
    let envelope = provider
        .get_memory()
        .expect("get_memory")
        .expect("Some envelope");
    assert!(envelope.contains("marker-mem-zzy987"));
    // Cleanup
    let _ = provider.remove_document(&key);
}

#[test]
#[ignore = "requires live hosted stack"]
fn save_soul_get_soul_round_trip() {
    let provider = build_provider().expect("provider");
    let body = "# SOUL.md\n\nvoice: marker-soul-pqx456";
    let key = provider.save_soul(Some(body)).expect("save_soul");
    let envelope = provider
        .get_soul()
        .expect("get_soul")
        .expect("Some envelope");
    assert!(envelope.contains("marker-soul-pqx456"));
    let _ = provider.remove_document(&key);
}

#[test]
#[ignore = "requires live hosted stack"]
fn save_memory_appears_in_query_by_type_memory() {
    let provider = build_provider().expect("provider");
    let key = provider
        .save_memory(Some("memory body"))
        .expect("save_memory");
    let keys = provider.query_by_type("memory", 100, 0).expect("query");
    assert!(keys.iter().any(|k| k == &key));
    let _ = provider.remove_document(&key);
}

// =============================================================================
// D9 — Signed-text + signed-image round-trip via the SDK helpers
//
// These tests require `LocalJacsProvider` to call `sign_text_file` /
// `sign_image` so the bytes carry a real JACS signature the server will
// accept. The hosted-stack test rig must register the test agent's public
// key via `HaiClient::register` — pre-condition documented in
// `~/personal/haisdk/.../docs/HOSTED_STACK_LOCAL.md`.
// =============================================================================

#[test]
#[ignore = "requires live hosted stack with LocalJacsProvider keys provisioned"]
fn store_text_file_unsigned_rejects_locally() {
    // The SDK helper must refuse to upload an unsigned MD file BEFORE making
    // any HTTP request. This test covers the local rejection path; the
    // `httpmock` unit tests in `jacs_remote.rs::tests` already pin the
    // zero-HTTP-requests assertion.
    let provider = build_provider().expect("provider");
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("unsigned.md");
    std::fs::write(&path, b"hello world (no signature block)").expect("write");
    let err = provider
        .store_text_file(path.to_str().unwrap())
        .expect_err("must reject");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("no JACS signature block"),
        "expected rejection on unsigned md, got: {msg}"
    );
}

#[test]
#[ignore = "requires live hosted stack"]
fn store_image_file_unsigned_rejects_locally() {
    // Same: the SDK refuses to upload an unsigned PNG without an HTTP call.
    let provider = build_provider().expect("provider");
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("unsigned.png");
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend_from_slice(b"...no jacs chunk here...");
    std::fs::write(&path, &bytes).expect("write");
    let err = provider
        .store_image_file(path.to_str().unwrap())
        .expect_err("must reject");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("no JACS signature"),
        "expected rejection on unsigned png, got: {msg}"
    );
}

#[test]
fn config_remote_storage_label_resolves_without_hosted_stack() {
    // Doesn't need the hosted stack — pure config plumbing test.
    // Confirms `config::resolve_storage_backend_label` accepts "remote"
    // (TASK_010 already shipped this; we re-pin it from the SDK integration
    // boundary so a future regression here surfaces as a TASK_012 failure
    // instead of getting buried in TASK_010's quieter test surface).
    let label = resolve_storage_backend_label("remote").expect("remote label resolves");
    assert_eq!(label, "remote");
    // Empty label is rejected with a typed `ConfigInvalid` — pin that posture.
    assert!(
        resolve_storage_backend_label("").is_err(),
        "empty label must be rejected by resolve_storage_backend_label"
    );
}
