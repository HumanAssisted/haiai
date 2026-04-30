use std::path::Path;

use crate::client::DEFAULT_BASE_URL;
use crate::config::resolve_storage_backend;
use crate::error::{HaiError, Result};
use crate::jacs::JacsDocumentProvider;
use crate::jacs_local::LocalJacsProvider;
use crate::jacs_remote::{RemoteJacsProvider, RemoteJacsProviderOptions};

const LOCAL_SIGNER_STORAGE: &str = "fs";

/// Build the canonical document provider for local or remote JACS document storage.
///
/// This is the single routed provider entry point for CLI, MCP, and FFI document
/// operations. `storage` may be an explicit CLI/config override; otherwise
/// [`resolve_storage_backend`] applies `JACS_DEFAULT_STORAGE`, `jacs.config.json`,
/// and the `fs` default.
pub fn build_document_provider(
    config_path: Option<&Path>,
    storage: Option<&str>,
    base_url: Option<String>,
) -> Result<Box<dyn JacsDocumentProvider>> {
    let backend = resolve_storage_backend(storage, config_path)?;
    build_document_provider_for_backend(config_path, &backend, base_url)
}

/// Build a document provider for a pre-resolved backend label.
pub fn build_document_provider_for_backend(
    config_path: Option<&Path>,
    backend: &str,
    base_url: Option<String>,
) -> Result<Box<dyn JacsDocumentProvider>> {
    match backend {
        "fs" | "rusqlite" | "sqlite" => {
            tracing::info!(backend, "building local JACS document provider");
            let local = LocalJacsProvider::from_config_path(config_path, Some(backend))?;
            Ok(Box::new(local))
        }
        "remote" => {
            tracing::info!(backend, "building remote HAI document provider");
            let local =
                LocalJacsProvider::from_config_path(config_path, Some(LOCAL_SIGNER_STORAGE))
                    .map_err(|e| {
                        HaiError::Provider(format!(
                            "failed to load local JACS signer for remote document provider: {e}"
                        ))
                    })?;
            let base_url = base_url
                .or_else(|| std::env::var("HAI_URL").ok())
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
            let remote = RemoteJacsProvider::new(
                local,
                RemoteJacsProviderOptions {
                    base_url,
                    ..RemoteJacsProviderOptions::default()
                },
            )?;
            Ok(Box::new(remote))
        }
        other => Err(HaiError::ConfigInvalid {
            message: format!(
                "Unsupported storage backend '{}'. Valid routed labels: fs, rusqlite, sqlite, remote",
                other
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use jacs::simple::CreateAgentParams;

    use super::*;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn create_test_agent() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path().canonicalize().expect("canonical tempdir");
        let config_path = base.join("jacs.config.json");
        let data_dir = base.join("jacs_data");
        let key_dir = base.join("jacs_keys");
        std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", "TestPass!123");
        LocalJacsProvider::create_agent(CreateAgentParams {
            name: "doc-store-test".to_string(),
            password: "TestPass!123".to_string(),
            config_path: config_path.to_string_lossy().into_owned(),
            data_directory: data_dir.to_string_lossy().into_owned(),
            key_directory: key_dir.to_string_lossy().into_owned(),
            algorithm: "ed25519".to_string(),
            default_storage: "fs".to_string(),
            ..CreateAgentParams::default()
        })
        .expect("create agent");
        (dir, config_path)
    }

    #[test]
    fn remote_backend_loads_local_signer_when_env_selects_remote() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let (_dir, config_path) = create_test_agent();
        let saved = std::env::var("JACS_DEFAULT_STORAGE").ok();
        std::env::set_var("JACS_DEFAULT_STORAGE", "remote");

        let provider = build_document_provider(
            Some(&config_path),
            None,
            Some("http://127.0.0.1:9".to_string()),
        )
        .expect("remote provider should still load local signer");

        restore_env("JACS_DEFAULT_STORAGE", saved);
        assert!(!provider.jacs_id().is_empty());
    }

    #[test]
    fn local_backend_can_save_memory_through_default_d5_path() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let (_dir, config_path) = create_test_agent();
        let provider =
            build_document_provider(Some(&config_path), Some("fs"), None).expect("local provider");

        let key = provider
            .save_memory(Some("remember the routed provider"))
            .expect("save memory locally");
        let saved = provider
            .get_document(&key)
            .expect("get saved memory by key");

        assert!(key.contains(':'));
        assert!(
            saved.contains("remember the routed provider"),
            "saved local memory should be retrievable by key"
        );
        let latest = provider
            .get_memory()
            .expect("get latest local memory")
            .expect("latest local memory should exist");
        assert!(
            latest.contains("remember the routed provider"),
            "saved local memory should be retrievable through get_memory"
        );
    }

    fn restore_env(key: &str, saved: Option<String>) {
        if let Some(value) = saved {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
    }
}
