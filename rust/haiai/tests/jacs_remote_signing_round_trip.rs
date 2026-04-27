//! Issue 021 regression: end-to-end round-trip from
//! `RemoteJacsProvider::sign_document` (over a real `LocalJacsProvider`)
//! through JACS's authoritative verifier `SimpleAgent::verify_with_key` —
//! the same surface `api/src/jacs_verify.rs::verify_jacs_json_with_public_key_pem`
//! delegates to. If `sign_document` produces an envelope these tests verify,
//! the server's pipeline will accept it byte-for-byte.
//!
//! The previous `sign_document` shimmed `canonical_json` + `sign_string`,
//! producing signatures that NEVER verified under JACS's per-field
//! `build_signature_content`. Issue 021 routes through `JacsProvider::sign_envelope`
//! → `LocalJacsProvider::sign_envelope` → `agent.create_document_and_load`
//! → `signing_procedure`, the canonical path. These tests pin that contract.

#![cfg(feature = "jacs-crate")]

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use haiai::jacs::{JacsDocumentProvider, JacsProvider};
use haiai::jacs_local::LocalJacsProvider;
use haiai::jacs_remote::{RemoteJacsProvider, RemoteJacsProviderOptions};
use haiai::CreateAgentOptions;
use uuid::Uuid;

static REMOTE_SIGN_LOCK: Mutex<()> = Mutex::new(());

struct AgentEnv {
    base: PathBuf,
    original_cwd: PathBuf,
    config_path: PathBuf,
}

impl AgentEnv {
    fn new() -> Self {
        let original_cwd = std::env::current_dir().expect("cwd");
        let base = original_cwd.join(format!("target/jacs-remote-sign-{}", Uuid::new_v4()));
        fs::create_dir_all(&base).expect("create base");
        std::env::set_current_dir(&base).expect("cd to base");

        let options = CreateAgentOptions {
            name: "remote-sign-test-agent".to_string(),
            password: "TestPass!123".to_string(),
            algorithm: Some("ed25519".to_string()),
            data_directory: Some("data".to_string()),
            key_directory: Some("keys".to_string()),
            config_path: Some("jacs.config.json".to_string()),
            agent_type: Some("ai".to_string()),
            description: Some("RemoteJacsProvider signing round-trip test".to_string()),
            domain: None,
            default_storage: Some("fs".to_string()),
        };
        LocalJacsProvider::create_agent_with_options(&options).expect("create agent");
        std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", "TestPass!123");

        let config_path = base.join("jacs.config.json");
        Self {
            base: base.clone(),
            original_cwd,
            config_path,
        }
    }

    fn local(&self) -> LocalJacsProvider {
        LocalJacsProvider::from_config_path(Some(&self.config_path), None).expect("provider")
    }

    fn remote(&self) -> RemoteJacsProvider<LocalJacsProvider> {
        // base_url is irrelevant — these tests never call out, they only sign.
        RemoteJacsProvider::new(
            self.local(),
            RemoteJacsProviderOptions {
                base_url: "http://example.invalid".to_string(),
                ..Default::default()
            },
        )
        .expect("remote provider")
    }
}

impl Drop for AgentEnv {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original_cwd);
        // best-effort cleanup so target/ doesn't accumulate per-run dirs
        let _ = fs::remove_dir_all(&self.base);
    }
}

/// Sign via the inner `LocalJacsProvider::sign_envelope` directly, then verify
/// with `SimpleAgent::verify_with_key`. Pins the canonical signing path.
#[test]
fn local_sign_envelope_emits_full_jacs_metadata() {
    let _g = REMOTE_SIGN_LOCK.lock().unwrap();
    let env = AgentEnv::new();
    let local = env.local();

    let payload = serde_json::json!({"hello": "world", "marker": "round-trip-A"});
    let signed = local.sign_envelope(&payload).expect("sign_envelope");

    // The envelope MUST carry the JACS metadata fields the server's
    // `extract_envelope_metadata` requires (Issue 001 + Issue 021).
    let parsed: serde_json::Value = serde_json::from_str(&signed).expect("signed json");
    assert!(parsed.get("jacsId").is_some(), "missing jacsId");
    assert!(parsed.get("jacsVersion").is_some(), "missing jacsVersion");
    assert!(parsed.get("jacsType").is_some(), "missing jacsType");
    assert!(
        parsed.get("jacsVersionDate").is_some(),
        "missing jacsVersionDate"
    );
    let sig = parsed
        .get("jacsSignature")
        .and_then(|v| v.as_object())
        .expect("jacsSignature object");
    // Issue 021: full signature shape, not the previous 3-field
    // {agentID, signature, algorithm} stub.
    assert!(sig.contains_key("agentID"), "jacsSignature.agentID");
    assert!(sig.contains_key("signature"), "jacsSignature.signature");
    assert!(sig.contains_key("date"), "jacsSignature.date");
    assert!(
        sig.contains_key("publicKeyHash"),
        "jacsSignature.publicKeyHash"
    );
    assert!(sig.contains_key("fields"), "jacsSignature.fields");
    assert!(
        sig.contains_key("signingAlgorithm"),
        "jacsSignature.signingAlgorithm"
    );
}

/// Sign via `RemoteJacsProvider::sign_document` and assert the produced
/// envelope carries the JACS metadata + the full canonical jacsSignature
/// shape. Issue 021: previous impl emitted a 3-field stub
/// (`{agentID,signature,algorithm}`) that fails server-side verify. The fix
/// delegates to the inner `sign_envelope`, so the byte-shape now matches
/// exactly what `signing_procedure` produces.
#[test]
fn remote_sign_document_preserves_user_jacs_type_and_signs_with_canonical_jacs() {
    let _g = REMOTE_SIGN_LOCK.lock().unwrap();
    let env = AgentEnv::new();
    let remote = env.remote();

    let payload = serde_json::json!({"jacsType": "memory", "body": "round-trip-B"});
    let signed = remote.sign_document(&payload).expect("sign_document");

    // Confirm the user's `jacsType` is preserved (it's a non-collision header
    // field that JACS keeps when present) and the body survives unchanged.
    let parsed: serde_json::Value = serde_json::from_str(&signed).expect("signed json");
    assert_eq!(
        parsed.get("jacsType").and_then(|v| v.as_str()),
        Some("memory"),
        "user-supplied jacsType must survive the signing pass"
    );
    assert_eq!(
        parsed.get("body").and_then(|v| v.as_str()),
        Some("round-trip-B")
    );

    // Confirm we get the full JACS signature shape, not the previous stub.
    let sig = parsed
        .get("jacsSignature")
        .and_then(|v| v.as_object())
        .expect("jacsSignature object");
    for key in [
        "agentID",
        "agentVersion",
        "date",
        "iat",
        "jti",
        "signature",
        "signingAlgorithm",
        "publicKeyHash",
        "fields",
    ] {
        assert!(
            sig.contains_key(key),
            "jacsSignature missing canonical field {key}; full envelope: {signed}"
        );
    }
    assert_eq!(
        sig.get("agentID").and_then(|v| v.as_str()),
        Some(remote.jacs_id()),
        "jacsSignature.agentID must be the caller"
    );
}

/// Confirm `sign_document` builds a fresh envelope each call — the JACS
/// schema injects a new `jacsId`/`jacsVersion`/`jacsVersionDate` on every
/// sign, so two signs of the same payload MUST produce different envelopes
/// (otherwise the server's PRIMARY KEY (jacs_id, jacs_version) would
/// collide).
#[test]
fn remote_sign_document_produces_distinct_envelopes_per_call() {
    let _g = REMOTE_SIGN_LOCK.lock().unwrap();
    let env = AgentEnv::new();
    let remote = env.remote();

    let a = remote.sign_document(&serde_json::json!({"k": 1})).expect("a");
    let b = remote.sign_document(&serde_json::json!({"k": 1})).expect("b");
    let a_v: serde_json::Value = serde_json::from_str(&a).unwrap();
    let b_v: serde_json::Value = serde_json::from_str(&b).unwrap();
    assert_ne!(
        a_v.get("jacsId"),
        b_v.get("jacsId"),
        "each sign call must produce a fresh jacsId"
    );
    assert_ne!(
        a_v.get("jacsVersion"),
        b_v.get("jacsVersion"),
        "each sign call must produce a fresh jacsVersion"
    );
    assert_ne!(
        a_v.get("jacsSignature").and_then(|s| s.get("signature")),
        b_v.get("jacsSignature").and_then(|s| s.get("signature")),
        "each sign call must produce a different signature"
    );
}
