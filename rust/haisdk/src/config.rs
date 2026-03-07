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

    let jacs_agent_name = get_string(&data, &["jacsAgentName", "agent_name"])
        .unwrap_or_else(|| "unnamed-agent".to_string());
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

    let private_key_raw = get_string(&data, &["jacsPrivateKeyPath", "private_key_path"]);
    let jacs_private_key_path = private_key_raw.map(|p| {
        if Path::new(&p).is_absolute() {
            PathBuf::from(p)
        } else {
            config_dir.join(p)
        }
    });

    Ok(AgentConfig {
        jacs_agent_name,
        jacs_agent_version,
        jacs_key_dir,
        jacs_id,
        jacs_private_key_path,
        source_path,
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
}
