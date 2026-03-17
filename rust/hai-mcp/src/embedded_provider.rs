use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};

use anyhow::{anyhow, Context as _};
use haiai::key_format::normalize_public_key_pem;
use haiai::{HaiError, JacsProvider, Result as HaiResult, SignedPayload};
use jacs::agent::boilerplate::BoilerPlate;
use jacs::agent::Agent;
use jacs::crypt::KeyManager;
use jacs_binding_core::AgentWrapper;
use serde_json::Value;

const MISSING_JACS_CONFIG_MESSAGE: &str = "JACS_CONFIG environment variable is not set.\n\
\n\
To use hai-mcp, you need to:\n\
1. Create a jacs.config.json file with your agent configuration\n\
2. Set JACS_CONFIG=/path/to/jacs.config.json\n\
\n\
Alternatively, run from a directory that contains jacs.config.json (current directory is checked).";

pub struct LoadedSharedAgent {
    inner: Arc<StdMutex<Agent>>,
    config_path: PathBuf,
}

impl LoadedSharedAgent {
    /// Load agent from config. Resolution order:
    /// 1. JACS_CONFIG env var
    /// 2. JACS_CONFIG_PATH env var
    /// 3. ./jacs.config.json in the current directory
    pub fn load_from_config_env() -> anyhow::Result<Self> {
        let default_path = PathBuf::from("./jacs.config.json");
        let cfg_path = std::env::var("JACS_CONFIG")
            .ok()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("JACS_CONFIG_PATH")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| default_path.clone());
        if cfg_path == default_path && !cfg_path.exists() {
            return Err(anyhow!(MISSING_JACS_CONFIG_MESSAGE));
        }
        Self::load_from_config_path(cfg_path)
    }

    pub fn load_from_config_path(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let config_path = absolutize_path(path.as_ref())?;
        if !config_path.exists() {
            return Err(anyhow!(
                "Config file not found at '{}'. Create a jacs.config.json file or set JACS_CONFIG to an existing path.",
                config_path.display()
            ));
        }

        let cfg_str = fs::read_to_string(&config_path).map_err(|error| {
            anyhow!(
                "Failed to read config file '{}': {}. Check file permissions.",
                config_path.display(),
                error
            )
        })?;

        let resolved_cfg_str = resolve_relative_config_paths(&cfg_str, &config_path)?;
        #[allow(deprecated)]
        let _ =
            jacs::config::set_env_vars(true, Some(&resolved_cfg_str), false).map_err(|error| {
                anyhow!("Invalid config file '{}': {}", config_path.display(), error)
            })?;

        let mut agent = jacs::get_empty_agent();
        agent
            .load_by_config(config_path.to_string_lossy().into_owned())
            .map_err(|error| anyhow!("Failed to load agent: {}", error))?;

        Ok(Self {
            inner: Arc::new(StdMutex::new(agent)),
            config_path,
        })
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn agent_wrapper(&self) -> AgentWrapper {
        AgentWrapper::from_inner(Arc::clone(&self.inner))
    }

    pub fn embedded_provider(&self) -> HaiResult<EmbeddedJacsProvider> {
        EmbeddedJacsProvider::new(Arc::clone(&self.inner), self.config_path.clone())
    }
}

#[derive(Clone)]
pub struct EmbeddedJacsProvider {
    inner: Arc<StdMutex<Agent>>,
    jacs_id: String,
    algorithm: String,
    public_key_pem: String,
}

impl EmbeddedJacsProvider {
    pub fn new(inner: Arc<StdMutex<Agent>>, _config_path: PathBuf) -> HaiResult<Self> {
        let (jacs_id, algorithm, public_key_pem) = {
            let agent = inner.lock().map_err(|error| {
                HaiError::Provider(format!("failed to lock JACS agent: {error}"))
            })?;
            let jacs_id = agent.get_id().map_err(|error| {
                HaiError::Provider(format!("failed to resolve JACS agent id: {error}"))
            })?;
            let algorithm = agent.get_key_algorithm().cloned().ok_or_else(|| {
                HaiError::Provider(
                    "Cannot resolve signing algorithm from embedded JACS agent.".to_string(),
                )
            })?;
            let public_key = agent.get_public_key().map_err(|error| {
                HaiError::Provider(format!("failed to read embedded public key bytes: {error}"))
            })?;
            (jacs_id, algorithm, normalize_public_key_pem(&public_key))
        };

        Ok(Self {
            inner,
            jacs_id,
            algorithm,
            public_key_pem,
        })
    }

    #[cfg(test)]
    pub fn testing(jacs_id: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(StdMutex::new(jacs::get_empty_agent())),
            jacs_id: jacs_id.into(),
            algorithm: "test".to_string(),
            public_key_pem: "-----BEGIN PUBLIC KEY-----\nTEST\n-----END PUBLIC KEY-----\n"
                .to_string(),
        }
    }

    pub fn export_agent_json(&self) -> HaiResult<String> {
        let agent = self
            .inner
            .lock()
            .map_err(|error| HaiError::Provider(format!("failed to lock JACS agent: {error}")))?;
        let value = agent
            .get_value()
            .cloned()
            .ok_or_else(|| HaiError::Provider("embedded JACS agent is not loaded".to_string()))?;
        serde_json::to_string(&value).map_err(|error| {
            HaiError::Provider(format!("failed to export embedded agent json: {error}"))
        })
    }

    pub fn public_key_pem(&self) -> HaiResult<String> {
        Ok(self.public_key_pem.clone())
    }
}

impl JacsProvider for EmbeddedJacsProvider {
    fn jacs_id(&self) -> &str {
        &self.jacs_id
    }

    fn sign_string(&self, message: &str) -> HaiResult<String> {
        let mut agent = self
            .inner
            .lock()
            .map_err(|error| HaiError::Provider(format!("failed to lock JACS agent: {error}")))?;
        agent.sign_string(message).map_err(|error| {
            HaiError::Provider(format!("embedded JACS sign_string failed: {error}"))
        })
    }

    fn sign_bytes(&self, data: &[u8]) -> HaiResult<Vec<u8>> {
        let mut agent = self
            .inner
            .lock()
            .map_err(|error| HaiError::Provider(format!("failed to lock JACS agent: {error}")))?;
        jacs::agent::Agent::sign_bytes(&mut agent, data).map_err(|error| {
            HaiError::Provider(format!("embedded JACS sign_bytes failed: {error}"))
        })
    }

    fn key_id(&self) -> &str {
        &self.jacs_id
    }

    fn algorithm(&self) -> &str {
        &self.algorithm
    }

    fn canonical_json(&self, value: &Value) -> HaiResult<String> {
        Ok(jacs::protocol::canonicalize_json(value))
    }

    fn verify_a2a_artifact(&self, wrapped_json: &str) -> HaiResult<String> {
        let wrapped: Value = serde_json::from_str(wrapped_json)?;
        let agent = self
            .inner
            .lock()
            .map_err(|error| HaiError::Provider(format!("failed to lock JACS agent: {error}")))?;
        let result =
            jacs::a2a::provenance::verify_wrapped_artifact(&agent, &wrapped).map_err(|error| {
                HaiError::Provider(format!("embedded JACS A2A verification failed: {error}"))
            })?;
        serde_json::to_string(&result).map_err(|error| {
            HaiError::Provider(format!(
                "failed to serialize A2A verification result: {error}"
            ))
        })
    }

    fn sign_response(&self, payload: &Value) -> HaiResult<SignedPayload> {
        let mut agent = self
            .inner
            .lock()
            .map_err(|error| HaiError::Provider(format!("failed to lock JACS agent: {error}")))?;

        let envelope = jacs::protocol::sign_response(&mut agent, payload)
            .map_err(|error| HaiError::Provider(format!("JACS sign_response failed: {error}")))?;

        Ok(SignedPayload {
            signed_document: serde_json::to_string(&envelope)?,
            agent_jacs_id: self.jacs_id.clone(),
        })
    }
}

fn absolutize_path(path: &Path) -> anyhow::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .context("Failed to determine current working directory")?
            .join(path)
            .pipe(Ok)
    }
}

fn resolve_relative_config_paths(config_json: &str, config_path: &Path) -> anyhow::Result<String> {
    let mut value: serde_json::Value =
        serde_json::from_str(config_json).context("Config file is not valid JSON")?;
    let config_dir = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    for field in ["jacs_data_directory", "jacs_key_directory"] {
        if let Some(path_value) = value.get_mut(field) {
            if let Some(path_str) = path_value.as_str() {
                let path = Path::new(path_str);
                if !path.is_absolute() {
                    *path_value = serde_json::Value::String(
                        config_dir.join(path).to_string_lossy().into_owned(),
                    );
                }
            }
        }
    }

    serde_json::to_string(&value).context("Failed to serialize resolved config")
}

trait Pipe: Sized {
    fn pipe<T>(self, func: impl FnOnce(Self) -> T) -> T {
        func(self)
    }
}

impl<T> Pipe for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use haiai::LocalJacsProvider;
    use tempfile::TempDir;

    fn jacs_fixture_config_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/jacs-agent/jacs.config.json")
            .canonicalize()
            .expect("fixtures/jacs-agent/jacs.config.json must exist in repo")
    }

    fn write_temp_fixture_config() -> (TempDir, PathBuf) {
        std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", "secretpassord");

        let source = jacs_fixture_config_path();
        let source_dir = source.parent().expect("fixture config dir");
        let mut value: Value =
            serde_json::from_str(&fs::read_to_string(&source).expect("read fixture config"))
                .expect("parse fixture config");

        let temp_dir = tempfile::tempdir().expect("tempdir");

        // Copy key directory to temp
        let source_key_dir = value
            .get("jacs_key_directory")
            .and_then(Value::as_str)
            .map(|p| if PathBuf::from(p).is_absolute() { PathBuf::from(p) } else { source_dir.join(p) })
            .expect("key dir in config");
        let temp_key_dir = temp_dir.path().join("keys");
        fs::create_dir_all(&temp_key_dir).expect("create temp key dir");
        for entry in fs::read_dir(&source_key_dir).expect("read key dir") {
            let entry = entry.expect("key dir entry");
            fs::copy(entry.path(), temp_key_dir.join(entry.file_name())).expect("copy key file");
        }

        // Copy agent data to temp, converting underscore filenames to colon
        // (colons are illegal on Windows, so fixtures use underscores in git)
        let source_data_dir = value
            .get("jacs_data_directory")
            .and_then(Value::as_str)
            .map(|p| if PathBuf::from(p).is_absolute() { PathBuf::from(p) } else { source_dir.join(p) })
            .expect("data dir in config");
        let temp_data_dir = temp_dir.path().join("data");
        fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) {
            fs::create_dir_all(dst).expect("create dir");
            for entry in fs::read_dir(src).expect("read dir") {
                let entry = entry.expect("dir entry");
                let src_path = entry.path();
                // Convert underscores back to colons for JACS {id}:{version} filenames
                let name = entry.file_name().to_string_lossy().replace('_', ":");
                let dst_path = dst.join(&name);
                if src_path.is_dir() {
                    copy_dir_recursive(&src_path, &dst_path);
                } else {
                    fs::copy(&src_path, &dst_path).expect("copy file");
                }
            }
        }
        copy_dir_recursive(&source_data_dir, &temp_data_dir);

        // Point config at temp directories
        value["jacs_data_directory"] = Value::String(temp_data_dir.to_string_lossy().into_owned());
        value["jacs_key_directory"] = Value::String(temp_key_dir.to_string_lossy().into_owned());

        let config_path = temp_dir.path().join("embedded-jacs.config.json");
        fs::write(
            &config_path,
            serde_json::to_vec_pretty(&value).expect("encode fixture config"),
        )
        .expect("write temp config");

        (temp_dir, config_path)
    }

    #[test]
    fn embedded_provider_matches_local_provider_registration_material() {
        let (_temp_dir, config_path) = write_temp_fixture_config();
        let shared = LoadedSharedAgent::load_from_config_path(&config_path).expect("load shared");
        let embedded = shared.embedded_provider().expect("embedded provider");
        let local = LocalJacsProvider::from_config_path(Some(config_path.as_path()))
            .expect("local provider");

        assert_eq!(embedded.jacs_id(), local.jacs_id());
        assert_eq!(embedded.algorithm(), local.algorithm());
        assert_eq!(
            embedded.public_key_pem().unwrap(),
            local.public_key_pem().unwrap()
        );

        let embedded_json: Value =
            serde_json::from_str(&embedded.export_agent_json().unwrap()).expect("embedded json");
        let local_json: Value =
            serde_json::from_str(&local.export_agent_json().unwrap()).expect("local json");
        assert_eq!(embedded_json, local_json);
    }
}
