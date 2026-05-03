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
///
/// The `remote` config axis (PRD Section 6.1) is consulted independently of the
/// storage backend label. When `remote=true` (via env, config, or backward-compat
/// `JACS_DEFAULT_STORAGE=remote`), the provider syncs with HAI records API
/// regardless of which local storage label was resolved.
pub fn build_document_provider(
    config_path: Option<&Path>,
    storage: Option<&str>,
    base_url: Option<String>,
) -> Result<Box<dyn JacsDocumentProvider>> {
    let backend = resolve_storage_backend(storage, config_path)?;
    let remote = crate::config::resolve_remote(None, config_path);

    // When remote=true and backend is a local label (fs/sqlite), promote to
    // remote provider with local signing — the PRD's two-axis model.
    if remote && backend != "remote" {
        tracing::info!(backend = %backend, remote = true, "promoting to remote provider (remote=true in config)");
        return build_document_provider_for_backend(config_path, "remote", base_url);
    }

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
    use jacs::simple::CreateAgentParams;

    use super::*;

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
        let _guard = crate::test_support::env_lock();
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
        let _guard = crate::test_support::env_lock();
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

    #[test]
    fn local_backend_save_memory_twice_versions_same_document() {
        let _guard = crate::test_support::env_lock();
        let (_dir, config_path) = create_test_agent();
        let provider =
            build_document_provider(Some(&config_path), Some("fs"), None).expect("local provider");

        let first = provider
            .save_memory(Some("first memory"))
            .expect("first save");
        let first_id = first.split_once(':').expect("id:version").0.to_string();

        let second = provider
            .save_memory(Some("second memory"))
            .expect("second save updates same memory document");
        let (second_id, second_version) = second.split_once(':').expect("id:version");
        assert_eq!(second_id, first_id);
        assert_ne!(second, first);

        let latest = provider
            .get_record_bytes(&first_id)
            .expect("fetch latest by doc id");
        let latest_text = String::from_utf8(latest).expect("signed markdown is UTF-8");
        assert!(latest_text.contains("second memory"));
        assert!(latest_text.contains(&format!(
            "jacsPreviousVersion: {}",
            first.split_once(':').unwrap().1
        )));
        assert!(latest_text.contains(&format!("jacsVersion: {second_version}")));
    }

    #[test]
    fn local_backend_explicit_doc_id_update_uses_real_latest_key() {
        let _guard = crate::test_support::env_lock();
        let (_dir, config_path) = create_test_agent();
        let provider =
            build_document_provider(Some(&config_path), Some("fs"), None).expect("local provider");

        let first = provider
            .save_memory(Some("first memory"))
            .expect("first save");
        let first_id = first.split_once(':').expect("id:version").0.to_string();
        let updated = provider
            .save_document(crate::jacs::SaveDocumentRequest {
                doc_id: Some(first_id.clone()),
                jacs_type: "memory".to_string(),
                logical_name: Some("MEMORY.md".to_string()),
                content_type: "text/markdown; profile=jacs-text-v1".to_string(),
                plaintext: b"explicit update".to_vec(),
                expected_previous_version: None,
                singleton: true,
                intent: crate::jacs::SaveIntent::Update,
            })
            .expect("explicit update");

        assert!(updated.key.starts_with(&format!("{first_id}:")));
        assert_ne!(updated.key, first);
        assert!(updated.json.contains("explicit update"));
    }

    #[test]
    fn local_backend_create_intent_rejects_existing_singleton() {
        let _guard = crate::test_support::env_lock();
        let (_dir, config_path) = create_test_agent();
        let provider =
            build_document_provider(Some(&config_path), Some("fs"), None).expect("local provider");

        provider
            .save_memory(Some("first memory"))
            .expect("first save");
        let err = provider
            .save_document(crate::jacs::SaveDocumentRequest {
                doc_id: None,
                jacs_type: "memory".to_string(),
                logical_name: Some("MEMORY.md".to_string()),
                content_type: "text/markdown; profile=jacs-text-v1".to_string(),
                plaintext: b"should not create".to_vec(),
                expected_previous_version: None,
                singleton: true,
                intent: crate::jacs::SaveIntent::Create,
            })
            .expect_err("create intent must reject existing memory singleton");

        assert!(err.to_string().contains("duplicate singleton"));
    }

    fn restore_env(key: &str, saved: Option<String>) {
        if let Some(value) = saved {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
    }
}
