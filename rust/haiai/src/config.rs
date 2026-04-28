use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{HaiError, Result};

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
/// 2. `JACS_STORAGE` env var
/// 3. `default_storage` field in `jacs.config.json`
/// 4. `"fs"` default
pub fn resolve_storage_backend(
    explicit: Option<&str>,
    config_path: Option<&Path>,
) -> Result<String> {
    // Priority 1: explicit parameter
    if let Some(label) = explicit {
        return resolve_storage_backend_label(label);
    }

    // Priority 2: JACS_STORAGE env var
    if let Ok(label) = env::var("JACS_STORAGE") {
        if !label.is_empty() {
            return resolve_storage_backend_label(&label);
        }
    }

    // Priority 3: default_storage in config
    let config_path_resolved = resolve_config_path(config_path);
    if config_path_resolved.is_file() {
        if let Ok(raw) = fs::read_to_string(&config_path_resolved) {
            if let Ok(data) = serde_json::from_str::<Value>(&raw) {
                if let Some(label) = get_string(&data, &["default_storage", "defaultStorage"]) {
                    return resolve_storage_backend_label(&label);
                }
            }
        }
    }

    // Priority 4: default to fs
    Ok("fs".to_string())
}

/// A storage configuration summary safe for logging/display.
///
/// Never includes passwords, connection strings, or credentials.
/// Complies with PRD Section 4.4.6: "Never log backend configuration details."
#[derive(Debug, Clone, Serialize)]
pub struct StorageConfigSummary {
    pub backend: String,
    pub source: &'static str,
}

impl std::fmt::Display for StorageConfigSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "backend={} (from {})", self.backend, self.source)
    }
}

/// Return a redacted summary of the resolved storage configuration.
///
/// This is safe for logging, CLI output, and error messages.
/// It reports the backend label and its resolution source, but never
/// exposes connection strings, passwords, or file system paths.
pub fn redacted_display(explicit: Option<&str>, config_path: Option<&Path>) -> StorageConfigSummary {
    // Priority 1: explicit parameter
    if let Some(label) = explicit {
        return StorageConfigSummary {
            backend: label.to_string(),
            source: "--storage flag",
        };
    }

    // Priority 2: JACS_STORAGE env var
    if let Ok(label) = env::var("JACS_STORAGE") {
        if !label.is_empty() {
            return StorageConfigSummary {
                backend: label,
                source: "JACS_STORAGE env var",
            };
        }
    }

    // Priority 3: config file
    let config_path_resolved = resolve_config_path(config_path);
    if config_path_resolved.is_file() {
        if let Ok(raw) = fs::read_to_string(&config_path_resolved) {
            if let Ok(data) = serde_json::from_str::<Value>(&raw) {
                if let Some(label) = get_string(&data, &["default_storage", "defaultStorage"]) {
                    return StorageConfigSummary {
                        backend: label,
                        source: "config file",
                    };
                }
            }
        }
    }

    // Priority 4: default
    StorageConfigSummary {
        backend: "fs".to_string(),
        source: "default",
    }
}

fn get_string(data: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = data.get(key).and_then(Value::as_str) {
            return Some(value.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Mutex;

    use super::*;

    /// Shared serialisation guard for tests that mutate `JACS_STORAGE` env var.
    /// Without this, parallel test threads race and produce non-deterministic failures.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

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
        assert!(result.is_err(), "missing jacs_agent_name should be an error");
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

        let cfg = load_config(Some(&config_path)).expect("should succeed with defaults for version and key_dir");
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
    fn resolve_storage_backend_env_var_remote() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let saved = env::var("JACS_STORAGE").ok();
        env::set_var("JACS_STORAGE", "remote");
        let r = resolve_storage_backend(None, Some(Path::new("/nonexistent/path.json")))
            .expect("ok");
        if let Some(v) = saved {
            env::set_var("JACS_STORAGE", v);
        } else {
            env::remove_var("JACS_STORAGE");
        }
        assert_eq!(r, "remote");
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
        let _guard = ENV_LOCK.lock().expect("env lock");
        let orig = env::var("JACS_STORAGE").ok();
        env::remove_var("JACS_STORAGE");

        let summary = redacted_display(None, Some(Path::new("/nonexistent/path.json")));
        assert_eq!(summary.backend, "fs");
        assert_eq!(summary.source, "default");

        if let Some(val) = orig {
            env::set_var("JACS_STORAGE", val);
        }
    }

    #[test]
    fn redacted_display_from_config_file() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("jacs.config.json");
        fs::write(
            &config_path,
            r#"{"default_storage": "sqlite", "jacsAgentName": "test"}"#,
        )
        .expect("write config");

        let orig = env::var("JACS_STORAGE").ok();
        env::remove_var("JACS_STORAGE");

        let summary = redacted_display(None, Some(&config_path));
        assert_eq!(summary.backend, "sqlite");
        assert_eq!(summary.source, "config file");

        if let Some(val) = orig {
            env::set_var("JACS_STORAGE", val);
        }
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
        fs::write(
            &config_path,
            r#"{"jacsAgentName": "bot", "jacsId": "a-1"}"#,
        )
        .expect("write config");

        let cfg = load_config(Some(&config_path)).expect("load");
        assert_eq!(cfg.agent_email, None);
    }
}
