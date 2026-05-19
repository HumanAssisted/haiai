// HAIAI_WASM_PRD §4.7 + Task 009: the on-disk loaders below need `std::fs`
// and are unreachable from a browser build. We keep the in-memory
// `AgentConfig` struct + serde derives unconditional so the wasm
// `config_browser::from_json_string` (Task 016) can re-use the same shape.
// All FS-touching helpers are gated `cfg(not(target_arch = "wasm32"))`
// or use the env-only fallbacks that already short-circuit on missing
// disk state.
use std::env;
#[cfg(not(target_arch = "wasm32"))]
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
#[cfg(not(target_arch = "wasm32"))]
use serde_json::Value;

use crate::error::{HaiError, Result};

pub const DEFAULT_LOG_FILTER: &str = "info,rmcp=warn";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub jacs_agent_name: String,
    pub jacs_agent_version: String,
    pub jacs_key_dir: PathBuf,
    pub jacs_id: Option<String>,
    pub jacs_private_key_path: Option<PathBuf>,
    pub source_path: PathBuf,
    /// Cached @hai.ai email address for this agent.
    pub agent_email: Option<String>,
}

/// Load `jacs.config.json`.
///
/// Discovery order:
/// 1. explicit `path`
/// 2. `JACS_CONFIG_PATH`
/// 3. `./jacs.config.json`
#[cfg(not(target_arch = "wasm32"))]
pub fn load_config(path: Option<&Path>) -> Result<AgentConfig> {
    let source_path = resolve_config_path(path);
    if !source_path.is_file() {
        return Err(HaiError::ConfigNotFound {
            path: source_path.display().to_string(),
        });
    }

    let raw = fs::read_to_string(&source_path).map_err(|e| HaiError::ConfigInvalid {
        message: format!("failed to read {}: {e}", source_path.display()),
    })?;
    let data: Value = serde_json::from_str(&raw)?;

    let config_dir = source_path.parent().unwrap_or_else(|| Path::new("."));

    let jacs_agent_name = get_string(&data, &["jacsAgentName", "agent_name"]).ok_or_else(|| {
        HaiError::ConfigInvalid {
            message: "jacsAgentName (or agent_name) is required but missing".to_string(),
        }
    })?;
    let jacs_agent_version = get_string(&data, &["jacsAgentVersion", "agent_version"])
        .unwrap_or_else(|| "1.0.0".to_string());

    let key_dir_raw =
        get_string(&data, &["jacsKeyDir", "key_dir"]).unwrap_or_else(|| ".".to_string());
    let jacs_key_dir = if Path::new(&key_dir_raw).is_absolute() {
        PathBuf::from(key_dir_raw)
    } else {
        config_dir.join(key_dir_raw)
    };

    let jacs_id = get_string(&data, &["jacsId", "jacs_id"]);
    if jacs_id.is_none() {
        return Err(HaiError::ConfigInvalid {
            message: "jacsId (or jacs_id) is required but missing".to_string(),
        });
    }

    let private_key_raw = get_string(&data, &["jacsPrivateKeyPath", "private_key_path"]);
    let jacs_private_key_path = private_key_raw.map(|p| {
        if Path::new(&p).is_absolute() {
            PathBuf::from(p)
        } else {
            config_dir.join(p)
        }
    });

    let agent_email = get_string(&data, &["agent_email", "agentEmail"]);

    Ok(AgentConfig {
        jacs_agent_name,
        jacs_agent_version,
        jacs_key_dir,
        jacs_id,
        jacs_private_key_path,
        source_path,
        agent_email,
    })
}

pub fn resolve_private_key_candidates(config: &AgentConfig) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(explicit) = &config.jacs_private_key_path {
        candidates.push(explicit.clone());
    }

    candidates.push(config.jacs_key_dir.join("agent_private_key.pem"));
    candidates.push(
        config
            .jacs_key_dir
            .join(format!("{}.private.pem", config.jacs_agent_name)),
    );
    candidates.push(config.jacs_key_dir.join("private_key.pem"));

    candidates
}

#[cfg(target_arch = "wasm32")]
pub fn load_config(_path: Option<&Path>) -> Result<AgentConfig> {
    // Browsers cannot read jacs.config.json from disk. Callers in the
    // wasm build should use `config_browser::from_json_string` (Task 016)
    // to materialize an AgentConfig from a JSON string instead. We re-use
    // the existing `BackendUnsupported` typed variant (PRD §10 explicitly
    // forbids new HaiError variants for the wasm port).
    Err(HaiError::BackendUnsupported {
        method: "load_config".to_string(),
        detail: "file system access not available in wasm32 build; use config_browser::from_json_string"
            .to_string(),
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn resolve_config_path(path: Option<&Path>) -> PathBuf {
    if let Some(path) = path {
        return path.to_path_buf();
    }

    if let Ok(path) = env::var("JACS_CONFIG_PATH") {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }

    PathBuf::from("./jacs.config.json")
}

/// Validate a routed backend label for DocumentService-backed operations.
///
/// Accepts: `fs`, `rusqlite`, `sqlite` (alias for `rusqlite`), `remote`.
/// Returns the canonical label on success, or an error with valid options on failure.
pub fn resolve_storage_backend_label(label: &str) -> Result<String> {
    match label {
        "fs" => Ok("fs".to_string()),
        "rusqlite" | "sqlite" => Ok("rusqlite".to_string()),
        "remote" => Ok("remote".to_string()),
        other => Err(HaiError::ConfigInvalid {
            message: format!(
                "Unsupported storage backend '{}'. Valid routed labels: fs, rusqlite, sqlite, remote",
                other
            ),
        }),
    }
}

/// Resolve which storage backend to use with priority:
/// 1. Explicit parameter (CLI `--storage` flag)
/// 2. `JACS_DEFAULT_STORAGE` env var
/// 3. `jacs_default_storage` field in `jacs.config.json`
/// 4. `"fs"` default
pub fn resolve_storage_backend(
    explicit: Option<&str>,
    config_path: Option<&Path>,
) -> Result<String> {
    // Priority 1: explicit parameter
    if let Some(label) = explicit {
        return resolve_storage_backend_label(label);
    }

    // Priority 2: JACS_DEFAULT_STORAGE env var
    if let Ok(label) = env::var("JACS_DEFAULT_STORAGE") {
        if !label.is_empty() {
            return resolve_storage_backend_label(&label);
        }
    }

    // Priority 3: storage field in config (file-backed lookup, native only).
    // The wasm build skips straight to Priority 4 — see HAIAI_WASM_PRD §4.7.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let config_path_resolved = resolve_config_path(config_path);
        if config_path_resolved.is_file() {
            if let Ok(raw) = fs::read_to_string(&config_path_resolved) {
                if let Ok(data) = serde_json::from_str::<Value>(&raw) {
                    if let Some(label) = get_string(
                        &data,
                        &["jacs_default_storage", "default_storage", "defaultStorage"],
                    ) {
                        return resolve_storage_backend_label(&label);
                    }
                }
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        // Browser builds have no jacs.config.json on disk; silence the
        // `unused` warning on the parameter.
        let _ = config_path;
    }

    // Priority 4: default to fs
    Ok("fs".to_string())
}

/// Resolve whether the SDK should operate in remote mode (resolve and push
/// documents to the HAI records API) or local-only mode.
///
/// Priority order:
/// 1. Explicit `explicit` parameter (e.g. CLI `--remote` flag)
/// 2. `JACS_REMOTE` env var (`"true"` / `"1"` / `"yes"`)
/// 3. `remote` field in `jacs.config.json`
/// 4. Backward compat: `JACS_DEFAULT_STORAGE=remote` implies `remote=true`
/// 5. Default: `false`
pub fn resolve_remote(explicit: Option<bool>, config_path: Option<&Path>) -> bool {
    // Priority 1: explicit parameter
    if let Some(r) = explicit {
        return r;
    }

    // Priority 2: JACS_REMOTE env var
    if let Ok(val) = env::var("JACS_REMOTE") {
        let val = val.trim().to_lowercase();
        if matches!(val.as_str(), "true" | "1" | "yes") {
            return true;
        }
        if matches!(val.as_str(), "false" | "0" | "no") {
            return false;
        }
    }

    // Priority 3: config file `remote` field (native only — wasm skips).
    #[cfg(not(target_arch = "wasm32"))]
    {
        let config_path_resolved = resolve_config_path(config_path);
        if config_path_resolved.is_file() {
            if let Ok(raw) = fs::read_to_string(&config_path_resolved) {
                if let Ok(data) = serde_json::from_str::<Value>(&raw) {
                    if let Some(val) = data.get("remote").and_then(|v| v.as_bool()) {
                        return val;
                    }
                }
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = config_path;
    }

    // Priority 4: backward compat — JACS_DEFAULT_STORAGE=remote implies remote=true
    if let Ok(label) = env::var("JACS_DEFAULT_STORAGE") {
        if label.trim().eq_ignore_ascii_case("remote") {
            return true;
        }
    }

    // Priority 5: default
    false
}

/// A storage configuration summary safe for logging/display.
///
/// Never includes passwords, connection strings, or credentials.
/// Complies with PRD Section 4.4.6: "Never log backend configuration details."
#[derive(Debug, Clone, Serialize)]
pub struct StorageConfigSummary {
    pub backend: String,
    pub source: &'static str,
    /// Whether the SDK operates in remote mode (resolve/push to HAI records API).
    pub remote: bool,
}

impl std::fmt::Display for StorageConfigSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "backend={} (from {}), remote={}",
            self.backend, self.source, self.remote
        )
    }
}

/// Return a redacted summary of the resolved storage configuration.
///
/// This is safe for logging, CLI output, and error messages.
/// It reports the backend label and its resolution source, but never
/// exposes connection strings, passwords, or file system paths.
pub fn redacted_display(
    explicit: Option<&str>,
    config_path: Option<&Path>,
) -> StorageConfigSummary {
    let remote = resolve_remote(None, config_path);

    // Priority 1: explicit parameter
    if let Some(label) = explicit {
        return StorageConfigSummary {
            backend: label.to_string(),
            source: "--storage flag",
            remote,
        };
    }

    // Priority 2: JACS_DEFAULT_STORAGE env var
    if let Ok(label) = env::var("JACS_DEFAULT_STORAGE") {
        if !label.is_empty() {
            return StorageConfigSummary {
                backend: label,
                source: "JACS_DEFAULT_STORAGE env var",
                remote,
            };
        }
    }

    // Priority 3: config file (native only — wasm has no disk).
    #[cfg(not(target_arch = "wasm32"))]
    {
        let config_path_resolved = resolve_config_path(config_path);
        if config_path_resolved.is_file() {
            if let Ok(raw) = fs::read_to_string(&config_path_resolved) {
                if let Ok(data) = serde_json::from_str::<Value>(&raw) {
                    if let Some(label) = get_string(
                        &data,
                        &["jacs_default_storage", "default_storage", "defaultStorage"],
                    ) {
                        return StorageConfigSummary {
                            backend: label,
                            source: "config file",
                            remote,
                        };
                    }
                }
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = config_path;
    }

    // Priority 4: default
    StorageConfigSummary {
        backend: "fs".to_string(),
        source: "default",
        remote,
    }
}

/// Resolve the tracing filter with precedence: RUST_LOG, JACS config, default.
#[cfg(feature = "jacs-crate")]
pub fn resolve_log_filter(config_path: Option<&Path>) -> String {
    if let Ok(filter) = env::var("RUST_LOG") {
        if !filter.trim().is_empty() {
            return filter;
        }
    }

    let config_path_resolved = resolve_config_path(config_path);
    if config_path_resolved.is_file() {
        if let Ok(raw) = fs::read_to_string(&config_path_resolved) {
            if let Ok(config) = serde_json::from_str::<jacs::config::Config>(&raw) {
                let level = config.effective_log_level().trim();
                if !level.is_empty() && level != "info" {
                    return format!("{level},rmcp=warn");
                }
            }
        }
    }

    DEFAULT_LOG_FILTER.to_string()
}

#[cfg(not(target_arch = "wasm32"))]
fn get_string(data: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = data.get(key).and_then(Value::as_str) {
            return Some(value.to_string());
        }
    }
    None
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn resolves_relative_paths_from_config_location() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("nested").join("jacs.config.json");
        fs::create_dir_all(config_path.parent().expect("parent")).expect("mkdir");

        fs::write(
            &config_path,
            r#"{
  "jacsAgentName": "agent",
  "jacsAgentVersion": "1.0.0",
  "jacsKeyDir": "./keys",
  "jacsPrivateKeyPath": "./custom/private.pem",
  "jacsId": "agent-1"
}"#,
        )
        .expect("write config");

        let cfg = load_config(Some(&config_path)).expect("load");
        assert!(cfg.jacs_key_dir.ends_with("nested/keys"));
        assert!(cfg
            .jacs_private_key_path
            .expect("private key path")
            .ends_with("nested/custom/private.pem"));
    }

    // ── Issue #8: required config fields ─────────────────────────────────

    #[test]
    fn load_config_errors_when_jacs_agent_name_missing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("jacs.config.json");
        fs::write(
            &config_path,
            r#"{"jacsId": "agent-1", "jacsAgentVersion": "2.0.0"}"#,
        )
        .expect("write config");

        let result = load_config(Some(&config_path));
        assert!(
            result.is_err(),
            "missing jacs_agent_name should be an error"
        );
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("jacsAgentName") || err.contains("agent_name"),
            "error should mention the missing field: {err}"
        );
    }

    #[test]
    fn load_config_errors_when_jacs_id_missing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("jacs.config.json");
        fs::write(
            &config_path,
            r#"{"jacsAgentName": "my-agent", "jacsAgentVersion": "2.0.0"}"#,
        )
        .expect("write config");

        let result = load_config(Some(&config_path));
        assert!(result.is_err(), "missing jacs_id should be an error");
        let err = format!("{}", result.unwrap_err());
        assert!(
            err.contains("jacsId") || err.contains("jacs_id"),
            "error should mention the missing field: {err}"
        );
    }

    #[test]
    fn load_config_defaults_version_and_key_dir() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("jacs.config.json");
        fs::write(
            &config_path,
            r#"{"jacsAgentName": "my-agent", "jacsId": "agent-1"}"#,
        )
        .expect("write config");

        let cfg = load_config(Some(&config_path))
            .expect("should succeed with defaults for version and key_dir");
        assert_eq!(cfg.jacs_agent_version, "1.0.0");
        assert_eq!(cfg.jacs_agent_name, "my-agent");
        assert_eq!(cfg.jacs_id, Some("agent-1".to_string()));
    }

    #[test]
    fn resolve_storage_backend_label_accepts_remote() {
        assert_eq!(
            resolve_storage_backend_label("remote").expect("ok"),
            "remote"
        );
    }

    #[test]
    fn resolve_storage_backend_label_error_lists_remote() {
        let err = resolve_storage_backend_label("bogus").expect_err("must error");
        let msg = format!("{}", err);
        assert!(
            msg.contains("remote"),
            "error must list remote in valid options: {msg}"
        );
    }

    #[test]
    fn resolve_storage_backend_jacs_default_storage_env_var_remote() {
        let _guard = crate::test_support::env_lock();
        let saved_default = env::var("JACS_DEFAULT_STORAGE").ok();
        let saved_legacy = env::var("JACS_STORAGE").ok();
        env::set_var("JACS_DEFAULT_STORAGE", "remote");
        env::set_var("JACS_STORAGE", "rusqlite");
        let r =
            resolve_storage_backend(None, Some(Path::new("/nonexistent/path.json"))).expect("ok");
        restore_env("JACS_DEFAULT_STORAGE", saved_default);
        restore_env("JACS_STORAGE", saved_legacy);
        assert_eq!(r, "remote");
    }

    #[test]
    fn resolve_storage_backend_ignores_legacy_jacs_storage_env_var() {
        let _guard = crate::test_support::env_lock();
        let saved_default = env::var("JACS_DEFAULT_STORAGE").ok();
        let saved_legacy = env::var("JACS_STORAGE").ok();
        env::remove_var("JACS_DEFAULT_STORAGE");
        env::set_var("JACS_STORAGE", "remote");

        let r =
            resolve_storage_backend(None, Some(Path::new("/nonexistent/path.json"))).expect("ok");

        restore_env("JACS_DEFAULT_STORAGE", saved_default);
        restore_env("JACS_STORAGE", saved_legacy);
        assert_eq!(r, "fs");
    }

    #[test]
    fn resolve_storage_backend_reads_jacs_default_storage_config_key() {
        let _guard = crate::test_support::env_lock();
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("jacs.config.json");
        fs::write(
            &config_path,
            r#"{"jacs_default_storage": "remote", "default_storage": "fs", "jacsAgentName": "test"}"#,
        )
        .expect("write config");

        let saved_default = env::var("JACS_DEFAULT_STORAGE").ok();
        env::remove_var("JACS_DEFAULT_STORAGE");

        let r = resolve_storage_backend(None, Some(&config_path)).expect("ok");

        restore_env("JACS_DEFAULT_STORAGE", saved_default);
        assert_eq!(r, "remote");
    }

    #[test]
    fn redacted_display_uses_jacs_default_storage_env_source() {
        let _guard = crate::test_support::env_lock();
        let saved = env::var("JACS_DEFAULT_STORAGE").ok();
        env::set_var("JACS_DEFAULT_STORAGE", "remote");

        let summary = redacted_display(None, Some(Path::new("/nonexistent/path.json")));

        restore_env("JACS_DEFAULT_STORAGE", saved);
        assert_eq!(summary.backend, "remote");
        assert_eq!(summary.source, "JACS_DEFAULT_STORAGE env var");
    }

    fn restore_env(key: &str, saved: Option<String>) {
        if let Some(v) = saved {
            env::set_var(key, v);
        } else {
            env::remove_var(key);
        }
    }

    #[test]
    fn resolve_storage_backend_explicit_param_remote() {
        let r = resolve_storage_backend(Some("remote"), None).expect("ok");
        assert_eq!(r, "remote");
    }

    #[test]
    fn redacted_display_includes_remote() {
        let summary = redacted_display(Some("remote"), None);
        assert_eq!(summary.backend, "remote");
    }

    #[test]
    fn redacted_display_explicit_flag() {
        let summary = redacted_display(Some("rusqlite"), None);
        assert_eq!(summary.backend, "rusqlite");
        assert_eq!(summary.source, "--storage flag");
        // Display trait should never expose connection details
        let rendered = format!("{}", summary);
        assert!(rendered.contains("rusqlite"));
        assert!(rendered.contains("--storage flag"));
    }

    #[test]
    fn redacted_display_default_fallback() {
        let _guard = crate::test_support::env_lock();
        let orig = env::var("JACS_DEFAULT_STORAGE").ok();
        env::remove_var("JACS_DEFAULT_STORAGE");

        let summary = redacted_display(None, Some(Path::new("/nonexistent/path.json")));
        assert_eq!(summary.backend, "fs");
        assert_eq!(summary.source, "default");

        restore_env("JACS_DEFAULT_STORAGE", orig);
    }

    #[test]
    fn redacted_display_from_config_file() {
        let _guard = crate::test_support::env_lock();
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("jacs.config.json");
        fs::write(
            &config_path,
            r#"{"default_storage": "sqlite", "jacsAgentName": "test"}"#,
        )
        .expect("write config");

        let orig = env::var("JACS_DEFAULT_STORAGE").ok();
        env::remove_var("JACS_DEFAULT_STORAGE");

        let summary = redacted_display(None, Some(&config_path));
        assert_eq!(summary.backend, "sqlite");
        assert_eq!(summary.source, "config file");

        restore_env("JACS_DEFAULT_STORAGE", orig);
    }

    #[test]
    fn resolve_log_filter_rust_log_wins() {
        let _guard = crate::test_support::env_lock();
        let saved = env::var("RUST_LOG").ok();
        env::set_var("RUST_LOG", "trace,rmcp=debug");

        let filter = resolve_log_filter(Some(Path::new("/nonexistent/path.json")));

        restore_env("RUST_LOG", saved);
        assert_eq!(filter, "trace,rmcp=debug");
    }

    #[test]
    fn resolve_log_filter_reads_observability_logs_level() {
        let _guard = crate::test_support::env_lock();
        let saved = env::var("RUST_LOG").ok();
        env::remove_var("RUST_LOG");
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("jacs.config.json");
        fs::write(
            &config_path,
            r#"{"observability":{"logs":{"level":"debug"}}}"#,
        )
        .expect("write config");

        let filter = resolve_log_filter(Some(&config_path));

        restore_env("RUST_LOG", saved);
        assert_eq!(filter, "debug,rmcp=warn");
    }

    #[test]
    fn load_config_reads_agent_email() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("jacs.config.json");
        fs::write(
            &config_path,
            r#"{"jacsAgentName": "bot", "jacsId": "a-1", "agent_email": "bot@hai.ai"}"#,
        )
        .expect("write config");

        let cfg = load_config(Some(&config_path)).expect("load");
        assert_eq!(cfg.agent_email, Some("bot@hai.ai".to_string()));
    }

    #[test]
    fn load_config_agent_email_absent_is_none() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("jacs.config.json");
        fs::write(&config_path, r#"{"jacsAgentName": "bot", "jacsId": "a-1"}"#)
            .expect("write config");

        let cfg = load_config(Some(&config_path)).expect("load");
        assert_eq!(cfg.agent_email, None);
    }

    #[test]
    fn resolve_remote_defaults_to_false() {
        let _guard = crate::test_support::env_lock();
        let saved_remote = env::var("JACS_REMOTE").ok();
        let saved_storage = env::var("JACS_DEFAULT_STORAGE").ok();
        env::remove_var("JACS_REMOTE");
        env::remove_var("JACS_DEFAULT_STORAGE");

        let result = resolve_remote(None, Some(Path::new("/nonexistent/path.json")));
        assert!(!result, "default should be false");

        restore_env("JACS_REMOTE", saved_remote);
        restore_env("JACS_DEFAULT_STORAGE", saved_storage);
    }

    #[test]
    fn resolve_remote_explicit_true_overrides() {
        let _guard = crate::test_support::env_lock();
        let saved = env::var("JACS_REMOTE").ok();
        env::set_var("JACS_REMOTE", "false");

        let result = resolve_remote(Some(true), None);
        assert!(result, "explicit true must override env var");

        restore_env("JACS_REMOTE", saved);
    }

    #[test]
    fn resolve_remote_env_var_true() {
        let _guard = crate::test_support::env_lock();
        let saved = env::var("JACS_REMOTE").ok();
        env::set_var("JACS_REMOTE", "true");

        let result = resolve_remote(None, Some(Path::new("/nonexistent/path.json")));
        assert!(result, "JACS_REMOTE=true should be true");

        restore_env("JACS_REMOTE", saved);
    }

    #[test]
    fn resolve_remote_env_var_one() {
        let _guard = crate::test_support::env_lock();
        let saved = env::var("JACS_REMOTE").ok();
        env::set_var("JACS_REMOTE", "1");

        let result = resolve_remote(None, Some(Path::new("/nonexistent/path.json")));
        assert!(result, "JACS_REMOTE=1 should be true");

        restore_env("JACS_REMOTE", saved);
    }

    #[test]
    fn resolve_remote_config_file_field() {
        let _guard = crate::test_support::env_lock();
        let saved_remote = env::var("JACS_REMOTE").ok();
        let saved_storage = env::var("JACS_DEFAULT_STORAGE").ok();
        env::remove_var("JACS_REMOTE");
        env::remove_var("JACS_DEFAULT_STORAGE");

        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("jacs.config.json");
        fs::write(&config_path, r#"{"remote": true, "jacsAgentName": "bot"}"#)
            .expect("write config");

        let result = resolve_remote(None, Some(&config_path));
        assert!(result, "config file remote=true should be true");

        restore_env("JACS_REMOTE", saved_remote);
        restore_env("JACS_DEFAULT_STORAGE", saved_storage);
    }

    #[test]
    fn resolve_remote_backward_compat_jacs_default_storage_remote() {
        let _guard = crate::test_support::env_lock();
        let saved_remote = env::var("JACS_REMOTE").ok();
        let saved_storage = env::var("JACS_DEFAULT_STORAGE").ok();
        env::remove_var("JACS_REMOTE");
        env::set_var("JACS_DEFAULT_STORAGE", "remote");

        let result = resolve_remote(None, Some(Path::new("/nonexistent/path.json")));
        assert!(
            result,
            "JACS_DEFAULT_STORAGE=remote should imply remote=true"
        );

        restore_env("JACS_REMOTE", saved_remote);
        restore_env("JACS_DEFAULT_STORAGE", saved_storage);
    }

    #[test]
    fn resolve_remote_false_when_jacs_default_storage_is_fs() {
        let _guard = crate::test_support::env_lock();
        let saved_remote = env::var("JACS_REMOTE").ok();
        let saved_storage = env::var("JACS_DEFAULT_STORAGE").ok();
        env::remove_var("JACS_REMOTE");
        env::set_var("JACS_DEFAULT_STORAGE", "fs");

        let result = resolve_remote(None, Some(Path::new("/nonexistent/path.json")));
        assert!(
            !result,
            "JACS_DEFAULT_STORAGE=fs should not imply remote=true"
        );

        restore_env("JACS_REMOTE", saved_remote);
        restore_env("JACS_DEFAULT_STORAGE", saved_storage);
    }
}
