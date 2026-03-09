use std::env;
use std::path::{Path, PathBuf};
use std::sync::Mutex;


use jacs::agent::boilerplate::BoilerPlate;
use jacs::crypt::KeyManager;
use jacs::simple::{self, CreateAgentParams, SimpleAgent};
use serde_json::Value;

use crate::error::{HaiError, Result};
use crate::jacs::JacsProvider;
#[cfg(feature = "jacs-crate")]
use crate::types::RotationResult;
use crate::types::{CreateAgentOptions, CreateAgentResult, SignedPayload};

/// Local JACS-backed provider using the canonical Rust `jacs` crate.
///
/// This adapter loads the local agent configured by `jacs.config.json` and
/// delegates signing operations to JACS runtime methods.
pub struct LocalJacsProvider {
    agent: Mutex<jacs::agent::Agent>,
    jacs_id: String,
    algorithm: String,
    config_path: PathBuf,
}

impl LocalJacsProvider {
    pub fn from_config_path(config_path: Option<&Path>) -> Result<Self> {
        let config_path = resolve_jacs_config_path(config_path);
        let mut agent = jacs::get_empty_agent();
        agent
            .load_by_config(config_path.display().to_string())
            .map_err(|e| {
                HaiError::Provider(format!(
                    "failed to load JACS agent from {}: {e}",
                    config_path.display()
                ))
            })?;

        let jacs_id = agent
            .get_id()
            .map_err(|e| HaiError::Provider(format!("failed to resolve JACS agent id: {e}")))?;

        // Resolve algorithm from the loaded agent (PRD lines 86-98).
        // Fail fast if the algorithm cannot be determined — a silent default
        // would cause algorithm mismatches during verification (Issue 039).
        let algorithm = agent.get_key_algorithm().cloned().ok_or_else(|| {
            HaiError::Provider(
                "Cannot resolve signing algorithm from JACS agent. \
                 Ensure the agent was created with a valid key algorithm."
                    .to_string(),
            )
        })?;

        Ok(Self {
            agent: Mutex::new(agent),
            jacs_id,
            algorithm,
            config_path,
        })
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn export_agent_json(&self) -> Result<String> {
        let simple = self.load_simple_agent()?;
        simple
            .export_agent()
            .map_err(|e| HaiError::Provider(format!("failed to export JACS agent json: {e}")))
    }

    pub fn public_key_pem(&self) -> Result<String> {
        let simple = self.load_simple_agent()?;
        simple
            .get_public_key_pem()
            .map_err(|e| HaiError::Provider(format!("failed to read JACS public key pem: {e}")))
    }

    pub fn create_agent(params: CreateAgentParams) -> Result<simple::AgentInfo> {
        SimpleAgent::create_with_params(params)
            .map(|(_, info)| info)
            .map_err(|e| HaiError::Provider(format!("failed to create JACS agent: {e}")))
    }

    pub fn create_agent_with_options(options: &CreateAgentOptions) -> Result<CreateAgentResult> {
        let mut params = CreateAgentParams {
            name: options.name.clone(),
            password: options.password.clone(),
            ..CreateAgentParams::default()
        };

        if let Some(v) = &options.algorithm {
            params.algorithm = v.clone();
        }
        if let Some(v) = &options.data_directory {
            params.data_directory = v.clone();
        }
        if let Some(v) = &options.key_directory {
            params.key_directory = v.clone();
        }
        if let Some(v) = &options.config_path {
            params.config_path = v.clone();
        }
        if let Some(v) = &options.agent_type {
            params.agent_type = v.clone();
        }
        if let Some(v) = &options.description {
            params.description = v.clone();
        }
        if let Some(v) = &options.domain {
            params.domain = v.clone();
        }
        if let Some(v) = &options.default_storage {
            params.default_storage = v.clone();
        }

        let info = Self::create_agent(params)?;
        Ok(map_agent_info(info))
    }

    fn load_simple_agent(&self) -> Result<SimpleAgent> {
        SimpleAgent::load(Some(&self.config_path.display().to_string()), Some(false)).map_err(|e| {
            HaiError::Provider(format!(
                "failed to load SimpleAgent from {}: {e}",
                self.config_path.display()
            ))
        })
    }
}

fn map_agent_info(info: simple::AgentInfo) -> CreateAgentResult {
    CreateAgentResult {
        agent_id: info.agent_id,
        name: info.name,
        public_key_path: info.public_key_path,
        config_path: info.config_path,
        version: info.version,
        algorithm: info.algorithm,
        private_key_path: info.private_key_path,
        data_directory: info.data_directory,
        key_directory: info.key_directory,
        domain: info.domain,
        dns_record: info.dns_record,
    }
}

impl JacsProvider for LocalJacsProvider {
    fn jacs_id(&self) -> &str {
        &self.jacs_id
    }

    fn sign_string(&self, message: &str) -> Result<String> {
        let mut agent = self
            .agent
            .lock()
            .map_err(|e| HaiError::Provider(format!("failed to lock JACS agent: {e}")))?;

        agent
            .sign_string(message)
            .map_err(|e| HaiError::Provider(format!("JACS sign_string failed: {e}")))
    }

    fn sign_bytes(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut agent = self
            .agent
            .lock()
            .map_err(|e| HaiError::Provider(format!("failed to lock JACS agent: {e}")))?;

        // Use Agent::sign_bytes when available (jacs-local path with our changes).
        // For the crates.io jacs-crate path, fall back to sign_string
        // with base64 encoding as a bridge.
        #[cfg(feature = "jacs-crate")]
        {
            jacs::agent::Agent::sign_bytes(&mut *agent, data)
                .map_err(|e| HaiError::Provider(format!("JACS sign_bytes failed: {e}")))
        }
        #[cfg(not(feature = "jacs-crate"))]
        {
            use base64::Engine;
            // Encode data as base64 string, sign it, then decode the signature
            let encoded = base64::engine::general_purpose::STANDARD.encode(data);
            let sig_b64 = agent.sign_string(&encoded).map_err(|e| {
                HaiError::Provider(format!("JACS sign_bytes (via sign_string) failed: {e}"))
            })?;
            base64::engine::general_purpose::STANDARD
                .decode(&sig_b64)
                .map_err(|e| HaiError::Provider(format!("JACS sign_bytes decode failed: {e}")))
        }
    }

    fn key_id(&self) -> &str {
        &self.jacs_id
    }

    fn algorithm(&self) -> &str {
        &self.algorithm
    }

    fn canonical_json(&self, value: &Value) -> Result<String> {
        Ok(jacs::protocol::canonicalize_json(value))
    }

    fn verify_a2a_artifact(&self, wrapped_json: &str) -> Result<String> {
        // Delegate to JACS core for proper cryptographic verification.
        // Uses the underlying Agent directly since SimpleAgent::verify_a2a_artifact
        // may not be available in all jacs crate versions.
        let wrapped: Value = serde_json::from_str(wrapped_json)?;
        let agent = self
            .agent
            .lock()
            .map_err(|e| HaiError::Provider(format!("failed to lock JACS agent: {e}")))?;

        let result =
            jacs::a2a::provenance::verify_wrapped_artifact(&agent, &wrapped).map_err(|e| {
                HaiError::Provider(format!("JACS A2A artifact verification failed: {e}"))
            })?;

        serde_json::to_string(&result).map_err(|e| {
            HaiError::Provider(format!("failed to serialize verification result: {e}"))
        })
    }

    fn sign_response(&self, payload: &Value) -> Result<SignedPayload> {
        let mut agent = self
            .agent
            .lock()
            .map_err(|e| HaiError::Provider(format!("failed to lock JACS agent: {e}")))?;

        let envelope = jacs::protocol::sign_response(&mut agent, payload)
            .map_err(|e| HaiError::Provider(format!("JACS sign_response failed: {e}")))?;

        Ok(SignedPayload {
            signed_document: serde_json::to_string(&envelope)?,
            agent_jacs_id: self.jacs_id.clone(),
        })
    }

    fn sign_email_locally(&self, raw_email: &[u8]) -> Result<Vec<u8>> {
        let simple = self.load_simple_agent()?;
        jacs::email::sign_email(raw_email, &simple)
            .map_err(|e| HaiError::Provider(format!("JACS email signing failed: {e}")))
    }

    #[cfg(feature = "jacs-crate")]
    fn rotate(&self) -> Result<RotationResult> {
        let simple = self.load_simple_agent()?;
        let jacs_result = simple
            .rotate()
            .map_err(|e| HaiError::Provider(format!("JACS key rotation failed: {e}")))?;

        // Reload the agent so in-memory state reflects the rotated keys
        let mut agent = self
            .agent
            .lock()
            .map_err(|e| HaiError::Provider(format!("failed to lock JACS agent: {e}")))?;
        let mut new_agent = jacs::get_empty_agent();
        new_agent
            .load_by_config(self.config_path.display().to_string())
            .map_err(|e| {
                HaiError::Provider(format!("failed to reload JACS agent after rotation: {e}"))
            })?;
        *agent = new_agent;

        Ok(RotationResult {
            jacs_id: jacs_result.jacs_id,
            old_version: jacs_result.old_version,
            new_version: jacs_result.new_version,
            new_public_key_hash: jacs_result.new_public_key_hash,
            registered_with_hai: false,
            signed_agent_json: jacs_result.signed_agent_json,
        })
    }
}

fn resolve_jacs_config_path(config_path: Option<&Path>) -> PathBuf {
    if let Some(path) = config_path {
        return path.to_path_buf();
    }

    if let Ok(path) = env::var("JACS_CONFIG") {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }

    if let Ok(path) = env::var("JACS_CONFIG_PATH") {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }

    PathBuf::from("./jacs.config.json")
}
