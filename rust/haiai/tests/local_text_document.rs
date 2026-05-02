//! TASK_004 integration tests: `sign_text_document_create` and
//! `sign_text_document_update` on `LocalJacsProvider`.
//!
//! Each test creates an isolated agent under `target/local-textdoc-<uuid>/`,
//! exercises the in-memory inline signing pipeline, and verifies the JACS
//! footer metadata end-to-end.

#![cfg(feature = "jacs-crate")]

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use haiai::{CreateAgentOptions, JacsDocumentProvider, LocalJacsProvider};
use uuid::Uuid;

static TEXT_DOC_TEST_LOCK: Mutex<()> = Mutex::new(());

#[allow(dead_code)]
struct AgentEnv {
    base: PathBuf,
    original_cwd: PathBuf,
    config_path: PathBuf,
}

impl AgentEnv {
    fn new() -> Self {
        let original_cwd = std::env::current_dir().expect("cwd");
        let base = original_cwd.join(format!("target/local-textdoc-{}", Uuid::new_v4()));
        fs::create_dir_all(&base).expect("create base");
        std::env::set_current_dir(&base).expect("cd to base");

        let options = CreateAgentOptions {
            name: "local-textdoc-agent".to_string(),
            password: "TestPass!123".to_string(),
            algorithm: Some("pq2025".to_string()),
            data_directory: Some("data".to_string()),
            key_directory: Some("keys".to_string()),
            config_path: Some("jacs.config.json".to_string()),
            agent_type: Some("ai".to_string()),
            description: Some("Local text document test agent".to_string()),
            domain: None,
            default_storage: Some("fs".to_string()),
        };
        LocalJacsProvider::create_agent_with_options(&options).expect("create agent");
        std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", "TestPass!123");

        let config_path = base.join("jacs.config.json");
        Self {
            base,
            original_cwd,
            config_path,
        }
    }

    fn provider(&self) -> LocalJacsProvider {
        LocalJacsProvider::from_config_path(Some(&self.config_path), None).expect("provider")
    }
}

impl Drop for AgentEnv {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original_cwd);
    }
}

/// Helper: extract the JACS footer YAML body and parse as JSON.
fn extract_footer_json(signed: &[u8]) -> serde_json::Value {
    let text = std::str::from_utf8(signed).expect("signed output is UTF-8");
    let begin = "-----BEGIN JACS SIGNATURE-----";
    let end = "-----END JACS SIGNATURE-----";
    let begin_idx = text.rfind(begin).expect("no BEGIN marker");
    let end_idx = text.rfind(end).expect("no END marker");
    let body = &text[begin_idx + begin.len()..end_idx].trim();
    let json_str = jacs::convert::yaml_to_jacs(body).expect("yaml_to_jacs");
    serde_json::from_str(&json_str).expect("parse JSON")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn create_soul_produces_footer_with_jacs_type_soul() {
    let _lock = TEXT_DOC_TEST_LOCK.lock().unwrap();
    let env = AgentEnv::new();
    let provider = env.provider();

    let plaintext = b"# My Soul\n\nI am a helpful assistant.\n";
    let signed = provider
        .sign_text_document_create("soul", "SOUL.md", "text/markdown", plaintext)
        .expect("sign_text_document_create soul");

    // Signed output is valid UTF-8.
    let text = std::str::from_utf8(&signed).expect("UTF-8");
    assert!(
        text.contains("-----BEGIN JACS SIGNATURE-----"),
        "must contain JACS footer"
    );

    // Footer jacsType is "soul".
    let footer = extract_footer_json(&signed);
    assert_eq!(
        footer.get("jacsType").and_then(|v| v.as_str()),
        Some("soul"),
        "jacsType must be 'soul'"
    );

    // Footer has jacsId and jacsVersion.
    assert!(
        footer.get("jacsId").and_then(|v| v.as_str()).is_some(),
        "must have jacsId"
    );
    assert!(
        footer.get("jacsVersion").and_then(|v| v.as_str()).is_some(),
        "must have jacsVersion"
    );
}

#[test]
fn create_memory_produces_footer_with_jacs_type_memory() {
    let _lock = TEXT_DOC_TEST_LOCK.lock().unwrap();
    let env = AgentEnv::new();
    let provider = env.provider();

    let plaintext = b"# Memory Log\n\nUser prefers concise answers.\n";
    let signed = provider
        .sign_text_document_create("memory", "MEMORY.md", "text/markdown", plaintext)
        .expect("sign_text_document_create memory");

    let footer = extract_footer_json(&signed);
    assert_eq!(
        footer.get("jacsType").and_then(|v| v.as_str()),
        Some("memory"),
        "jacsType must be 'memory'"
    );
}

#[test]
fn update_preserves_jacs_id_and_sets_previous_version() {
    let _lock = TEXT_DOC_TEST_LOCK.lock().unwrap();
    let env = AgentEnv::new();
    let provider = env.provider();

    // Create v1.
    let plaintext_v1 = b"# Soul v1\n\nOriginal content.\n";
    let signed_v1 = provider
        .sign_text_document_create("soul", "SOUL.md", "text/markdown", plaintext_v1)
        .expect("create v1");

    let footer_v1 = extract_footer_json(&signed_v1);
    let jacs_id = footer_v1
        .get("jacsId")
        .and_then(|v| v.as_str())
        .expect("v1 jacsId")
        .to_string();
    let version_v1 = footer_v1
        .get("jacsVersion")
        .and_then(|v| v.as_str())
        .expect("v1 jacsVersion")
        .to_string();

    // Update to v2.
    let plaintext_v2 = b"# Soul v2\n\nUpdated content.\n";
    let signed_v2 = provider
        .sign_text_document_update(&signed_v1, plaintext_v2, &version_v1)
        .expect("update to v2");

    let footer_v2 = extract_footer_json(&signed_v2);

    // Same jacsId.
    assert_eq!(
        footer_v2.get("jacsId").and_then(|v| v.as_str()),
        Some(jacs_id.as_str()),
        "jacsId must be preserved across updates"
    );

    // Different jacsVersion.
    let version_v2 = footer_v2
        .get("jacsVersion")
        .and_then(|v| v.as_str())
        .expect("v2 jacsVersion");
    assert_ne!(
        version_v2, version_v1,
        "jacsVersion must change on update"
    );

    // jacsPreviousVersion equals v1's version.
    assert_eq!(
        footer_v2.get("jacsPreviousVersion").and_then(|v| v.as_str()),
        Some(version_v1.as_str()),
        "jacsPreviousVersion must match the previous version"
    );

    // Content of v2 includes the new plaintext.
    let text_v2 = std::str::from_utf8(&signed_v2).expect("UTF-8");
    assert!(
        text_v2.contains("# Soul v2"),
        "updated content must be present"
    );
    assert!(
        !text_v2.contains("# Soul v1"),
        "old content must not be present"
    );
}
