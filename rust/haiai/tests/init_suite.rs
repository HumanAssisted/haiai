#![cfg(feature = "jacs-crate")]

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use haiai::{CreateAgentOptions, JacsProvider, LocalJacsProvider};
use uuid::Uuid;

static INIT_TEST_LOCK: Mutex<()> = Mutex::new(());

struct InitPaths {
    absolute_base: PathBuf,
    original_cwd: PathBuf,
}

impl InitPaths {
    /// Create an isolated test directory and cd into it.
    /// Must be called while holding INIT_TEST_LOCK (tests mutate CWD).
    fn new() -> Self {
        let original_cwd = std::env::current_dir().expect("current dir");
        let absolute_base = original_cwd.join(format!("target/init-suite-{}", Uuid::new_v4()));
        fs::create_dir_all(&absolute_base).expect("create unique test base");
        std::env::set_current_dir(&absolute_base).expect("cd to test dir");

        Self {
            absolute_base,
            original_cwd,
        }
    }

    fn config_path(&self) -> PathBuf {
        self.absolute_base.join("jacs.config.json")
    }

    fn key_dir(&self) -> PathBuf {
        self.absolute_base.join("keys")
    }

    fn to_options(&self) -> CreateAgentOptions {
        // jacs create_with_params writes relative to CWD and load_by_config
        // resolves relative to config parent dir. Since we cd into the test
        // dir, using "data"/"keys"/"jacs.config.json" is consistent.
        CreateAgentOptions {
            name: "rust-init-agent".to_string(),
            password: "TestPass!123".to_string(),
            algorithm: Some("pq2025".to_string()),
            data_directory: Some("data".to_string()),
            key_directory: Some("keys".to_string()),
            config_path: Some("jacs.config.json".to_string()),
            agent_type: Some("ai".to_string()),
            description: Some("Rust init test agent".to_string()),
            domain: None,
            default_storage: Some("fs".to_string()),
        }
    }
}

impl Drop for InitPaths {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original_cwd);
    }
}

#[test]
fn create_agent_writes_config_and_key_material() {
    let _lock = INIT_TEST_LOCK.lock().expect("lock init tests");
    let paths = InitPaths::new();
    let options = paths.to_options();

    let created = LocalJacsProvider::create_agent_with_options(&options).expect("create agent");

    assert!(!created.agent_id.is_empty());
    assert!(paths.config_path().is_file());

    let key_file_names: Vec<String> = fs::read_dir(paths.key_dir())
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
    let _lock = INIT_TEST_LOCK.lock().expect("lock init tests");
    let paths = InitPaths::new();
    let options = paths.to_options();
    let created = LocalJacsProvider::create_agent_with_options(&options).expect("create agent");
    let config_path = paths.config_path();
    assert!(config_path.is_file());
    std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", "TestPass!123");

    let provider = LocalJacsProvider::from_config_path(Some(&config_path)).expect("load provider");

    assert!(!provider.jacs_id().is_empty());
    assert_eq!(created.agent_id, provider.jacs_id());

    let signature = provider.sign_string("hello-rust-init").expect("sign");
    assert!(!signature.is_empty());

    let exported_agent = provider.export_agent_json().expect("export agent");
    assert!(exported_agent.contains("\"jacsId\""));
}

#[test]
fn local_provider_fails_for_missing_config() {
    let _lock = INIT_TEST_LOCK.lock().expect("lock init tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let missing = temp.path().join("does-not-exist.config.json");

    let err = match LocalJacsProvider::from_config_path(Some(&missing)) {
        Ok(_) => panic!("expected missing-config failure"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("failed to load JACS agent"));
}
