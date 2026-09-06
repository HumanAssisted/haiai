use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};

use anyhow::{anyhow, Context as _};
use haiai::jacs_local::{lock_jacs_config_env, JacsConfigEnvSnapshot};
use haiai::key_format::normalize_public_key_pem;
use haiai::{
    HaiError, JacsMediaProvider, JacsProvider, MediaVerificationResult, Result as HaiResult,
    SignImageOptions, SignTextOptions, SignTextOutcome, SignedMedia, SignedPayload,
    VerifyImageOptions, VerifyTextOptions, VerifyTextResult,
};
use jacs::agent::boilerplate::BoilerPlate;
use jacs::agent::document::DocumentTraits;
use jacs::agent::Agent;
use jacs::crypt::KeyManager;
use jacs::email::JacsSigner;
use jacs::error::JacsError;
use jacs::simple::{SignedDocument, SimpleAgent, VerificationResult};
use jacs_binding_core::AgentWrapper;
use serde_json::{json, Value};

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
    jacs_config_env: JacsConfigEnvSnapshot,
    /// `agent_email` extracted from the config file at load time.
    agent_email: Option<String>,
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

        let _config_env_lock = lock_jacs_config_env();
        let mut config =
            jacs::config::Config::from_file(&config_path.to_string_lossy()).map_err(|error| {
                anyhow!("Invalid config file '{}': {}", config_path.display(), error)
            })?;
        // Defensive: preserve config_dir through apply_env_overrides (Issue 024).
        let saved_config_dir = config.config_dir().map(std::path::PathBuf::from);
        config.apply_env_overrides();
        config.set_config_dir(saved_config_dir);

        // Extract agent_email before config is consumed by Agent::from_config.
        let agent_email = config.agent_email.clone();
        let jacs_config_env = JacsConfigEnvSnapshot::from_config(&config);

        let agent = Agent::from_config(config, None)
            .map_err(|error| anyhow!("Failed to load agent: {}", error))?;

        Ok(Self {
            inner: Arc::new(StdMutex::new(agent)),
            config_path,
            jacs_config_env,
            agent_email,
        })
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    /// The `agent_email` extracted from the config file at load time, if present.
    pub fn agent_email(&self) -> Option<&str> {
        self.agent_email.as_deref()
    }

    pub fn agent_wrapper(&self) -> AgentWrapper {
        AgentWrapper::from_inner(Arc::clone(&self.inner))
    }

    pub fn embedded_provider(&self) -> HaiResult<EmbeddedJacsProvider> {
        EmbeddedJacsProvider::new(
            Arc::clone(&self.inner),
            self.config_path.clone(),
            self.jacs_config_env.clone(),
        )
    }
}

#[derive(Clone)]
pub struct EmbeddedJacsProvider {
    inner: Arc<StdMutex<Agent>>,
    jacs_id: String,
    algorithm: String,
    public_key_pem: String,
    /// Original config metadata; media operations use `inner`, never reload it.
    config_path: PathBuf,
}

impl EmbeddedJacsProvider {
    pub fn new(
        inner: Arc<StdMutex<Agent>>,
        config_path: PathBuf,
        _jacs_config_env: JacsConfigEnvSnapshot,
    ) -> HaiResult<Self> {
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
            config_path,
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
            config_path: PathBuf::from("/dev/null"),
        }
    }

    fn simple_agent(&self) -> SimpleAgent {
        SimpleAgent::from_shared_agent(
            Arc::clone(&self.inner),
            Some(self.config_path.to_string_lossy().into_owned()),
            false,
        )
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

/// Newtype wrapper that implements [`JacsSigner`] for an embedded `Agent`.
///
/// `jacs::email::sign_email` requires an `impl JacsSigner`. Only `SimpleAgent`
/// implements it in jacs, but the underlying operations (`create_document_and_load`,
/// `load_document`, `verify_document_signature`, `verify_hash`) are all available
/// on `Agent`. This wrapper bridges the gap so the MCP server can sign emails
/// the same way the CLI does via `LocalJacsProvider`.
struct AgentSigner(Arc<StdMutex<Agent>>);

impl JacsSigner for AgentSigner {
    fn sign_message(&self, data: &Value) -> Result<SignedDocument, JacsError> {
        let doc_content = json!({
            "jacsType": "document",
            "jacsLevel": "raw",
            "content": data
        });
        let doc_string = doc_content.to_string();

        let mut agent = self.0.lock().map_err(|e| JacsError::Internal {
            message: format!("Failed to acquire agent lock: {e}"),
        })?;

        let jacs_doc = agent
            .create_document_and_load(&doc_string, None, None)
            .map_err(|e| JacsError::SigningFailed {
                reason: format!("{e}"),
            })?;

        let raw = serde_json::to_string(&jacs_doc.value).map_err(|e| JacsError::Internal {
            message: format!("Failed to serialize signed document: {e}"),
        })?;

        Ok(SignedDocument {
            raw,
            document_id: jacs_doc.id,
            agent_id: agent.get_id().unwrap_or_default(),
            timestamp: String::new(),
        })
    }

    fn verify_with_key(
        &self,
        signed_document: &str,
        public_key: Vec<u8>,
    ) -> Result<VerificationResult, JacsError> {
        let mut agent = self.0.lock().map_err(|e| JacsError::Internal {
            message: format!("Failed to acquire agent lock: {e}"),
        })?;

        let jacs_doc =
            agent
                .load_document(signed_document)
                .map_err(|e| JacsError::DocumentMalformed {
                    field: "document".to_string(),
                    reason: e.to_string(),
                })?;

        let document_key = jacs_doc.getkey();
        let mut errors = Vec::new();

        if let Err(e) =
            agent.verify_document_signature(&document_key, None, None, Some(public_key), None)
        {
            errors.push(e.to_string());
        }
        if let Err(e) = agent.verify_hash(&jacs_doc.value) {
            errors.push(format!("Hash verification failed: {e}"));
        }

        let valid = errors.is_empty();
        let signer_id = jacs_doc
            .value
            .pointer("/jacsSignature/agentID")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let timestamp = jacs_doc
            .value
            .pointer("/jacsSignature/date")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let data = jacs_doc
            .value
            .get("content")
            .cloned()
            .unwrap_or_else(|| jacs_doc.value.clone());

        Ok(VerificationResult {
            valid,
            // An explicit verification key establishes integrity, not enrollment.
            identity_binding_status: Default::default(),
            data,
            signer_id,
            signer_name: None,
            timestamp,
            attachments: vec![],
            errors,
        })
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

    fn build_request_auth_header(
        &self,
        method: &str,
        url: &str,
        body: &[u8],
        audience: &str,
    ) -> HaiResult<String> {
        let mut agent = self
            .inner
            .lock()
            .map_err(|error| HaiError::Provider(format!("failed to lock JACS agent: {error}")))?;
        jacs::protocol::build_request_auth_header(&mut agent, method, url, body, audience).map_err(
            |error| {
                HaiError::Provider(format!(
                    "embedded JACS request authentication failed: {error}"
                ))
            },
        )
    }

    fn sign_email_locally(&self, raw_email: &[u8]) -> HaiResult<Vec<u8>> {
        let signer = AgentSigner(Arc::clone(&self.inner));
        jacs::email::sign_email(raw_email, &signer)
            .map_err(|e| HaiError::Provider(format!("JACS email signing failed: {e}")))
    }
}

// =============================================================================
// JacsMediaProvider implementation (Layer 8) — JACS 0.10.0
// =============================================================================
//
// All media operations share the same loaded identity/lock as ordinary
// document signing. JACS owns the crypto and takes the lock when needed;
// wrapping this view performs no file IO or private-key decryption.

impl JacsMediaProvider for EmbeddedJacsProvider {
    fn sign_text_file(&self, path: &str, opts: SignTextOptions) -> HaiResult<SignTextOutcome> {
        let simple = self.simple_agent();
        jacs::simple::advanced::sign_text_file(&simple, path, opts)
            .map_err(|e| HaiError::Provider(format!("sign_text_file failed: {e}")))
    }

    fn verify_text_file(&self, path: &str, opts: VerifyTextOptions) -> HaiResult<VerifyTextResult> {
        let simple = self.simple_agent();
        jacs::simple::advanced::verify_text_file(&simple, path, opts)
            .map_err(|e| HaiError::Provider(format!("verify_text_file failed: {e}")))
    }

    fn sign_image(
        &self,
        in_path: &str,
        out_path: &str,
        opts: SignImageOptions,
    ) -> HaiResult<SignedMedia> {
        let simple = self.simple_agent();
        jacs::simple::advanced::sign_image(&simple, in_path, out_path, opts)
            .map_err(|e| HaiError::Provider(format!("sign_image failed: {e}")))
    }

    fn verify_image(
        &self,
        path: &str,
        opts: VerifyImageOptions,
    ) -> HaiResult<MediaVerificationResult> {
        let simple = self.simple_agent();
        jacs::simple::advanced::verify_image(&simple, path, opts)
            .map_err(|e| HaiError::Provider(format!("verify_image failed: {e}")))
    }

    fn extract_media_signature(&self, path: &str, raw_payload: bool) -> HaiResult<Option<String>> {
        // Same dispatch logic as LocalJacsProvider — JACS exposes two free
        // functions (decoded vs raw); neither needs a SimpleAgent.
        let result = if raw_payload {
            jacs::simple::advanced::extract_media_signature_raw(path)
        } else {
            jacs::simple::advanced::extract_media_signature(path)
        };
        result.map_err(|e| HaiError::Provider(format!("extract_media_signature failed: {e}")))
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

trait Pipe: Sized {
    fn pipe<T>(self, func: impl FnOnce(Self) -> T) -> T {
        func(self)
    }
}

impl<T> Pipe for T {}

#[cfg(test)]
pub(crate) mod tests {
    use std::fs;

    use super::*;
    use haiai::LocalJacsProvider;
    use tempfile::TempDir;

    const FIXTURE_PRIVATE_KEY_PASSWORD: &str = "TestHaiaiEmbedded!2026";
    static FIXTURE_PASSWORD_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    pub(crate) struct GuardedTempDir {
        temp_dir: TempDir,
        _password_guard: FixturePasswordEnvGuard,
    }

    impl GuardedTempDir {
        pub(crate) fn path(&self) -> &Path {
            self.temp_dir.path()
        }
    }

    struct FixturePasswordEnvGuard {
        previous: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl FixturePasswordEnvGuard {
        fn set() -> Self {
            let lock = FIXTURE_PASSWORD_ENV_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let previous = std::env::var("JACS_PRIVATE_KEY_PASSWORD").ok();
            // SAFETY: this test-only RAII guard holds FIXTURE_PASSWORD_ENV_LOCK
            // until it restores the prior process environment on Drop.
            unsafe {
                std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", FIXTURE_PRIVATE_KEY_PASSWORD);
            }
            Self {
                previous,
                _lock: lock,
            }
        }
    }

    impl Drop for FixturePasswordEnvGuard {
        fn drop(&mut self) {
            // SAFETY: the matching fixture lock is still held and prevents
            // another fixture test from observing this restoration window.
            unsafe {
                match self.previous.take() {
                    Some(previous) => std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", previous),
                    None => std::env::remove_var("JACS_PRIVATE_KEY_PASSWORD"),
                }
            }
        }
    }

    /// Crate-internal sibling that exposes `write_temp_fixture_config` to
    /// the `hai_tools::tests` module (issues 004/005/006 MCP integration
    /// tests).
    pub(crate) fn write_temp_fixture_config_pub() -> (GuardedTempDir, PathBuf) {
        write_temp_fixture_config()
    }

    fn write_temp_fixture_config() -> (GuardedTempDir, PathBuf) {
        let password_guard = FixturePasswordEnvGuard::set();
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let workspace_root = temp_dir.path().canonicalize().expect("canonical tempdir");
        let temp_key_dir = workspace_root.join("keys");
        let temp_data_dir = workspace_root.join("data");
        let config_path = workspace_root.join("embedded-jacs.config.json");
        let params = jacs::simple::CreateAgentParams::builder()
            .name("haiai-embedded-provider-test")
            .password(FIXTURE_PRIVATE_KEY_PASSWORD)
            .algorithm("pq2025")
            .data_directory(&temp_data_dir.to_string_lossy())
            .key_directory(&temp_key_dir.to_string_lossy())
            .config_path(&config_path.to_string_lossy())
            .agent_type("ai")
            .description("Isolated HAIAI embedded-provider test fixture")
            .default_storage("fs")
            .no_compat_key(true)
            .build();
        let (_created, _info) = SimpleAgent::create_with_params(params)
            .expect("create signed temporary fixture with explicit password");

        let persisted: Value = serde_json::from_str(
            &fs::read_to_string(&config_path).expect("read generated fixture config"),
        )
        .expect("parse generated fixture config");
        assert!(persisted.get("jacsSignature").is_some());
        assert!(persisted.get("jacsSha256").is_some());

        (
            GuardedTempDir {
                temp_dir,
                _password_guard: password_guard,
            },
            config_path,
        )
    }

    #[test]
    fn embedded_provider_matches_local_provider_registration_material() {
        let (_temp_dir, config_path) = write_temp_fixture_config();
        let shared = LoadedSharedAgent::load_from_config_path(&config_path).expect("load shared");
        let embedded = shared.embedded_provider().expect("embedded provider");
        let local = LocalJacsProvider::from_config_path(Some(config_path.as_path()), None)
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

    #[test]
    fn embedded_provider_authenticates_exact_client_request_with_shared_identity() {
        let (_temp_dir, config_path) = write_temp_fixture_config();
        let shared = LoadedSharedAgent::load_from_config_path(&config_path).expect("load shared");
        let embedded = shared.embedded_provider().expect("embedded provider");
        let public_key = embedded.public_key_pem().expect("public key");
        let agent_json: Value =
            serde_json::from_str(&embedded.export_agent_json().unwrap()).expect("agent json");
        let key_id = format!(
            "{}:{}",
            agent_json["jacsId"].as_str().unwrap(),
            agent_json["jacsVersion"].as_str().unwrap()
        );
        let audience = "embedded-hai-deployment";
        let client = haiai::HaiClient::new(
            embedded,
            haiai::HaiClientOptions {
                base_url: "https://hai.example".into(),
                request_auth_audience: audience.into(),
                ..Default::default()
            },
        )
        .expect("embedded HAI client");
        let url = "https://hai.example/api/items/a%2Fb?x=%2B&x=two";
        let body = [0, 0xff, b'\r', b'\n'];
        let header = client
            .build_request_auth_header("PATCH", url, &body)
            .expect("embedded provider must support authenticated HAI requests");

        jacs::protocol::verify_request_auth_header_with_trusted_key_without_replay(
            &header,
            public_key.as_bytes(),
            &key_id,
            "PATCH",
            url,
            &body,
            audience,
            60,
        )
        .expect("shared registered identity verifies the exact request");
        assert!(
            jacs::protocol::verify_request_auth_header_with_trusted_key_without_replay(
                &header,
                public_key.as_bytes(),
                &key_id,
                "PATCH",
                url,
                b"changed body",
                audience,
                60,
            )
            .is_err(),
            "the embedded adapter must preserve request-byte binding"
        );
    }

    // -------------------------------------------------------------------------
    // TASK_002: JacsMediaProvider impl tests
    // -------------------------------------------------------------------------

    fn make_test_png(width: u32, height: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(width, height, image::Rgba([32, 64, 128, 255]));
        let mut buf = Vec::new();
        let mut cur = std::io::Cursor::new(&mut buf);
        img.write_to(&mut cur, image::ImageFormat::Png)
            .expect("png encode");
        buf
    }

    #[test]
    fn embedded_provider_sign_text_round_trip() {
        let (temp_dir, config_path) = write_temp_fixture_config();
        let shared = LoadedSharedAgent::load_from_config_path(&config_path).expect("load shared");
        let embedded = shared.embedded_provider().expect("embedded provider");

        let path = temp_dir.path().join("hello.md");
        fs::write(&path, b"# Hello\n").expect("write md");

        let outcome = embedded
            .sign_text_file(path.to_str().unwrap(), SignTextOptions::default())
            .expect("sign_text_file");
        assert_eq!(outcome.signers_added, 1);

        let result = embedded
            .verify_text_file(path.to_str().unwrap(), VerifyTextOptions::default())
            .expect("verify_text_file");
        match result {
            VerifyTextResult::Signed { signatures } => {
                assert_eq!(signatures.len(), 1);
                assert_eq!(
                    signatures[0].status,
                    haiai::TextSignatureStatus::Valid,
                    "expected Valid signature"
                );
            }
            other => panic!("expected Signed variant, got {other:?}"),
        }
    }

    #[test]
    fn embedded_provider_sign_image_png_round_trip() {
        let (temp_dir, config_path) = write_temp_fixture_config();
        let shared = LoadedSharedAgent::load_from_config_path(&config_path).expect("load shared");
        let embedded = shared.embedded_provider().expect("embedded provider");

        let in_path = temp_dir.path().join("in.png");
        fs::write(&in_path, make_test_png(32, 32)).expect("write png");
        let out_path = temp_dir.path().join("out.png");

        let signed = embedded
            .sign_image(
                in_path.to_str().unwrap(),
                out_path.to_str().unwrap(),
                SignImageOptions::default(),
            )
            .expect("sign_image");
        assert_eq!(signed.format, "png");

        let result = embedded
            .verify_image(out_path.to_str().unwrap(), VerifyImageOptions::default())
            .expect("verify_image");
        assert_eq!(result.status, haiai::MediaVerifyStatus::Valid);
        assert_eq!(result.signer_id.as_deref(), Some(signed.signer_id.as_str()));
    }

    #[test]
    fn embedded_provider_extract_media_signature_returns_decoded_json() {
        let (temp_dir, config_path) = write_temp_fixture_config();
        let shared = LoadedSharedAgent::load_from_config_path(&config_path).expect("load shared");
        let embedded = shared.embedded_provider().expect("embedded provider");

        let in_path = temp_dir.path().join("ex.png");
        fs::write(&in_path, make_test_png(32, 32)).expect("write png");
        let out_path = temp_dir.path().join("ex_signed.png");
        embedded
            .sign_image(
                in_path.to_str().unwrap(),
                out_path.to_str().unwrap(),
                SignImageOptions::default(),
            )
            .expect("sign");

        let payload = embedded
            .extract_media_signature(out_path.to_str().unwrap(), false)
            .expect("extract")
            .expect("present");
        let parsed: Value = serde_json::from_str(&payload).expect("decoded JSON");
        assert!(
            parsed.is_object(),
            "decoded payload should be a JSON object"
        );
    }

    #[test]
    fn embedded_and_local_produce_compatible_image_signatures() {
        // Sign with LocalJacsProvider, verify with EmbeddedJacsProvider, both
        // pointed at the same authenticated fixture. Proves trait-impl parity.
        let (temp_dir, config_path) = write_temp_fixture_config();
        let local =
            LocalJacsProvider::from_config_path(Some(&config_path), None).expect("local provider");
        let shared = LoadedSharedAgent::load_from_config_path(&config_path).expect("shared");
        let embedded = shared.embedded_provider().expect("embedded provider");

        let in_path = temp_dir.path().join("in.png");
        fs::write(&in_path, make_test_png(32, 32)).expect("write png");
        let out_path = temp_dir.path().join("local_signed.png");

        let signed = local
            .sign_image(
                in_path.to_str().unwrap(),
                out_path.to_str().unwrap(),
                SignImageOptions::default(),
            )
            .expect("local sign_image");

        let result = embedded
            .verify_image(out_path.to_str().unwrap(), VerifyImageOptions::default())
            .expect("embedded verify_image");
        assert_eq!(result.status, haiai::MediaVerifyStatus::Valid);
        assert_eq!(result.signer_id.as_deref(), Some(signed.signer_id.as_str()));
    }

    #[test]
    fn local_and_embedded_media_keep_loaded_identity_without_reloading_config() {
        let (temp_dir, config_path) = write_temp_fixture_config();
        let local = LocalJacsProvider::from_config_path(Some(&config_path), None).unwrap();
        let shared = LoadedSharedAgent::load_from_config_path(&config_path).unwrap();
        let embedded = shared.embedded_provider().unwrap();
        let expected_id = embedded.jacs_id.clone();
        // The original config is unavailable after construction. Every media
        // operation must still use its loaded identity, not reload/unlock one.
        fs::rename(&config_path, temp_dir.path().join("saved.config.json")).unwrap();
        let input = temp_dir.path().join("input.png");
        fs::write(&input, make_test_png(8, 8)).unwrap();
        let providers: [&dyn JacsMediaProvider; 2] = [&local, &embedded];
        for (index, provider) in providers.into_iter().enumerate() {
            let text = temp_dir.path().join(format!("loaded-{index}.md"));
            fs::write(&text, b"# Same loaded agent\n").unwrap();
            let signed_text = provider
                .sign_text_file(text.to_str().unwrap(), SignTextOptions::default())
                .unwrap();
            assert_eq!(signed_text.signers_added, 1);
            match provider
                .verify_text_file(text.to_str().unwrap(), VerifyTextOptions::default())
                .unwrap()
            {
                VerifyTextResult::Signed { signatures } => {
                    assert_eq!(signatures.len(), 1);
                    assert_eq!(signatures[0].status, haiai::TextSignatureStatus::Valid);
                    assert_eq!(signatures[0].signer_id, expected_id);
                }
                other => panic!("Expected verified text, got {other:?}"),
            }
            let output = temp_dir.path().join(format!("loaded-{index}.png"));
            let signed = provider
                .sign_image(
                    input.to_str().unwrap(),
                    output.to_str().unwrap(),
                    SignImageOptions::default(),
                )
                .unwrap();
            assert_eq!(signed.signer_id, expected_id);
            let verified = provider
                .verify_image(output.to_str().unwrap(), VerifyImageOptions::default())
                .unwrap();
            assert_eq!(verified.status, haiai::MediaVerifyStatus::Valid);
            assert_eq!(verified.signer_id.as_deref(), Some(expected_id.as_str()));
            let payload = provider
                .extract_media_signature(output.to_str().unwrap(), false)
                .unwrap()
                .unwrap();
            let document: Value = serde_json::from_str(&payload).unwrap();
            assert_eq!(document["jacsSignature"]["agentID"], expected_id);
        }
    }
}
