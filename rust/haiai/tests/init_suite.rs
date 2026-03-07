#![cfg(any(feature = "jacs-crate", feature = "jacs-local"))]

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use haiai::{CreateAgentOptions, JacsProvider, LocalJacsProvider};
use uuid::Uuid;

static INIT_TEST_LOCK: Mutex<()> = Mutex::new(());

struct InitPaths {
    relative_base: String,
    absolute_base: PathBuf,
}

impl InitPaths {
    fn new() -> Self {
        let relative_base = format!("target/init-suite-{}", Uuid::new_v4());
        let absolute_base = std::env::current_dir()
            .expect("current dir")
            .join(&relative_base);
        fs::create_dir_all(&absolute_base).expect("create unique test base");

        Self {
            relative_base,
            absolute_base,
        }
    }

    fn config_path_abs(&self) -> PathBuf {
        self.absolute_base.join("jacs.config.json")
    }

    fn key_dir_abs(&self) -> PathBuf {
        self.absolute_base.join("keys")
    }

    fn to_options(&self) -> CreateAgentOptions {
        CreateAgentOptions {
            name: "rust-init-agent".to_string(),
            password: "TestPass!123".to_string(),
            algorithm: Some("ring-Ed25519".to_string()),
            data_directory: Some(format!("./{}/data", self.relative_base)),
            key_directory: Some(format!("./{}/keys", self.relative_base)),
            config_path: Some(format!("./{}/jacs.config.json", self.relative_base)),
            agent_type: Some("ai".to_string()),
            description: Some("Rust init test agent".to_string()),
            domain: None,
            default_storage: Some("fs".to_string()),
        }
    }
}

#[test]
fn create_agent_writes_config_and_key_material() {
    let _lock = INIT_TEST_LOCK.lock().expect("lock init tests");
    let paths = InitPaths::new();
    let options = paths.to_options();

    let created = LocalJacsProvider::create_agent_with_options(&options).expect("create agent");

    assert!(!created.agent_id.is_empty());
    assert!(paths.config_path_abs().is_file());

    let key_file_names: Vec<String> = fs::read_dir(paths.key_dir_abs())
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
    let config_path = paths.config_path_abs();
    assert!(config_path.is_file());
    std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", "TestPass!123");

    let provider = LocalJacsProvider::from_config_path(Some(&config_path)).expect("load provider");

    assert_eq!(provider.config_path(), config_path.as_path());
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
