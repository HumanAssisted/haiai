#![cfg(feature = "jacs-local")]

use std::fs;
use std::path::{Path, PathBuf};

use haisdk::{CreateAgentOptions, JacsProvider, LocalJacsProvider};

struct CwdGuard {
    original: PathBuf,
}

impl CwdGuard {
    fn enter(path: &Path) -> Self {
        let original = std::env::current_dir().expect("current dir");
        std::env::set_current_dir(path).expect("set current dir");
        Self { original }
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original);
    }
}

fn new_create_options() -> CreateAgentOptions {
    CreateAgentOptions {
        name: "rust-init-agent".to_string(),
        password: "TestPass!123".to_string(),
        algorithm: Some("ring-Ed25519".to_string()),
        data_directory: Some("./data".to_string()),
        key_directory: Some("./keys".to_string()),
        config_path: Some("./jacs.config.json".to_string()),
        agent_type: Some("ai".to_string()),
        description: Some("Rust init test agent".to_string()),
        domain: None,
        default_storage: Some("fs".to_string()),
    }
}

#[test]
fn create_agent_writes_config_and_key_material() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _cwd = CwdGuard::enter(temp.path());
    let options = new_create_options();

    let created = LocalJacsProvider::create_agent_with_options(&options).expect("create agent");

    assert!(!created.agent_id.is_empty());
    assert!(temp.path().join("jacs.config.json").is_file());

    let key_file_names: Vec<String> = fs::read_dir(temp.path().join("keys"))
        .expect("read keys dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();

    assert!(
        key_file_names.iter().any(|n| n.contains("private")),
        "expected a private key file in keys directory, got {key_file_names:?}"
    );
    assert!(
        key_file_names.iter().any(|n| n.contains("public")),
        "expected a public key file in keys directory, got {key_file_names:?}"
    );
}

#[test]
fn local_provider_loads_created_agent_and_signs() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _cwd = CwdGuard::enter(temp.path());
    let options = new_create_options();
    let created = LocalJacsProvider::create_agent_with_options(&options).expect("create agent");
    let config_path = temp.path().join("jacs.config.json");
    assert!(config_path.is_file());

    let provider = LocalJacsProvider::from_config_path(Some(&config_path)).expect("load provider");

    assert_eq!(provider.config_path(), config_path.as_path());
    assert!(!provider.jacs_id().is_empty());
    assert_eq!(created.agent_id, provider.jacs_id());

    let signature = provider.sign_string("hello-rust-init").expect("sign");
    assert!(!signature.is_empty());

    let exported_agent = provider.export_agent_json().expect("export agent");
    assert!(exported_agent.contains("\"jacsId\""));

    let public_key = provider.public_key_pem().expect("public key");
    assert!(public_key.contains("BEGIN PUBLIC KEY"));
}

#[test]
fn local_provider_fails_for_missing_config() {
    let temp = tempfile::tempdir().expect("tempdir");
    let missing = temp.path().join("does-not-exist.config.json");

    let err = match LocalJacsProvider::from_config_path(Some(&missing)) {
        Ok(_) => panic!("expected missing-config failure"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("failed to load JACS agent"));
}
