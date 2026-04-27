//! # hai-binding-core
//!
//! Shared core logic for HAI SDK language bindings (Python, Node.js, Go).
//!
//! This crate wraps `HaiClient` with a JSON-in/JSON-out interface suitable for
//! FFI consumption. All methods accept and return JSON strings — no
//! language-specific types cross this boundary.
//!
//! Follows the same pattern as JACS's `jacs-binding-core` crate.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use tokio::sync::RwLock;

use std::path::Path;

use haiai::client::{HaiClient, HaiClientOptions, SseConnection, WsConnection};
use haiai::error::HaiError;
use haiai::jacs::{
    media_verify_result_to_json, verify_text_result_to_json, JacsMediaProvider, JacsProvider,
    SignImageOptions, SignTextOptions, StaticJacsProvider, VerifyImageOptions, VerifyTextOptions,
};
use haiai::jacs_local::LocalJacsProvider;
use std::path::PathBuf;

// =============================================================================
// Static tokio runtime for FFI callers
// =============================================================================

// NOTE: No shared tokio runtime is defined here. Each FFI binding manages its own:
// - haiinpm: uses napi-rs's built-in async runtime (automatic)
// - haiipy: defines its own static RT for sync `_sync` method wrappers
// - haiigo: defines its own static RT for the spawn+channel pattern

// =============================================================================
// Streaming handle management (opaque handle pattern for SSE/WS connections)
// =============================================================================

/// Maximum number of concurrent streaming handles (SSE + WS combined).
const MAX_STREAMING_HANDLES: usize = 100;

/// Handles idle for longer than this are considered stale and eligible for cleanup.
const HANDLE_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60); // 30 minutes

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

/// Tracked SSE connection with last-access timestamp for idle cleanup.
struct TrackedSseConnection {
    conn: SseConnection,
    last_access: std::time::Instant,
}

/// Tracked WS connection with last-access timestamp for idle cleanup.
struct TrackedWsConnection {
    conn: WsConnection,
    last_access: std::time::Instant,
}

static SSE_CONNECTIONS: std::sync::LazyLock<tokio::sync::Mutex<HashMap<u64, TrackedSseConnection>>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(HashMap::new()));

static WS_CONNECTIONS: std::sync::LazyLock<tokio::sync::Mutex<HashMap<u64, TrackedWsConnection>>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(HashMap::new()));

/// Remove stale SSE handles that have been idle past the timeout.
async fn cleanup_stale_sse_handles() {
    let mut connections = SSE_CONNECTIONS.lock().await;
    let now = std::time::Instant::now();
    let stale_handles: Vec<u64> = connections.iter()
        .filter(|(_, tracked)| now.duration_since(tracked.last_access) > HANDLE_IDLE_TIMEOUT)
        .map(|(id, _)| *id)
        .collect();
    for id in stale_handles {
        if let Some(mut tracked) = connections.remove(&id) {
            tracked.conn.close().await;
        }
    }
}

/// Remove stale WS handles that have been idle past the timeout.
async fn cleanup_stale_ws_handles() {
    let mut connections = WS_CONNECTIONS.lock().await;
    let now = std::time::Instant::now();
    let stale_handles: Vec<u64> = connections.iter()
        .filter(|(_, tracked)| now.duration_since(tracked.last_access) > HANDLE_IDLE_TIMEOUT)
        .map(|(id, _)| *id)
        .collect();
    for id in stale_handles {
        if let Some(mut tracked) = connections.remove(&id) {
            tracked.conn.close().await;
        }
    }
}

/// Returns total number of active streaming handles (SSE + WS).
async fn total_streaming_handles() -> usize {
    let sse_count = SSE_CONNECTIONS.lock().await.len();
    let ws_count = WS_CONNECTIONS.lock().await.len();
    sse_count + ws_count
}

// =============================================================================
// Error types
// =============================================================================

/// Categories of binding errors for language-specific mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Client configuration failed (invalid base_url, missing config, etc.)
    ConfigFailed,
    /// Authentication failed (401, 403)
    AuthFailed,
    /// Rate limited (429)
    RateLimited,
    /// Resource not found (404)
    NotFound,
    /// Other API error (non-auth, non-rate-limit HTTP errors)
    ApiError,
    /// Network / connection error
    NetworkFailed,
    /// JSON serialization / deserialization failed
    SerializationFailed,
    /// Invalid argument provided by caller
    InvalidArgument,
    /// JACS provider error (signing, verification, etc.)
    ProviderError,
    /// Generic / uncategorized failure
    Generic,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ErrorKind::ConfigFailed => "ConfigFailed",
            ErrorKind::AuthFailed => "AuthFailed",
            ErrorKind::RateLimited => "RateLimited",
            ErrorKind::NotFound => "NotFound",
            ErrorKind::ApiError => "ApiError",
            ErrorKind::NetworkFailed => "NetworkFailed",
            ErrorKind::SerializationFailed => "SerializationFailed",
            ErrorKind::InvalidArgument => "InvalidArgument",
            ErrorKind::ProviderError => "ProviderError",
            ErrorKind::Generic => "Generic",
        };
        write!(f, "{}", s)
    }
}

/// Error type for binding-core operations.
///
/// Language bindings convert this to their native error types
/// (PyErr, napi::Error, Go error).
#[derive(Debug)]
pub struct HaiBindingError {
    pub kind: ErrorKind,
    pub message: String,
}

impl HaiBindingError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for HaiBindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for HaiBindingError {}

/// Convert `HaiError` to `HaiBindingError` with status-code-aware mapping.
impl From<HaiError> for HaiBindingError {
    fn from(err: HaiError) -> Self {
        match &err {
            HaiError::Api { status, .. } => {
                let kind = match *status {
                    401 | 403 => ErrorKind::AuthFailed,
                    404 => ErrorKind::NotFound,
                    429 => ErrorKind::RateLimited,
                    _ => ErrorKind::ApiError,
                };
                HaiBindingError::new(kind, err.to_string())
            }
            HaiError::Http(_) => {
                HaiBindingError::new(ErrorKind::NetworkFailed, err.to_string())
            }
            HaiError::Json(_) => {
                HaiBindingError::new(ErrorKind::SerializationFailed, err.to_string())
            }
            HaiError::ConfigNotFound { .. } | HaiError::ConfigInvalid { .. } => {
                HaiBindingError::new(ErrorKind::ConfigFailed, err.to_string())
            }
            HaiError::MissingJacsId => {
                HaiBindingError::new(ErrorKind::ConfigFailed, err.to_string())
            }
            HaiError::Provider(_) => {
                HaiBindingError::new(ErrorKind::ProviderError, err.to_string())
            }
            HaiError::Validation { .. } => {
                HaiBindingError::new(ErrorKind::InvalidArgument, err.to_string())
            }
            HaiError::VerifyUrlTooLong { .. }
            | HaiError::MissingHostedDocumentId
            | HaiError::Message(_) => {
                HaiBindingError::new(ErrorKind::Generic, err.to_string())
            }
        }
    }
}

impl From<serde_json::Error> for HaiBindingError {
    fn from(err: serde_json::Error) -> Self {
        HaiBindingError::new(ErrorKind::SerializationFailed, err.to_string())
    }
}

/// Result type for binding-core operations.
pub type HaiBindingResult<T> = Result<T, HaiBindingError>;

// =============================================================================
// Client wrapper
// =============================================================================

/// Thread-safe wrapper around `HaiClient` for FFI consumption.
///
/// Uses `Arc<RwLock<...>>` because `HaiClient` has `&mut self` methods
/// (`set_hai_agent_id`, `set_agent_email`) that require interior mutability.
/// Standard read-only methods acquire a read lock; mutating methods acquire
/// a write lock.
pub struct HaiClientWrapper {
    inner: Arc<RwLock<HaiClient<Box<dyn JacsMediaProvider>>>>,
    /// The resolved client identifier string (e.g. "haiai-python/0.3.0").
    /// Stored here for test verification since reqwest::Client doesn't
    /// expose default headers after construction.
    client_identifier: String,
}

impl fmt::Debug for HaiClientWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HaiClientWrapper").finish_non_exhaustive()
    }
}

impl HaiClientWrapper {
    /// Create a new `HaiClientWrapper` from a boxed `JacsProvider` and options.
    pub fn new(
        jacs: Box<dyn JacsMediaProvider>,
        options: HaiClientOptions,
    ) -> HaiBindingResult<Self> {
        let resolved_id = options.client_identifier.clone().unwrap_or_else(|| {
            format!("haiai-rust/{}", env!("CARGO_PKG_VERSION"))
        });
        let client = HaiClient::new(jacs, options)
            .map_err(HaiBindingError::from)?;
        Ok(Self {
            inner: Arc::new(RwLock::new(client)),
            client_identifier: resolved_id,
        })
    }

    /// Create a new `HaiClientWrapper` from a JSON config string.
    ///
    /// Expected JSON format:
    /// ```json
    /// {
    ///   "base_url": "https://beta.hai.ai",
    ///   "jacs_id": "...",
    ///   "timeout_secs": 30,
    ///   "max_retries": 3
    /// }
    /// ```
    ///
    /// Requires a `JacsProvider` to be supplied separately since providers
    /// cannot be deserialized from JSON (they hold cryptographic state).
    pub fn from_config_json(
        config_json: &str,
        jacs: Box<dyn JacsMediaProvider>,
    ) -> HaiBindingResult<Self> {
        let config: Value = serde_json::from_str(config_json)
            .map_err(|e| HaiBindingError::new(ErrorKind::ConfigFailed, e.to_string()))?;

        let base_url = config
            .get("base_url")
            .and_then(|v| v.as_str())
            .unwrap_or("https://beta.hai.ai")
            .to_string();

        let timeout_secs = config
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(30);

        let max_retries = config
            .get("max_retries")
            .and_then(|v| v.as_u64())
            .unwrap_or(3) as usize;

        let client_identifier = config
            .get("client_type")
            .and_then(|v| v.as_str())
            .map(|ct| format!("haiai-{}/{}", ct, env!("CARGO_PKG_VERSION")));

        let options = HaiClientOptions {
            base_url,
            timeout: std::time::Duration::from_secs(timeout_secs),
            max_retries,
            client_identifier,
        };

        Self::new(jacs, options)
    }

    /// Create a new `HaiClientWrapper` from a JSON config string, automatically
    /// selecting the appropriate JACS provider.
    ///
    /// If `jacs_config_path` is present in the config JSON, a real
    /// `LocalJacsProvider` is loaded from that path, enabling actual JACS
    /// cryptographic signing for authenticated API calls.
    ///
    /// If `jacs_config_path` is absent, falls back to `StaticJacsProvider`
    /// (test-only: produces deterministic fake signatures).
    ///
    /// Expected JSON format:
    /// ```json
    /// {
    ///   "base_url": "https://beta.hai.ai",
    ///   "jacs_id": "...",
    ///   "jacs_config_path": "/path/to/jacs.config.json",
    ///   "timeout_secs": 30,
    ///   "max_retries": 3
    /// }
    /// ```
    pub fn from_config_json_auto(config_json: &str) -> HaiBindingResult<Self> {
        let config: Value = serde_json::from_str(config_json)
            .map_err(|e| HaiBindingError::new(ErrorKind::ConfigFailed, e.to_string()))?;

        let jacs_config_path = config
            .get("jacs_config_path")
            .and_then(|v| v.as_str());

        let jacs_id = config
            .get("jacs_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let provider: Box<dyn JacsMediaProvider> = if let Some(path) = jacs_config_path {
            let local = LocalJacsProvider::from_config_path(
                Some(Path::new(path)),
                None,
            ).map_err(|e| HaiBindingError::new(
                ErrorKind::ConfigFailed,
                format!("failed to load JACS config from {path}: {e}"),
            ))?;
            Box::new(local)
        } else {
            // Fallback to StaticJacsProvider (test-only, produces fake signatures).
            // This provider generates deterministic signatures that will be rejected
            // by the HAI API on every authenticated endpoint.
            eprintln!(
                "WARNING: hai-binding-core: No jacs_config_path provided. \
                 Using test-only StaticJacsProvider. Authenticated API calls will fail."
            );
            Box::new(StaticJacsProvider::new(jacs_id))
        };

        Self::from_config_json(config_json, provider)
    }

    // =========================================================================
    // Smoke method — proves the wrapper works end-to-end
    // =========================================================================

    /// Call the hello endpoint and return JSON.
    pub async fn hello(&self, include_test: bool) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.hello(include_test).await?;
        Ok(serde_json::to_string(&result)?)
    }

    // =========================================================================
    // Client state accessors
    // =========================================================================

    /// Get the resolved client identifier (e.g. "haiai-python/0.3.0").
    pub fn client_identifier(&self) -> &str {
        &self.client_identifier
    }

    /// Get the JACS ID.
    pub async fn jacs_id(&self) -> String {
        let client = self.inner.read().await;
        client.jacs_id().to_string()
    }

    /// Get the base URL.
    pub async fn base_url(&self) -> String {
        let client = self.inner.read().await;
        client.base_url().to_string()
    }

    /// Get the HAI agent ID.
    pub async fn hai_agent_id(&self) -> String {
        let client = self.inner.read().await;
        client.hai_agent_id().to_string()
    }

    /// Get the agent email.
    pub async fn agent_email(&self) -> Option<String> {
        let client = self.inner.read().await;
        client.agent_email().map(|s| s.to_string())
    }

    /// Set the HAI agent ID (requires write lock).
    pub async fn set_hai_agent_id(&self, id: String) {
        let mut client = self.inner.write().await;
        client.set_hai_agent_id(id);
    }

    /// Set the agent email (requires write lock).
    pub async fn set_agent_email(&self, email: String) {
        let mut client = self.inner.write().await;
        client.set_agent_email(email);
    }

    // =========================================================================
    // Sync JACS delegation methods
    // =========================================================================

    /// Build an auth header.
    pub async fn build_auth_header(&self) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        Ok(client.build_auth_header()?)
    }

    /// Sign a message string.
    pub async fn sign_message(&self, message: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        Ok(client.sign_message(message)?)
    }

    /// Get canonical JSON for a value.
    pub async fn canonical_json(&self, value_json: &str) -> HaiBindingResult<String> {
        let value: Value = serde_json::from_str(value_json)?;
        let client = self.inner.read().await;
        Ok(client.canonical_json(&value)?)
    }

    /// Verify an A2A artifact.
    pub async fn verify_a2a_artifact(&self, wrapped_json: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        Ok(client.verify_a2a_artifact(wrapped_json)?)
    }

    /// Export the agent JSON.
    pub async fn export_agent_json(&self) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        Ok(client.export_agent_json()?)
    }

    // =========================================================================
    // Registration & Identity
    // =========================================================================

    /// Register an agent.
    pub async fn register(&self, options_json: &str) -> HaiBindingResult<String> {
        let options: haiai::types::RegisterAgentOptions = serde_json::from_str(options_json)?;
        let client = self.inner.read().await;
        let result = client.register(&options).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Register a brand-new agent: generate keys via JACS, create agent document,
    /// then register with the HAI server.
    ///
    /// This is a combined operation that:
    /// 1. Calls `LocalJacsProvider::create_agent_with_options()` to generate keypair + agent doc
    /// 2. Loads the newly created agent as a `LocalJacsProvider`
    /// 3. Creates a temporary `HaiClient` with that provider
    /// 4. Calls `register()` on the server
    /// 5. Returns combined result with agent_id, jacs_id, paths, etc.
    pub async fn register_new_agent(&self, options_json: &str) -> HaiBindingResult<String> {
        let v: Value = serde_json::from_str(options_json)?;

        let agent_name = v.get("agent_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HaiBindingError::new(ErrorKind::InvalidArgument, "missing agent_name"))?;
        let password = v.get("password")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HaiBindingError::new(ErrorKind::InvalidArgument, "missing password"))?;

        // Build CreateAgentOptions
        let create_opts = haiai::types::CreateAgentOptions {
            name: agent_name.to_string(),
            password: password.to_string(),
            algorithm: v.get("algorithm").and_then(|v| v.as_str()).map(String::from),
            data_directory: v.get("data_directory").and_then(|v| v.as_str()).map(String::from),
            key_directory: v.get("key_directory").and_then(|v| v.as_str()).map(String::from),
            config_path: v.get("config_path").and_then(|v| v.as_str()).map(String::from),
            agent_type: None,
            description: v.get("description").and_then(|v| v.as_str()).map(String::from),
            domain: v.get("domain").and_then(|v| v.as_str()).map(String::from),
            default_storage: None,
        };

        // Step 1: Create the agent (keygen + doc creation)
        let create_result = LocalJacsProvider::create_agent_with_options(&create_opts)
            .map_err(|e| HaiBindingError::new(ErrorKind::Generic, format!("agent creation failed: {e}")))?;

        // Step 2: Load the newly created agent
        let config_path = Path::new(&create_result.config_path);
        let provider = LocalJacsProvider::from_config_path(Some(config_path), None)
            .map_err(|e| HaiBindingError::new(ErrorKind::Generic, format!("failed to load new agent: {e}")))?;

        // Step 3: Read the public key for registration.
        // Read as raw bytes and normalize to PEM — JACS may write DER (binary)
        // or PEM depending on the algorithm.
        let pub_key_bytes = std::fs::read(&create_result.public_key_path)
            .map_err(|e| HaiBindingError::new(ErrorKind::Generic, format!("failed to read public key: {e}")))?;
        let pub_key_pem = haiai::key_format::normalize_public_key_pem(&pub_key_bytes);

        // Step 4: Get the agent JSON from the provider
        let agent_json = provider.export_agent_json()
            .map_err(|e| HaiBindingError::new(ErrorKind::Generic, format!("failed to export agent JSON: {e}")))?;

        // Step 5: Create a temporary HaiClient with the new provider
        let base_url = v.get("base_url").and_then(|v| v.as_str());
        let mut client_opts = HaiClientOptions::default();
        if let Some(url) = base_url {
            client_opts.base_url = url.to_string();
        }
        let temp_client = HaiClient::new(provider, client_opts)
            .map_err(|e| HaiBindingError::new(ErrorKind::Generic, format!("failed to create temp client: {e}")))?;

        // Step 6: Register with HAI
        let register_opts = haiai::types::RegisterAgentOptions {
            agent_json,
            public_key_pem: Some(pub_key_pem),
            owner_email: v.get("owner_email").and_then(|v| v.as_str()).map(String::from),
            domain: v.get("domain").and_then(|v| v.as_str()).map(String::from),
            description: v.get("description").and_then(|v| v.as_str()).map(String::from),
            registration_key: v.get("registration_key").and_then(|v| v.as_str()).map(String::from),
            is_mediator: None,
        };

        let reg_result = temp_client.register(&register_opts).await
            .map_err(|e| HaiBindingError::new(ErrorKind::Generic, format!("registration failed: {e}")))?;

        // Step 7: Build combined result
        let mut result = serde_json::to_value(&create_result)?;
        if let Some(obj) = result.as_object_mut() {
            if let Ok(reg_val) = serde_json::to_value(&reg_result) {
                if let Some(reg_obj) = reg_val.as_object() {
                    for (k, v) in reg_obj {
                        obj.insert(k.clone(), v.clone());
                    }
                }
            }
        }

        Ok(serde_json::to_string(&result)?)
    }

    /// Rotate the agent's cryptographic keys.
    pub async fn rotate_keys(&self, options_json: &str) -> HaiBindingResult<String> {
        // RotateKeysOptions lacks Deserialize -- manually construct
        let v: Value = serde_json::from_str(options_json)?;
        let options = haiai::types::RotateKeysOptions {
            register_with_hai: v.get("register_with_hai").and_then(|b| b.as_bool()),
        };
        let client = self.inner.read().await;
        let result = client.rotate_keys(Some(&options)).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Update agent metadata.
    pub async fn update_agent(&self, new_agent_data: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.update_agent(new_agent_data).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Submit a job response.
    ///
    /// Accepts JSON: `{"job_id": "...", "message": "...", "metadata": {...}, "processing_time_ms": 123}`
    pub async fn submit_response(&self, params_json: &str) -> HaiBindingResult<String> {
        let v: Value = serde_json::from_str(params_json)?;
        let job_id = v.get("job_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HaiBindingError::new(ErrorKind::InvalidArgument, "missing job_id"))?;
        let message = v.get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HaiBindingError::new(ErrorKind::InvalidArgument, "missing message"))?;
        let metadata = v.get("metadata").cloned();
        let processing_time_ms = v.get("processing_time_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let client = self.inner.read().await;
        let result = client.submit_response(job_id, message, metadata, processing_time_ms).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Check agent verification status.
    pub async fn verify_status(&self, agent_id: Option<&str>) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.verify_status(agent_id).await?;
        Ok(serde_json::to_string(&result)?)
    }

    // =========================================================================
    // Username
    // =========================================================================

    /// Update an agent's username.
    pub async fn update_username(&self, agent_id: &str, username: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.update_username(agent_id, username).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Delete an agent's username.
    pub async fn delete_username(&self, agent_id: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.delete_username(agent_id).await?;
        Ok(serde_json::to_string(&result)?)
    }

    // =========================================================================
    // Email Core
    // =========================================================================

    /// Send an email.
    pub async fn send_email(&self, options_json: &str) -> HaiBindingResult<String> {
        let options: haiai::types::SendEmailOptions = serde_json::from_str(options_json)?;
        let client = self.inner.read().await;
        let result = client.send_email(&options).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Send a signed email (locally signed with agent JACS key).
    pub async fn send_signed_email(&self, options_json: &str) -> HaiBindingResult<String> {
        let options: haiai::types::SendEmailOptions = serde_json::from_str(options_json)?;
        let client = self.inner.read().await;
        let result = client.send_signed_email(&options).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// List messages.
    pub async fn list_messages(&self, options_json: &str) -> HaiBindingResult<String> {
        let options: haiai::types::ListMessagesOptions = serde_json::from_str(options_json)?;
        let client = self.inner.read().await;
        let result = client.list_messages(&options).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Update labels on a message.
    ///
    /// Accepts JSON: `{"message_id": "...", "add": ["label1"], "remove": ["label2"]}`
    pub async fn update_labels(&self, params_json: &str) -> HaiBindingResult<String> {
        let v: Value = serde_json::from_str(params_json)?;
        let message_id = v.get("message_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HaiBindingError::new(ErrorKind::InvalidArgument, "missing message_id"))?;

        let add_vec: Vec<String> = v.get("add")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|s| s.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let remove_vec: Vec<String> = v.get("remove")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|s| s.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let add_refs: Vec<&str> = add_vec.iter().map(|s| s.as_str()).collect();
        let remove_refs: Vec<&str> = remove_vec.iter().map(|s| s.as_str()).collect();

        let client = self.inner.read().await;
        let result = client.update_labels(message_id, &add_refs, &remove_refs).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Get email status.
    pub async fn get_email_status(&self) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.get_email_status().await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Get a single message by ID.
    pub async fn get_message(&self, message_id: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.get_message(message_id).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Fetch the raw RFC 5322 MIME bytes for a message.
    ///
    /// Returns a JSON string with the PRD wire shape:
    /// `{message_id, rfc_message_id, available, raw_email_b64, size_bytes, omitted_reason}`.
    /// Language SDKs decode `raw_email_b64` into native byte types.
    pub async fn get_raw_email(&self, message_id: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.get_raw_email(message_id).await?;
        Ok(serde_json::to_string(&result.to_wire_json())?)
    }

    /// Get unread message count.
    pub async fn get_unread_count(&self) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.get_unread_count().await?;
        Ok(serde_json::to_string(&result)?)
    }

    // =========================================================================
    // Email Actions
    // =========================================================================

    /// Mark a message as read.
    pub async fn mark_read(&self, message_id: &str) -> HaiBindingResult<()> {
        let client = self.inner.read().await;
        client.mark_read(message_id).await?;
        Ok(())
    }

    /// Mark a message as unread.
    pub async fn mark_unread(&self, message_id: &str) -> HaiBindingResult<()> {
        let client = self.inner.read().await;
        client.mark_unread(message_id).await?;
        Ok(())
    }

    /// Delete a message.
    pub async fn delete_message(&self, message_id: &str) -> HaiBindingResult<()> {
        let client = self.inner.read().await;
        client.delete_message(message_id).await?;
        Ok(())
    }

    /// Archive a message.
    pub async fn archive(&self, message_id: &str) -> HaiBindingResult<()> {
        let client = self.inner.read().await;
        client.archive(message_id).await?;
        Ok(())
    }

    /// Unarchive a message.
    pub async fn unarchive(&self, message_id: &str) -> HaiBindingResult<()> {
        let client = self.inner.read().await;
        client.unarchive(message_id).await?;
        Ok(())
    }

    /// Reply to a message with options.
    ///
    /// Accepts JSON: `{"message_id": "...", "body": "...", "subject_override": "...", "reply_type": "sender", "recipients": [...]}`
    pub async fn reply_with_options(&self, params_json: &str) -> HaiBindingResult<String> {
        let v: Value = serde_json::from_str(params_json)?;
        let message_id = v.get("message_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HaiBindingError::new(ErrorKind::InvalidArgument, "missing message_id"))?;
        let body = v.get("body")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HaiBindingError::new(ErrorKind::InvalidArgument, "missing body"))?;
        let subject_override = v.get("subject_override").and_then(|v| v.as_str());
        let reply_type = v.get("reply_type").and_then(|v| v.as_str());
        let recipients: Vec<String> = v.get("recipients")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|s| s.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let client = self.inner.read().await;
        let result = client.reply_with_options(message_id, body, subject_override, reply_type, &recipients).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Forward a message.
    ///
    /// Accepts JSON: `{"message_id": "...", "to": "...", "comment": "..."}`
    pub async fn forward(&self, params_json: &str) -> HaiBindingResult<String> {
        let v: Value = serde_json::from_str(params_json)?;
        let message_id = v.get("message_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HaiBindingError::new(ErrorKind::InvalidArgument, "missing message_id"))?;
        let to = v.get("to")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HaiBindingError::new(ErrorKind::InvalidArgument, "missing to"))?;
        let comment = v.get("comment").and_then(|v| v.as_str());

        let client = self.inner.read().await;
        let result = client.forward(message_id, to, comment).await?;
        Ok(serde_json::to_string(&result)?)
    }

    // =========================================================================
    // Search & Contacts
    // =========================================================================

    /// Search messages.
    pub async fn search_messages(&self, options_json: &str) -> HaiBindingResult<String> {
        let options: haiai::types::SearchOptions = serde_json::from_str(options_json)?;
        let client = self.inner.read().await;
        let result = client.search_messages(&options).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Get contacts.
    pub async fn contacts(&self) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.contacts().await?;
        Ok(serde_json::to_string(&result)?)
    }

    // =========================================================================
    // Server Keys
    // =========================================================================

    /// Fetch the HAI server's public keys (unauthenticated).
    pub async fn fetch_server_keys(&self) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.fetch_server_keys().await?;
        Ok(serde_json::to_string(&result)?)
    }

    // =========================================================================
    // Raw Email Sign/Verify
    // =========================================================================

    /// Sign a raw RFC 5822 email (base64-encoded) via the HAI server.
    pub async fn sign_email_raw(&self, raw_email_b64: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        Ok(client.sign_email_raw(raw_email_b64).await?)
    }

    /// Verify a raw RFC 5822 email (base64-encoded) via the HAI server.
    pub async fn verify_email_raw(&self, raw_email_b64: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.verify_email_raw(raw_email_b64).await?;
        Ok(serde_json::to_string(&result)?)
    }

    // =========================================================================
    // Local Media Sign/Verify (Layer 8 / TASK_003)
    // =========================================================================
    //
    // Five local-only methods backed by JacsMediaProvider. Opts JSON contract
    // is FLAT for both sign_image and verify_image — `"robust": true` (NOT
    // the JACS-internal `scan_robust`). `parse_verify_image_opts` maps the
    // user's flat key into the nested `VerifyImageOptions.scan_robust` field.

    /// Sign a markdown / text file in place. Opts JSON: `{"backup": bool, "allow_duplicate": bool}`.
    pub async fn sign_text(&self, path: &str, opts_json: &str) -> HaiBindingResult<String> {
        let opts = parse_sign_text_opts(opts_json)?;
        let client = self.inner.read().await;
        let outcome = client.sign_text_file(path, opts).map_err(HaiBindingError::from)?;
        serde_json::to_string(&outcome).map_err(HaiBindingError::from)
    }

    /// Verify all signature blocks in a text file. Opts JSON:
    /// `{"strict": bool, "key_dir": string?}`. Returns a flat envelope:
    /// `{"status": "signed"|"missing_signature"|"malformed", "signatures": [...], "malformed_detail": ?}`.
    pub async fn verify_text(&self, path: &str, opts_json: &str) -> HaiBindingResult<String> {
        let opts = parse_verify_text_opts(opts_json)?;
        let client = self.inner.read().await;
        let result = client.verify_text_file(path, opts).map_err(HaiBindingError::from)?;
        let envelope = verify_text_result_to_json(&result);
        serde_json::to_string(&envelope).map_err(HaiBindingError::from)
    }

    /// Sign an image file (PNG/JPEG/WebP). Opts JSON:
    /// `{"robust": bool, "format_hint": string?, "refuse_overwrite": bool, "backup": bool, "unsafe_bak_mode": uint32?}`.
    pub async fn sign_image(
        &self,
        in_path: &str,
        out_path: &str,
        opts_json: &str,
    ) -> HaiBindingResult<String> {
        let opts = parse_sign_image_opts(opts_json)?;
        let client = self.inner.read().await;
        let signed = client
            .sign_image(in_path, out_path, opts)
            .map_err(HaiBindingError::from)?;
        serde_json::to_string(&signed).map_err(HaiBindingError::from)
    }

    /// Verify the JACS signature embedded in an image. Opts JSON:
    /// `{"strict": bool, "key_dir": string?, "robust": bool}`.
    /// Public key `"robust"` maps to JACS-internal `scan_robust`.
    ///
    /// Returns a flat JSON envelope produced by [`media_verify_result_to_json`] —
    /// `status` is always a plain snake_case string (never the JACS-internal
    /// `{"malformed": detail}` shape) and `malformed_detail` carries the detail
    /// when applicable. This is the single conversion site; language SDKs
    /// always read this shape.
    pub async fn verify_image(&self, path: &str, opts_json: &str) -> HaiBindingResult<String> {
        let opts = parse_verify_image_opts(opts_json)?;
        let client = self.inner.read().await;
        let result = client.verify_image(path, opts).map_err(HaiBindingError::from)?;
        let envelope = media_verify_result_to_json(&result);
        serde_json::to_string(&envelope).map_err(HaiBindingError::from)
    }

    /// Extract the JACS signature payload from a signed image without verifying.
    /// Opts JSON: `{"raw_payload": bool}`. Returns
    /// `{"present": bool, "payload": string?}`. The MCP layer wraps this in a
    /// `success` envelope (success = present); binding-core itself emits the
    /// flat shape so language SDKs and the CLI can decide their own error
    /// signaling (CLI exits 2 on `present: false`, MCP returns
    /// `success: false`, language SDKs surface `present` directly).
    pub async fn extract_media_signature(
        &self,
        path: &str,
        opts_json: &str,
    ) -> HaiBindingResult<String> {
        let raw_payload = parse_extract_opts(opts_json)?;
        let client = self.inner.read().await;
        let payload = client
            .extract_media_signature(path, raw_payload)
            .map_err(HaiBindingError::from)?;
        let envelope = serde_json::json!({
            "present": payload.is_some(),
            "payload": payload,
        });
        serde_json::to_string(&envelope).map_err(HaiBindingError::from)
    }

    // =========================================================================
    // Attestations
    // =========================================================================

    /// Create an attestation for an agent.
    ///
    /// Accepts JSON: `{"agent_id": "...", "subject": {...}, "claims": [...], "evidence": [...]}`
    pub async fn create_attestation(&self, params_json: &str) -> HaiBindingResult<String> {
        let v: Value = serde_json::from_str(params_json)?;
        let agent_id = v.get("agent_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HaiBindingError::new(ErrorKind::InvalidArgument, "missing agent_id"))?;
        let subject = v.get("subject")
            .ok_or_else(|| HaiBindingError::new(ErrorKind::InvalidArgument, "missing subject"))?;
        let claims = v.get("claims")
            .ok_or_else(|| HaiBindingError::new(ErrorKind::InvalidArgument, "missing claims"))?;
        let evidence = v.get("evidence");

        let client = self.inner.read().await;
        let result = client.create_attestation(agent_id, subject, claims, evidence).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// List attestations for an agent.
    ///
    /// Accepts JSON: `{"agent_id": "...", "limit": 20, "offset": 0}`
    pub async fn list_attestations(&self, params_json: &str) -> HaiBindingResult<String> {
        let v: Value = serde_json::from_str(params_json)?;
        let agent_id = v.get("agent_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HaiBindingError::new(ErrorKind::InvalidArgument, "missing agent_id"))?;
        let limit = v.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as u32;
        let offset = v.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

        let client = self.inner.read().await;
        let result = client.list_attestations(agent_id, limit, offset).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Get a single attestation by document ID.
    pub async fn get_attestation(&self, agent_id: &str, doc_id: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.get_attestation(agent_id, doc_id).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Verify an attestation document.
    pub async fn verify_attestation(&self, document: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.verify_attestation(document).await?;
        Ok(serde_json::to_string(&result)?)
    }

    // =========================================================================
    // Email Templates
    // =========================================================================

    /// Create an email template.
    pub async fn create_email_template(&self, options_json: &str) -> HaiBindingResult<String> {
        let options: haiai::types::CreateEmailTemplateOptions = serde_json::from_str(options_json)?;
        let client = self.inner.read().await;
        let result = client.create_email_template(&options).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// List/search email templates.
    pub async fn list_email_templates(&self, options_json: &str) -> HaiBindingResult<String> {
        let options: haiai::types::ListEmailTemplatesOptions = serde_json::from_str(options_json)?;
        let client = self.inner.read().await;
        let result = client.list_email_templates(&options).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Get a single email template by ID.
    pub async fn get_email_template(&self, template_id: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.get_email_template(template_id).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Update an email template.
    pub async fn update_email_template(&self, template_id: &str, options_json: &str) -> HaiBindingResult<String> {
        let options: haiai::types::UpdateEmailTemplateOptions = serde_json::from_str(options_json)?;
        let client = self.inner.read().await;
        let result = client.update_email_template(template_id, &options).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Delete an email template.
    pub async fn delete_email_template(&self, template_id: &str) -> HaiBindingResult<()> {
        let client = self.inner.read().await;
        client.delete_email_template(template_id).await?;
        Ok(())
    }

    // =========================================================================
    // Key Operations
    // =========================================================================

    /// Fetch a remote agent's public key.
    pub async fn fetch_remote_key(&self, jacs_id: &str, version: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.fetch_remote_key(jacs_id, version).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Fetch a key by its SHA-256 hash.
    pub async fn fetch_key_by_hash(&self, hash: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.fetch_key_by_hash(hash).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Fetch a key by email address.
    pub async fn fetch_key_by_email(&self, email: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.fetch_key_by_email(email).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Fetch a key by domain.
    pub async fn fetch_key_by_domain(&self, domain: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.fetch_key_by_domain(domain).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Fetch all key versions for an agent.
    pub async fn fetch_all_keys(&self, jacs_id: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.fetch_all_keys(jacs_id).await?;
        Ok(serde_json::to_string(&result)?)
    }

    // =========================================================================
    // Verification
    // =========================================================================

    /// Verify a document.
    pub async fn verify_document(&self, document: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.verify_document(document).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Get verification status for an agent.
    pub async fn get_verification(&self, agent_id: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.get_verification(agent_id).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Verify an agent document.
    pub async fn verify_agent_document(&self, request_json: &str) -> HaiBindingResult<String> {
        let request: haiai::types::VerifyAgentDocumentRequest = serde_json::from_str(request_json)?;
        let client = self.inner.read().await;
        let result = client.verify_agent_document(&request).await?;
        Ok(serde_json::to_string(&result)?)
    }

    // =========================================================================
    // Benchmarks
    // =========================================================================

    /// Run a benchmark.
    pub async fn benchmark(&self, name: Option<&str>, tier: Option<&str>) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.benchmark(name, tier).await?;
        // benchmark returns Value -- pass through as-is without round-trip
        Ok(serde_json::to_string(&result)?)
    }

    /// Run a free benchmark.
    pub async fn free_run(&self, transport: Option<&str>) -> HaiBindingResult<String> {
        let transport = transport.map(|t| match t {
            "ws" => haiai::types::TransportType::Ws,
            _ => haiai::types::TransportType::Sse,
        });
        let client = self.inner.read().await;
        let result = client.free_run(transport).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Run a pro benchmark.
    ///
    /// Accepts JSON: `{"transport": "sse", "poll_interval_ms": 2000, "poll_timeout_secs": 300}`
    pub async fn pro_run(&self, options_json: &str) -> HaiBindingResult<String> {
        // ProRunOptions lacks Deserialize -- manually construct
        let v: Value = serde_json::from_str(options_json)?;
        let transport = match v.get("transport").and_then(|t| t.as_str()) {
            Some("ws") => haiai::types::TransportType::Ws,
            _ => haiai::types::TransportType::Sse,
        };
        let poll_interval = std::time::Duration::from_millis(
            v.get("poll_interval_ms").and_then(|v| v.as_u64()).unwrap_or(2000)
        );
        let poll_timeout = std::time::Duration::from_secs(
            v.get("poll_timeout_secs").and_then(|v| v.as_u64()).unwrap_or(300)
        );

        let options = haiai::types::ProRunOptions {
            transport,
            poll_interval,
            poll_timeout,
        };

        let client = self.inner.read().await;
        let result = client.pro_run(&options).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Run an enterprise benchmark (coming soon).
    pub async fn enterprise_run(&self) -> HaiBindingResult<()> {
        let client = self.inner.read().await;
        client.enterprise_run().await?;
        Ok(())
    }

    // =========================================================================
    // SSE Streaming (opaque handle pattern)
    // =========================================================================

    /// Connect to SSE and return an opaque handle ID.
    ///
    /// Enforces a maximum handle limit and cleans up stale idle connections
    /// before allocating a new handle.
    pub async fn connect_sse(&self) -> HaiBindingResult<u64> {
        // Lazy cleanup of stale handles before allocating
        cleanup_stale_sse_handles().await;
        cleanup_stale_ws_handles().await;

        if total_streaming_handles().await >= MAX_STREAMING_HANDLES {
            return Err(HaiBindingError::new(
                ErrorKind::ApiError,
                format!("maximum streaming handle limit ({MAX_STREAMING_HANDLES}) exceeded; close existing connections first"),
            ));
        }

        let client = self.inner.read().await;
        let conn = client.connect_sse().await?;
        let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        SSE_CONNECTIONS.lock().await.insert(handle, TrackedSseConnection {
            conn,
            last_access: std::time::Instant::now(),
        });
        Ok(handle)
    }

    // =========================================================================
    // WebSocket Streaming (opaque handle pattern)
    // =========================================================================

    /// Connect to WebSocket and return an opaque handle ID.
    ///
    /// Enforces a maximum handle limit and cleans up stale idle connections
    /// before allocating a new handle.
    pub async fn connect_ws(&self) -> HaiBindingResult<u64> {
        // Lazy cleanup of stale handles before allocating
        cleanup_stale_sse_handles().await;
        cleanup_stale_ws_handles().await;

        if total_streaming_handles().await >= MAX_STREAMING_HANDLES {
            return Err(HaiBindingError::new(
                ErrorKind::ApiError,
                format!("maximum streaming handle limit ({MAX_STREAMING_HANDLES}) exceeded; close existing connections first"),
            ));
        }

        let client = self.inner.read().await;
        let conn = client.connect_ws().await?;
        let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        WS_CONNECTIONS.lock().await.insert(handle, TrackedWsConnection {
            conn,
            last_access: std::time::Instant::now(),
        });
        Ok(handle)
    }
}

// =============================================================================
// Media-signing option parsers (Layer 8 / TASK_003)
// =============================================================================
//
// JACS's `SignTextOptions` / `SignImageOptions` / `VerifyImageOptions` /
// `inline::VerifyOptions` are NOT Serde-able — they only derive Debug+Clone(+Default).
// These helpers construct them field-by-field from a `serde_json::Value`,
// applying defaults on `""` / `"null"` / `"{}"` per the binding-core convention.

fn parse_opts_root(s: &str) -> HaiBindingResult<Value> {
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed == "null" {
        return Ok(Value::Object(Default::default()));
    }
    serde_json::from_str(trimmed).map_err(|e| {
        HaiBindingError::new(
            ErrorKind::InvalidArgument,
            format!("invalid options JSON: {e}"),
        )
    })
}

fn coerce_bool(v: &Value, key: &str) -> HaiBindingResult<Option<bool>> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(other) => Err(HaiBindingError::new(
            ErrorKind::InvalidArgument,
            format!("option '{key}' must be a boolean, got: {other}"),
        )),
    }
}

fn coerce_string(v: &Value, key: &str) -> HaiBindingResult<Option<String>> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(other) => Err(HaiBindingError::new(
            ErrorKind::InvalidArgument,
            format!("option '{key}' must be a string, got: {other}"),
        )),
    }
}

fn coerce_u32(v: &Value, key: &str) -> HaiBindingResult<Option<u32>> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => {
            let val = n.as_u64().ok_or_else(|| {
                HaiBindingError::new(
                    ErrorKind::InvalidArgument,
                    format!("option '{key}' must be a non-negative integer"),
                )
            })?;
            if val > u32::MAX as u64 {
                return Err(HaiBindingError::new(
                    ErrorKind::InvalidArgument,
                    format!("option '{key}' out of u32 range"),
                ));
            }
            Ok(Some(val as u32))
        }
        Some(other) => Err(HaiBindingError::new(
            ErrorKind::InvalidArgument,
            format!("option '{key}' must be an integer, got: {other}"),
        )),
    }
}

pub(crate) fn parse_sign_text_opts(s: &str) -> HaiBindingResult<SignTextOptions> {
    let v = parse_opts_root(s)?;
    Ok(SignTextOptions {
        backup: coerce_bool(&v, "backup")?.unwrap_or(true),
        allow_duplicate: coerce_bool(&v, "allow_duplicate")?.unwrap_or(false),
        unsafe_bak_mode: coerce_u32(&v, "unsafe_bak_mode")?,
    })
}

pub(crate) fn parse_verify_text_opts(s: &str) -> HaiBindingResult<VerifyTextOptions> {
    let v = parse_opts_root(s)?;
    Ok(VerifyTextOptions {
        strict: coerce_bool(&v, "strict")?.unwrap_or(false),
        key_dir: coerce_string(&v, "key_dir")?.map(PathBuf::from),
    })
}

pub(crate) fn parse_sign_image_opts(s: &str) -> HaiBindingResult<SignImageOptions> {
    let v = parse_opts_root(s)?;
    Ok(SignImageOptions {
        robust: coerce_bool(&v, "robust")?.unwrap_or(false),
        format_hint: coerce_string(&v, "format_hint")?,
        refuse_overwrite: coerce_bool(&v, "refuse_overwrite")?.unwrap_or(false),
        backup: coerce_bool(&v, "backup")?.unwrap_or(true),
        unsafe_bak_mode: coerce_u32(&v, "unsafe_bak_mode")?,
    })
}

/// Parse the FLAT user-facing JSON contract `{strict, key_dir, robust}` into
/// the JACS-internal nested `VerifyImageOptions { base: VerifyOptions{...},
/// scan_robust }` shape. Only the wire field name `robust` is exposed; the
/// JACS-internal `scan_robust` name does not appear in any public surface
/// (CLI flag, MCP param, language SDK kwarg).
pub(crate) fn parse_verify_image_opts(s: &str) -> HaiBindingResult<VerifyImageOptions> {
    let v = parse_opts_root(s)?;
    let base = VerifyTextOptions {
        strict: coerce_bool(&v, "strict")?.unwrap_or(false),
        key_dir: coerce_string(&v, "key_dir")?.map(PathBuf::from),
    };
    Ok(VerifyImageOptions {
        base,
        scan_robust: coerce_bool(&v, "robust")?.unwrap_or(false),
    })
}

pub(crate) fn parse_extract_opts(s: &str) -> HaiBindingResult<bool> {
    let v = parse_opts_root(s)?;
    Ok(coerce_bool(&v, "raw_payload")?.unwrap_or(false))
}

// =============================================================================
// Issue 013: `verify_text_result_to_json` and `media_verify_result_to_json`
// were hoisted into `haiai::jacs` so the haiai-cli surface can route through
// the same conversion helper that binding-core/MCP use. The CLI previously
// produced wire-incompatible JSON (raw `serde_json::to_string` on the JACS
// struct leaked the tagged `{"malformed": ...}` shape; verify-text Signed
// branch emitted `"status": "valid"|"invalid"` instead of `"signed"`). By
// importing from `haiai::jacs` here, this module keeps a single source of
// truth for the wire envelopes across all surfaces.
// =============================================================================

// =============================================================================
// SSE streaming standalone functions (global handle access)
// =============================================================================

/// Poll next SSE event. Returns JSON string or None if connection closed.
pub async fn sse_next_event(handle_id: u64) -> HaiBindingResult<Option<String>> {
    // Take the connection out of the map to release the lock during await
    let mut tracked = {
        let mut connections = SSE_CONNECTIONS.lock().await;
        connections.remove(&handle_id)
            .ok_or_else(|| HaiBindingError::new(ErrorKind::InvalidArgument, format!("invalid SSE handle: {handle_id}")))?
    };

    let event = tracked.conn.next_event().await;

    match event {
        Some(evt) => {
            // Update last access time and put connection back
            tracked.last_access = std::time::Instant::now();
            SSE_CONNECTIONS.lock().await.insert(handle_id, tracked);
            Ok(Some(serde_json::to_string(&evt)?))
        }
        None => {
            // Connection closed, don't put back
            Ok(None)
        }
    }
}

/// Close an SSE connection and release the handle.
pub async fn sse_close(handle_id: u64) -> HaiBindingResult<()> {
    let mut connections = SSE_CONNECTIONS.lock().await;
    if let Some(mut tracked) = connections.remove(&handle_id) {
        tracked.conn.close().await;
    }
    Ok(())
}

// =============================================================================
// WebSocket streaming standalone functions (global handle access)
// =============================================================================

/// Poll next WebSocket event. Returns JSON string or None if connection closed.
pub async fn ws_next_event(handle_id: u64) -> HaiBindingResult<Option<String>> {
    // Take the connection out of the map to release the lock during await
    let mut tracked = {
        let mut connections = WS_CONNECTIONS.lock().await;
        connections.remove(&handle_id)
            .ok_or_else(|| HaiBindingError::new(ErrorKind::InvalidArgument, format!("invalid WS handle: {handle_id}")))?
    };

    let event = tracked.conn.next_event().await;

    match event {
        Some(evt) => {
            // Update last access time and put connection back
            tracked.last_access = std::time::Instant::now();
            WS_CONNECTIONS.lock().await.insert(handle_id, tracked);
            Ok(Some(serde_json::to_string(&evt)?))
        }
        None => Ok(None),
    }
}

/// Close a WebSocket connection and release the handle.
pub async fn ws_close(handle_id: u64) -> HaiBindingResult<()> {
    let mut connections = WS_CONNECTIONS.lock().await;
    if let Some(mut tracked) = connections.remove(&handle_id) {
        tracked.conn.close().await;
    }
    Ok(())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_kind_variants_create() {
        let kinds = vec![
            ErrorKind::ConfigFailed,
            ErrorKind::AuthFailed,
            ErrorKind::RateLimited,
            ErrorKind::NotFound,
            ErrorKind::ApiError,
            ErrorKind::NetworkFailed,
            ErrorKind::SerializationFailed,
            ErrorKind::InvalidArgument,
            ErrorKind::ProviderError,
            ErrorKind::Generic,
        ];

        for kind in kinds {
            let err = HaiBindingError::new(kind, format!("test {kind}"));
            assert_eq!(err.kind, kind);
            assert!(err.message.contains("test"));
        }
    }

    #[test]
    fn error_display_shows_message() {
        let err = HaiBindingError::new(ErrorKind::AuthFailed, "access denied");
        assert_eq!(format!("{err}"), "access denied");
    }

    #[test]
    fn hai_error_api_401_maps_to_auth_failed() {
        let hai_err = HaiError::Api {
            status: 401,
            message: "unauthorized".to_string(),
        };
        let binding_err: HaiBindingError = hai_err.into();
        assert_eq!(binding_err.kind, ErrorKind::AuthFailed);
    }

    #[test]
    fn hai_error_api_403_maps_to_auth_failed() {
        let hai_err = HaiError::Api {
            status: 403,
            message: "forbidden".to_string(),
        };
        let binding_err: HaiBindingError = hai_err.into();
        assert_eq!(binding_err.kind, ErrorKind::AuthFailed);
    }

    #[test]
    fn hai_error_api_404_maps_to_not_found() {
        let hai_err = HaiError::Api {
            status: 404,
            message: "not found".to_string(),
        };
        let binding_err: HaiBindingError = hai_err.into();
        assert_eq!(binding_err.kind, ErrorKind::NotFound);
    }

    #[test]
    fn hai_error_api_429_maps_to_rate_limited() {
        let hai_err = HaiError::Api {
            status: 429,
            message: "too many requests".to_string(),
        };
        let binding_err: HaiBindingError = hai_err.into();
        assert_eq!(binding_err.kind, ErrorKind::RateLimited);
    }

    #[test]
    fn hai_error_api_500_maps_to_api_error() {
        let hai_err = HaiError::Api {
            status: 500,
            message: "internal server error".to_string(),
        };
        let binding_err: HaiBindingError = hai_err.into();
        assert_eq!(binding_err.kind, ErrorKind::ApiError);
    }

    #[test]
    fn hai_error_provider_maps_to_provider_error() {
        let hai_err = HaiError::Provider("signing failed".to_string());
        let binding_err: HaiBindingError = hai_err.into();
        assert_eq!(binding_err.kind, ErrorKind::ProviderError);
    }

    #[test]
    fn hai_error_json_maps_to_serialization_failed() {
        let json_err = serde_json::from_str::<Value>("not json").unwrap_err();
        let hai_err = HaiError::Json(json_err);
        let binding_err: HaiBindingError = hai_err.into();
        assert_eq!(binding_err.kind, ErrorKind::SerializationFailed);
    }

    #[test]
    fn hai_error_config_not_found_maps_to_config_failed() {
        let hai_err = HaiError::ConfigNotFound {
            path: "/bad/path".to_string(),
        };
        let binding_err: HaiBindingError = hai_err.into();
        assert_eq!(binding_err.kind, ErrorKind::ConfigFailed);
    }

    #[test]
    fn hai_error_config_invalid_maps_to_config_failed() {
        let hai_err = HaiError::ConfigInvalid {
            message: "bad config".to_string(),
        };
        let binding_err: HaiBindingError = hai_err.into();
        assert_eq!(binding_err.kind, ErrorKind::ConfigFailed);
    }

    #[test]
    fn hai_error_validation_maps_to_invalid_argument() {
        let hai_err = HaiError::Validation {
            field: "base_url".to_string(),
            message: "must start with http".to_string(),
        };
        let binding_err: HaiBindingError = hai_err.into();
        assert_eq!(binding_err.kind, ErrorKind::InvalidArgument);
    }

    #[test]
    fn serde_error_maps_to_serialization_failed() {
        let json_err = serde_json::from_str::<Value>("not json").unwrap_err();
        let binding_err: HaiBindingError = json_err.into();
        assert_eq!(binding_err.kind, ErrorKind::SerializationFailed);
    }

    #[test]
    fn wrapper_invalid_config_returns_config_failed() {
        use haiai::jacs::StaticJacsProvider;

        let provider = StaticJacsProvider::new("test-id");
        let options = HaiClientOptions {
            base_url: "not-a-url".to_string(),
            ..HaiClientOptions::default()
        };

        let result = HaiClientWrapper::new(Box::new(provider), options);
        assert!(result.is_err());
        let err = result.unwrap_err();
        // The validation error from HaiClient::new maps to InvalidArgument
        assert_eq!(err.kind, ErrorKind::InvalidArgument);
    }

    #[tokio::test]
    async fn wrapper_from_config_json_works() {
        use haiai::jacs::StaticJacsProvider;

        let provider = StaticJacsProvider::new("test-id");
        let config = r#"{"base_url": "https://beta.hai.ai", "timeout_secs": 10, "max_retries": 2}"#;

        let wrapper = HaiClientWrapper::from_config_json(config, Box::new(provider));
        assert!(wrapper.is_ok());

        let wrapper = wrapper.unwrap();
        assert_eq!(wrapper.jacs_id().await, "test-id");
        assert_eq!(wrapper.base_url().await, "https://beta.hai.ai");
    }

    #[tokio::test]
    async fn wrapper_from_config_json_with_client_type_works() {
        use haiai::jacs::StaticJacsProvider;

        let provider = StaticJacsProvider::new("test-id");
        let config = r#"{"base_url": "https://beta.hai.ai", "client_type": "python"}"#;

        let wrapper = HaiClientWrapper::from_config_json(config, Box::new(provider));
        assert!(
            wrapper.is_ok(),
            "from_config_json with client_type should succeed"
        );

        let wrapper = wrapper.unwrap();
        // Verify the client_type -> client_identifier transformation produced the correct prefix.
        // The exact version suffix comes from CARGO_PKG_VERSION so we only assert the prefix.
        assert!(
            wrapper.client_identifier().starts_with("haiai-python/"),
            "Expected client_identifier to start with 'haiai-python/', got: {}",
            wrapper.client_identifier()
        );
    }

    #[tokio::test]
    async fn wrapper_without_client_type_defaults_to_rust() {
        use haiai::jacs::StaticJacsProvider;

        let provider = StaticJacsProvider::new("test-id");
        let config = r#"{"base_url": "https://beta.hai.ai"}"#;

        let wrapper = HaiClientWrapper::from_config_json(config, Box::new(provider))
            .expect("from_config_json without client_type should succeed");

        // Without client_type, should default to "haiai-rust/{version}"
        assert!(
            wrapper.client_identifier().starts_with("haiai-rust/"),
            "Expected client_identifier to default to 'haiai-rust/', got: {}",
            wrapper.client_identifier()
        );
    }

    #[tokio::test]
    async fn wrapper_from_config_json_invalid_json_returns_config_failed() {
        use haiai::jacs::StaticJacsProvider;

        let provider = StaticJacsProvider::new("test-id");
        let result = HaiClientWrapper::from_config_json("not json", Box::new(provider));
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, ErrorKind::ConfigFailed);
    }

    #[tokio::test]
    async fn wrapper_set_agent_state() {
        use haiai::jacs::StaticJacsProvider;

        let provider = StaticJacsProvider::new("test-id");
        let wrapper = HaiClientWrapper::new(
            Box::new(provider),
            HaiClientOptions::default(),
        )
        .unwrap();

        // Set and get hai_agent_id
        wrapper.set_hai_agent_id("agent-123".to_string()).await;
        assert_eq!(wrapper.hai_agent_id().await, "agent-123");

        // Set and get agent_email
        assert!(wrapper.agent_email().await.is_none());
        wrapper.set_agent_email("test@hai.ai".to_string()).await;
        assert_eq!(wrapper.agent_email().await.unwrap(), "test@hai.ai");
    }

    // =========================================================================
    // Contract file validation tests
    // =========================================================================

    #[test]
    fn methods_json_is_valid() {
        let json_str = include_str!("../methods.json");
        let val: Value = serde_json::from_str(json_str).expect("methods.json must be valid JSON");
        assert!(val.is_object(), "methods.json must be a JSON object");
    }

    #[test]
    fn methods_json_has_required_sections() {
        let json_str = include_str!("../methods.json");
        let val: Value = serde_json::from_str(json_str).unwrap();
        let obj = val.as_object().unwrap();

        for section in &["methods", "streaming", "callback", "sync", "mutating", "excluded", "summary"] {
            assert!(obj.contains_key(*section), "methods.json missing section: {section}");
        }
    }

    #[test]
    fn methods_json_async_methods_have_required_fields() {
        let json_str = include_str!("../methods.json");
        let val: Value = serde_json::from_str(json_str).unwrap();
        let methods = val["methods"].as_array().unwrap();

        for method in methods {
            let name = method.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
            assert!(method.get("category").is_some(), "method {name} missing 'category'");
            assert!(method.get("params").is_some(), "method {name} missing 'params'");
            assert!(method.get("returns").is_some(), "method {name} missing 'returns'");
        }
    }

    #[test]
    fn methods_json_no_duplicate_names() {
        let json_str = include_str!("../methods.json");
        let val: Value = serde_json::from_str(json_str).unwrap();
        let methods = val["methods"].as_array().unwrap();

        let mut names = std::collections::HashSet::new();
        for method in methods {
            let name = method["name"].as_str().unwrap();
            assert!(names.insert(name), "duplicate method name: {name}");
        }
    }

    #[test]
    fn methods_json_summary_counts_match() {
        let json_str = include_str!("../methods.json");
        let val: Value = serde_json::from_str(json_str).unwrap();

        let async_count = val["methods"].as_array().unwrap().len();
        let streaming_count = val["streaming"].as_array().unwrap().len();
        let callback_count = val["callback"].as_array().unwrap().len();
        let sync_count = val["sync"].as_array().unwrap().len();
        let mutating_count = val["mutating"].as_array().unwrap().len();
        let excluded_count = val["excluded"].as_array().unwrap().len();

        let summary = &val["summary"];
        assert_eq!(async_count, summary["async_methods"].as_u64().unwrap() as usize, "async count mismatch");
        assert_eq!(streaming_count, summary["streaming_methods"].as_u64().unwrap() as usize, "streaming count mismatch");
        assert_eq!(callback_count, summary["callback_methods"].as_u64().unwrap() as usize, "callback count mismatch");
        assert_eq!(sync_count, summary["sync_methods"].as_u64().unwrap() as usize, "sync count mismatch");
        assert_eq!(mutating_count, summary["mutating_methods"].as_u64().unwrap() as usize, "mutating count mismatch");
        assert_eq!(excluded_count, summary["excluded_methods"].as_u64().unwrap() as usize, "excluded count mismatch");
    }

    // =========================================================================
    // from_config_json_auto tests
    // =========================================================================

    #[tokio::test]
    async fn auto_config_without_jacs_path_uses_static_provider() {
        let config = r#"{"base_url": "https://beta.hai.ai", "jacs_id": "auto-test-id"}"#;
        let wrapper = HaiClientWrapper::from_config_json_auto(config);
        assert!(wrapper.is_ok(), "from_config_json_auto should succeed without jacs_config_path");

        let wrapper = wrapper.unwrap();
        assert_eq!(wrapper.jacs_id().await, "auto-test-id");
    }

    #[test]
    fn auto_config_invalid_json_returns_config_failed() {
        let result = HaiClientWrapper::from_config_json_auto("not json");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, ErrorKind::ConfigFailed);
    }

    #[test]
    fn auto_config_with_bad_jacs_path_returns_config_failed() {
        let config = r#"{"jacs_config_path": "/nonexistent/jacs.config.json", "jacs_id": "test"}"#;
        let result = HaiClientWrapper::from_config_json_auto(config);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind, ErrorKind::ConfigFailed);
        assert!(err.message.contains("/nonexistent/jacs.config.json"), "error should mention the bad path");
    }

    #[test]
    fn auto_config_with_jacs_path_to_invalid_file_returns_config_failed() {
        // Create a temporary file with invalid (non-JSON) contents
        let dir = std::env::temp_dir().join("hai_binding_core_test_045");
        std::fs::create_dir_all(&dir).unwrap();
        let bad_config = dir.join("invalid_jacs_config.json");
        std::fs::write(&bad_config, "this is not json").unwrap();

        let config = format!(
            r#"{{"jacs_config_path": "{}", "jacs_id": "test"}}"#,
            bad_config.display()
        );
        let result = HaiClientWrapper::from_config_json_auto(&config);
        assert!(result.is_err(), "should fail for invalid JACS config file contents");
        let err = result.unwrap_err();
        assert_eq!(err.kind, ErrorKind::ConfigFailed);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn auto_config_with_empty_jacs_path_uses_static_provider() {
        // Empty string for jacs_config_path should be treated as absent
        let config = r#"{"jacs_config_path": "", "jacs_id": "empty-path-test"}"#;
        // This should either succeed (treating empty as absent) or fail gracefully
        let result = HaiClientWrapper::from_config_json_auto(config);
        // Empty string is truthy in the JSON sense (Some("")), so it will try to load
        // from empty path and fail with ConfigFailed
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, ErrorKind::ConfigFailed);
    }

    #[test]
    fn auto_config_missing_both_jacs_path_and_jacs_id_uses_empty_id() {
        // No jacs_config_path and no jacs_id -- should still create with empty static ID
        let config = r#"{"base_url": "https://beta.hai.ai"}"#;
        let result = HaiClientWrapper::from_config_json_auto(config);
        assert!(result.is_ok(), "should succeed with no jacs_id (empty default)");
    }

    #[tokio::test]
    async fn auto_config_defaults_base_url() {
        let config = r#"{"jacs_id": "default-test"}"#;
        let wrapper = HaiClientWrapper::from_config_json_auto(config).unwrap();
        assert_eq!(wrapper.base_url().await, "https://beta.hai.ai");
    }

    // =========================================================================
    // JACS provider integration tests (using StaticJacsProvider)
    // =========================================================================

    #[tokio::test]
    async fn build_auth_header_uses_provider_signing() {
        let config = r#"{"jacs_id": "sign-test-id"}"#;
        let wrapper = HaiClientWrapper::from_config_json_auto(config).unwrap();

        // StaticJacsProvider produces deterministic base64-encoded "sig:..." signatures.
        // build_auth_header should return a JACS auth header string.
        let result = wrapper.build_auth_header().await;
        assert!(result.is_ok(), "build_auth_header should succeed with StaticJacsProvider");

        let header = result.unwrap();
        assert!(header.starts_with("JACS "), "auth header should start with 'JACS '");
        assert!(header.contains("sign-test-id"), "auth header should contain the jacs_id");
    }

    #[tokio::test]
    async fn sign_message_uses_provider() {
        let config = r#"{"jacs_id": "sign-test"}"#;
        let wrapper = HaiClientWrapper::from_config_json_auto(config).unwrap();

        let result = wrapper.sign_message("hello world").await;
        assert!(result.is_ok(), "sign_message should succeed with StaticJacsProvider");

        // StaticJacsProvider returns base64("sig:message")
        let sig = result.unwrap();
        // The signature should be a non-empty string (JSON-encoded)
        assert!(!sig.is_empty());
    }

    #[tokio::test]
    async fn canonical_json_normalizes() {
        let config = r#"{"jacs_id": "canon-test"}"#;
        let wrapper = HaiClientWrapper::from_config_json_auto(config).unwrap();

        let result = wrapper.canonical_json(r#"{"b": 2, "a": 1}"#).await;
        assert!(result.is_ok(), "canonical_json should succeed");

        let canonical = result.unwrap();
        // RFC 8785 orders keys alphabetically
        assert!(canonical.contains(r#""a""#));
        assert!(canonical.contains(r#""b""#));
    }

    // =========================================================================
    // Error kind Display tests
    // =========================================================================

    #[test]
    fn error_kind_display_formats_correctly() {
        assert_eq!(ErrorKind::ConfigFailed.to_string(), "ConfigFailed");
        assert_eq!(ErrorKind::AuthFailed.to_string(), "AuthFailed");
        assert_eq!(ErrorKind::RateLimited.to_string(), "RateLimited");
        assert_eq!(ErrorKind::NotFound.to_string(), "NotFound");
        assert_eq!(ErrorKind::ApiError.to_string(), "ApiError");
        assert_eq!(ErrorKind::NetworkFailed.to_string(), "NetworkFailed");
        assert_eq!(ErrorKind::SerializationFailed.to_string(), "SerializationFailed");
        assert_eq!(ErrorKind::InvalidArgument.to_string(), "InvalidArgument");
        assert_eq!(ErrorKind::ProviderError.to_string(), "ProviderError");
        assert_eq!(ErrorKind::Generic.to_string(), "Generic");
    }

    #[test]
    fn binding_error_display_shows_message() {
        let err = HaiBindingError::new(ErrorKind::AuthFailed, "token expired");
        assert_eq!(err.to_string(), "token expired");
    }

    #[test]
    fn provider_error_maps_correctly() {
        let hai_err = HaiError::Provider("jacs not loaded".to_string());
        let binding_err: HaiBindingError = hai_err.into();
        assert_eq!(binding_err.kind, ErrorKind::ProviderError);
        assert!(binding_err.message.contains("jacs not loaded"));
    }

    #[test]
    fn missing_jacs_id_maps_to_config_failed() {
        let hai_err = HaiError::MissingJacsId;
        let binding_err: HaiBindingError = hai_err.into();
        assert_eq!(binding_err.kind, ErrorKind::ConfigFailed);
    }

    // =========================================================================
    // Issue 064: methods.json ↔ HaiClientWrapper parity validation
    // =========================================================================

    /// Validates that every method listed in methods.json (async, sync, mutating)
    /// actually exists as a public method on HaiClientWrapper.
    ///
    /// This catches drift: if someone adds a method to HaiClient but forgets
    /// methods.json (or vice versa), this test fails.
    #[test]
    fn methods_json_matches_wrapper_impl() {
        let json_str = include_str!("../methods.json");
        let val: Value = serde_json::from_str(json_str).unwrap();

        // Collect all method names that should be on HaiClientWrapper
        let mut expected: Vec<String> = Vec::new();

        // async methods (the main surface area)
        for m in val["methods"].as_array().unwrap() {
            expected.push(m["name"].as_str().unwrap().to_string());
        }
        // sync methods (accessors + jacs delegation), excluding "new" (constructor)
        for m in val["sync"].as_array().unwrap() {
            let name = m["name"].as_str().unwrap();
            if name != "new" {
                expected.push(name.to_string());
            }
        }
        // mutating methods
        for m in val["mutating"].as_array().unwrap() {
            expected.push(m["name"].as_str().unwrap().to_string());
        }

        // Read the source file and find all `pub async fn <name>` and `pub fn <name>` on HaiClientWrapper
        let source = include_str!("lib.rs");
        let mut impl_methods: std::collections::HashSet<String> = std::collections::HashSet::new();

        // We're inside `impl HaiClientWrapper { ... }` — find pub methods
        // Match patterns: `pub async fn <name>(` and `pub fn <name>(`
        let re_async = regex::Regex::new(r"pub async fn (\w+)\s*\(").unwrap();
        let re_sync = regex::Regex::new(r"pub fn (\w+)\s*\(").unwrap();

        for cap in re_async.captures_iter(source) {
            impl_methods.insert(cap[1].to_string());
        }
        for cap in re_sync.captures_iter(source) {
            impl_methods.insert(cap[1].to_string());
        }

        // Verify every expected method exists in the impl
        let mut missing: Vec<&str> = Vec::new();
        for name in &expected {
            if !impl_methods.contains(name.as_str()) {
                missing.push(name);
            }
        }
        assert!(
            missing.is_empty(),
            "methods.json lists methods not found on HaiClientWrapper: {:?}",
            missing
        );

        // Verify no impl methods are missing from methods.json
        // (exclude constructors and test helpers)
        let excluded_from_check: std::collections::HashSet<&str> = [
            "new", "from_config_json", "from_config_json_auto",
            // Streaming functions are listed in the "streaming" section of methods.json,
            // not the "methods" section. connect_sse/connect_ws are on HaiClientWrapper,
            // while sse_next_event/sse_close/ws_next_event/ws_close are standalone fns.
            "connect_sse", "sse_next_event", "sse_close",
            "connect_ws", "ws_next_event", "ws_close",
        ].into_iter().collect();

        let expected_set: std::collections::HashSet<&str> = expected.iter().map(|s| s.as_str()).collect();
        let mut undocumented: Vec<&str> = Vec::new();
        for name in &impl_methods {
            if !expected_set.contains(name.as_str()) && !excluded_from_check.contains(name.as_str()) {
                undocumented.push(name);
            }
        }
        assert!(
            undocumented.is_empty(),
            "HaiClientWrapper has methods not listed in methods.json: {:?}",
            undocumented
        );
    }

    // =========================================================================
    // SSE/WS handle management tests
    // =========================================================================

    #[test]
    fn handle_counter_increments() {
        let h1 = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        let h2 = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
        assert!(h2 > h1, "handle counter should increment monotonically");
    }

    #[tokio::test]
    async fn sse_next_event_invalid_handle_returns_error() {
        let result = sse_next_event(999_999).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidArgument);
        assert!(err.message.contains("invalid SSE handle"));
    }

    #[tokio::test]
    async fn ws_next_event_invalid_handle_returns_error() {
        let result = ws_next_event(999_998).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidArgument);
        assert!(err.message.contains("invalid WS handle"));
    }

    #[tokio::test]
    async fn sse_close_nonexistent_handle_is_noop() {
        // Closing a handle that doesn't exist should not error
        let result = sse_close(888_888).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn ws_close_nonexistent_handle_is_noop() {
        // Closing a handle that doesn't exist should not error
        let result = ws_close(888_887).await;
        assert!(result.is_ok());
    }

    #[test]
    fn max_streaming_handles_constant_is_reasonable() {
        assert_eq!(MAX_STREAMING_HANDLES, 100);
    }

    #[test]
    fn handle_idle_timeout_is_30_minutes() {
        assert_eq!(HANDLE_IDLE_TIMEOUT, std::time::Duration::from_secs(30 * 60));
    }

    // =========================================================================
    // Phase 2 method coverage tests (binding-core delegation)
    // =========================================================================

    #[tokio::test]
    async fn fetch_server_keys_method_exists_on_wrapper() {
        // Verify fetch_server_keys is callable (will fail at runtime without
        // a real server, but should parse and compile)
        let config = r#"{"jacs_id": "test-fskeys"}"#;
        let wrapper = HaiClientWrapper::from_config_json_auto(config).unwrap();
        // We can't call it without a server, but verify it exists via compilation
        let _method_exists = HaiClientWrapper::fetch_server_keys;
        let _ = wrapper;
    }

    #[tokio::test]
    async fn sign_email_raw_method_exists_on_wrapper() {
        let _method_exists = HaiClientWrapper::sign_email_raw;
    }

    #[tokio::test]
    async fn verify_email_raw_method_exists_on_wrapper() {
        let _method_exists = HaiClientWrapper::verify_email_raw;
    }

    #[tokio::test]
    async fn get_raw_email_method_exists_on_wrapper() {
        let _method_exists = HaiClientWrapper::get_raw_email;
    }

    #[test]
    fn get_raw_email_wire_shape_contains_all_keys_available_true() {
        // End-to-end JSON shape check: serialize a RawEmailResponse
        // through the same path get_raw_email uses (to_wire_json).
        // This is what clients parse across the FFI boundary.
        use haiai::RawEmailResponse;
        let resp = RawEmailResponse {
            message_id: "m.1".into(),
            rfc_message_id: Some("<a@b>".into()),
            available: true,
            raw_email: Some(b"hello".to_vec()),
            size_bytes: Some(5),
            omitted_reason: None,
        };
        let wire = serde_json::to_string(&resp.to_wire_json()).expect("serialize");
        assert!(wire.contains("\"available\":true"));
        assert!(wire.contains("\"raw_email_b64\":\"aGVsbG8=\""));
        assert!(wire.contains("\"message_id\":\"m.1\""));
        assert!(wire.contains("\"size_bytes\":5"));
        assert!(wire.contains("\"omitted_reason\":null"));
    }

    #[test]
    fn get_raw_email_wire_shape_available_false_not_stored() {
        use haiai::RawEmailResponse;
        let resp = RawEmailResponse {
            message_id: "m.2".into(),
            rfc_message_id: None,
            available: false,
            raw_email: None,
            size_bytes: None,
            omitted_reason: Some("not_stored".into()),
        };
        let wire = serde_json::to_string(&resp.to_wire_json()).expect("serialize");
        assert!(wire.contains("\"available\":false"));
        assert!(wire.contains("\"raw_email_b64\":null"));
        assert!(wire.contains("\"omitted_reason\":\"not_stored\""));
    }

    /// Fixture parity: `get_raw_email` must appear in `ffi_method_parity.json`
    /// under `email_core` to keep the contract coherent across all SDKs.
    #[test]
    fn get_raw_email_is_declared_in_ffi_fixture() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/ffi_method_parity.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        let email_core = json
            .get("methods")
            .and_then(|m| m.get("email_core"))
            .and_then(|a| a.as_array())
            .expect("methods.email_core array");
        let has = email_core
            .iter()
            .any(|m| m.get("name").and_then(|n| n.as_str()) == Some("get_raw_email"));
        assert!(has, "fixture missing get_raw_email in email_core");
    }

    #[tokio::test]
    async fn create_attestation_method_exists_on_wrapper() {
        let _method_exists = HaiClientWrapper::create_attestation;
    }

    #[tokio::test]
    async fn list_attestations_method_exists_on_wrapper() {
        let _method_exists = HaiClientWrapper::list_attestations;
    }

    #[tokio::test]
    async fn get_attestation_method_exists_on_wrapper() {
        let _method_exists = HaiClientWrapper::get_attestation;
    }

    #[tokio::test]
    async fn verify_attestation_method_exists_on_wrapper() {
        let _method_exists = HaiClientWrapper::verify_attestation;
    }

    #[tokio::test]
    async fn create_email_template_method_exists_on_wrapper() {
        let _method_exists = HaiClientWrapper::create_email_template;
    }

    #[tokio::test]
    async fn list_email_templates_method_exists_on_wrapper() {
        let _method_exists = HaiClientWrapper::list_email_templates;
    }

    #[tokio::test]
    async fn get_email_template_method_exists_on_wrapper() {
        let _method_exists = HaiClientWrapper::get_email_template;
    }

    #[tokio::test]
    async fn update_email_template_method_exists_on_wrapper() {
        let _method_exists = HaiClientWrapper::update_email_template;
    }

    #[tokio::test]
    async fn delete_email_template_method_exists_on_wrapper() {
        let _method_exists = HaiClientWrapper::delete_email_template;
    }

    #[tokio::test]
    async fn register_new_agent_method_exists_on_wrapper() {
        let _method_exists = HaiClientWrapper::register_new_agent;
    }

    #[tokio::test]
    async fn register_new_agent_rejects_missing_agent_name() {
        let config = r#"{"jacs_id": "reg-test"}"#;
        let wrapper = HaiClientWrapper::from_config_json_auto(config).unwrap();
        let result = wrapper
            .register_new_agent(r#"{"password": "secret123"}"#)
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidArgument);
        assert!(
            err.message.contains("agent_name"),
            "error should mention agent_name: {}",
            err.message
        );
    }

    #[tokio::test]
    async fn register_new_agent_rejects_missing_password() {
        let config = r#"{"jacs_id": "reg-test"}"#;
        let wrapper = HaiClientWrapper::from_config_json_auto(config).unwrap();
        let result = wrapper
            .register_new_agent(r#"{"agent_name": "test-bot"}"#)
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind, ErrorKind::InvalidArgument);
        assert!(
            err.message.contains("password"),
            "error should mention password: {}",
            err.message
        );
    }

    #[tokio::test]
    async fn register_new_agent_rejects_invalid_json() {
        let config = r#"{"jacs_id": "reg-test"}"#;
        let wrapper = HaiClientWrapper::from_config_json_auto(config).unwrap();
        let result = wrapper.register_new_agent("not valid json").await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, ErrorKind::SerializationFailed);
    }

    #[tokio::test]
    async fn rotate_keys_accepts_valid_json() {
        let config = r#"{"jacs_id": "rotate-test"}"#;
        let wrapper = HaiClientWrapper::from_config_json_auto(config).unwrap();
        // rotate_keys with invalid options should still parse (options are valid JSON)
        // but will fail at the JACS provider level since StaticJacsProvider can't rotate
        let result = wrapper.rotate_keys(r#"{"register_with_hai": false}"#).await;
        // Expected to fail (StaticJacsProvider doesn't support rotation), but not with
        // a serialization error
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_ne!(err.kind, ErrorKind::SerializationFailed, "options JSON should parse correctly");
    }

    #[tokio::test]
    async fn rotate_keys_rejects_invalid_json() {
        let config = r#"{"jacs_id": "rotate-test"}"#;
        let wrapper = HaiClientWrapper::from_config_json_auto(config).unwrap();
        let result = wrapper.rotate_keys("not json").await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, ErrorKind::SerializationFailed);
    }

    // =========================================================================
    // TASK_003: Media-signing wrapper methods
    // =========================================================================

    /// Materialize the fixture JACS agent into a tempdir with adjusted paths.
    /// Returns (TempDir, config_path).
    fn write_temp_media_fixture_config() -> (tempfile::TempDir, std::path::PathBuf) {
        std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", "secretpassord");

        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/jacs-agent/jacs.config.json")
            .canonicalize()
            .expect("fixtures/jacs-agent/jacs.config.json must exist");
        let source_dir = source.parent().expect("fixture config dir");
        let mut value: Value =
            serde_json::from_str(&std::fs::read_to_string(&source).expect("read fixture"))
                .expect("parse fixture");

        let temp_dir = tempfile::tempdir().expect("tempdir");

        // Copy keys
        let source_key_dir = value
            .get("jacs_key_directory")
            .and_then(Value::as_str)
            .map(|p| {
                if std::path::PathBuf::from(p).is_absolute() {
                    std::path::PathBuf::from(p)
                } else {
                    source_dir.join(p)
                }
            })
            .expect("key dir");
        let temp_key_dir = temp_dir.path().join("keys");
        std::fs::create_dir_all(&temp_key_dir).expect("temp key dir");
        for entry in std::fs::read_dir(&source_key_dir).expect("read keys") {
            let entry = entry.expect("key entry");
            std::fs::copy(entry.path(), temp_key_dir.join(entry.file_name()))
                .expect("copy key");
        }

        // Copy data (with underscore→colon filename normalization)
        let source_data_dir = value
            .get("jacs_data_directory")
            .and_then(Value::as_str)
            .map(|p| {
                if std::path::PathBuf::from(p).is_absolute() {
                    std::path::PathBuf::from(p)
                } else {
                    source_dir.join(p)
                }
            })
            .expect("data dir");
        let temp_data_dir = temp_dir.path().join("data");
        fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) {
            std::fs::create_dir_all(dst).expect("create dst");
            for entry in std::fs::read_dir(src).expect("read src") {
                let entry = entry.expect("entry");
                let src_path = entry.path();
                let name = entry.file_name().to_string_lossy().replace('_', ":");
                let dst_path = dst.join(&name);
                if src_path.is_dir() {
                    copy_dir_recursive(&src_path, &dst_path);
                } else {
                    std::fs::copy(&src_path, &dst_path).expect("copy file");
                }
            }
        }
        copy_dir_recursive(&source_data_dir, &temp_data_dir);

        value["jacs_data_directory"] =
            Value::String(temp_data_dir.to_string_lossy().into_owned());
        value["jacs_key_directory"] =
            Value::String(temp_key_dir.to_string_lossy().into_owned());

        let config_path = temp_dir.path().join("media-binding.config.json");
        std::fs::write(
            &config_path,
            serde_json::to_vec_pretty(&value).expect("serialize config"),
        )
        .expect("write config");
        (temp_dir, config_path)
    }

    fn make_media_test_png(width: u32, height: u32) -> Vec<u8> {
        let img =
            image::RgbaImage::from_pixel(width, height, image::Rgba([32, 64, 128, 255]));
        let mut buf = Vec::new();
        let mut cur = std::io::Cursor::new(&mut buf);
        img.write_to(&mut cur, image::ImageFormat::Png)
            .expect("png encode");
        buf
    }

    fn build_media_wrapper(config_path: &std::path::Path) -> HaiClientWrapper {
        let json = format!(
            r#"{{"base_url":"https://beta.hai.ai","jacs_config_path":"{}","jacs_id":"media-binding-test"}}"#,
            config_path.display()
        );
        HaiClientWrapper::from_config_json_auto(&json).expect("build wrapper")
    }

    #[test]
    fn parse_sign_text_opts_defaults_when_empty() {
        let opts = parse_sign_text_opts("").expect("default");
        assert!(opts.backup);
        assert!(!opts.allow_duplicate);
    }

    #[test]
    fn parse_sign_text_opts_explicit() {
        let opts = parse_sign_text_opts(r#"{"backup": false, "allow_duplicate": true}"#)
            .expect("parse");
        assert!(!opts.backup);
        assert!(opts.allow_duplicate);
    }

    #[test]
    fn parse_verify_image_opts_flat_robust_maps_to_scan_robust() {
        // PRD §4.3: user-facing key "robust" maps to JACS-internal scan_robust.
        let opts =
            parse_verify_image_opts(r#"{"robust": true, "strict": true}"#).expect("parse");
        assert!(opts.scan_robust, "flat 'robust' must populate scan_robust");
        assert!(opts.base.strict);
    }

    #[test]
    fn parse_sign_image_opts_invalid_robust_returns_invalid_argument() {
        let err = parse_sign_image_opts(r#"{"robust": "yes"}"#).expect_err("must fail");
        assert_eq!(err.kind, ErrorKind::InvalidArgument);
    }

    #[test]
    fn parse_extract_opts_defaults_to_decoded() {
        assert!(!parse_extract_opts("").expect("default"));
        assert!(parse_extract_opts(r#"{"raw_payload": true}"#).expect("parse"));
    }

    #[test]
    fn methods_json_includes_media_methods() {
        let json_str = include_str!("../methods.json");
        let val: Value = serde_json::from_str(json_str).unwrap();
        let methods = val["methods"].as_array().unwrap();
        let names: Vec<&str> = methods
            .iter()
            .filter_map(|m| m.get("name").and_then(|n| n.as_str()))
            .collect();
        for required in &[
            "sign_text",
            "verify_text",
            "sign_image",
            "verify_image",
            "extract_media_signature",
        ] {
            assert!(
                names.contains(required),
                "methods.json missing entry: {required}"
            );
        }
        for required in &[
            "sign_text",
            "verify_text",
            "sign_image",
            "verify_image",
            "extract_media_signature",
        ] {
            let entry = methods
                .iter()
                .find(|m| m.get("name").and_then(|n| n.as_str()) == Some(required))
                .unwrap();
            assert_eq!(
                entry.get("group").and_then(|g| g.as_str()),
                Some("media_local"),
                "{required} should be in group media_local"
            );
            assert_eq!(
                entry.get("category").and_then(|c| c.as_str()),
                Some("async"),
                "{required} should be category async"
            );
        }
        let summary = val.get("summary").unwrap();
        assert_eq!(summary["async_methods"].as_u64(), Some(55));
        assert_eq!(summary["total_public_methods"].as_u64(), Some(81));
    }

    #[tokio::test]
    async fn wrapper_verify_text_missing_signature_envelope() {
        let (temp_dir, config_path) = write_temp_media_fixture_config();
        let wrapper = build_media_wrapper(&config_path);
        let path = temp_dir.path().join("unsigned.md");
        std::fs::write(&path, b"# unsigned\n").expect("write");

        let json = wrapper
            .verify_text(path.to_str().unwrap(), "{}")
            .await
            .expect("verify_text");
        let parsed: Value = serde_json::from_str(&json).expect("envelope JSON");
        assert_eq!(parsed["status"].as_str(), Some("missing_signature"));
        assert!(parsed["signatures"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn wrapper_sign_text_round_trip() {
        let (temp_dir, config_path) = write_temp_media_fixture_config();
        let wrapper = build_media_wrapper(&config_path);
        let path = temp_dir.path().join("hello.md");
        std::fs::write(&path, b"# Hello\n").expect("write");

        let outcome_json = wrapper
            .sign_text(path.to_str().unwrap(), "{}")
            .await
            .expect("sign_text");
        let parsed: Value = serde_json::from_str(&outcome_json).expect("outcome JSON");
        assert_eq!(parsed["signers_added"].as_u64(), Some(1));

        let verify_json = wrapper
            .verify_text(path.to_str().unwrap(), "{}")
            .await
            .expect("verify_text");
        let parsed: Value = serde_json::from_str(&verify_json).expect("verify JSON");
        assert_eq!(parsed["status"].as_str(), Some("signed"));
        let sigs = parsed["signatures"].as_array().expect("signatures");
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0]["status"].as_str(), Some("valid"));
    }

    #[tokio::test]
    async fn wrapper_sign_image_round_trip() {
        let (temp_dir, config_path) = write_temp_media_fixture_config();
        let wrapper = build_media_wrapper(&config_path);
        let in_path = temp_dir.path().join("in.png");
        std::fs::write(&in_path, &make_media_test_png(32, 32)).expect("write");
        let out_path = temp_dir.path().join("out.png");

        let signed_json = wrapper
            .sign_image(
                in_path.to_str().unwrap(),
                out_path.to_str().unwrap(),
                "{}",
            )
            .await
            .expect("sign_image");
        let parsed: Value = serde_json::from_str(&signed_json).expect("signed JSON");
        assert_eq!(parsed["format"].as_str(), Some("png"));
        let signer_id = parsed["signer_id"].as_str().unwrap().to_string();

        let verify_json = wrapper
            .verify_image(out_path.to_str().unwrap(), "{}")
            .await
            .expect("verify_image");
        let parsed: Value = serde_json::from_str(&verify_json).expect("verify JSON");
        assert_eq!(parsed["status"].as_str(), Some("valid"));
        assert_eq!(parsed["signer_id"].as_str(), Some(signer_id.as_str()));
    }

    #[tokio::test]
    async fn wrapper_verify_image_tampered_returns_hash_mismatch() {
        let (temp_dir, config_path) = write_temp_media_fixture_config();
        let wrapper = build_media_wrapper(&config_path);
        let in_path = temp_dir.path().join("in.png");
        std::fs::write(&in_path, &make_media_test_png(32, 32)).expect("write");
        let out_path = temp_dir.path().join("out.png");
        wrapper
            .sign_image(in_path.to_str().unwrap(), out_path.to_str().unwrap(), "{}")
            .await
            .expect("sign");

        // Mutate one byte in the IDAT region (after the iTXt chunk holding the
        // signature). Our prebuilt PNG's IDAT starts ~ byte 49.
        let mut bytes = std::fs::read(&out_path).expect("read signed");
        let idat_start = bytes
            .windows(4)
            .position(|w| w == b"IDAT")
            .expect("IDAT marker");
        // Flip one byte in the compressed data (well past the marker).
        if let Some(b) = bytes.get_mut(idat_start + 6) {
            *b ^= 0x01;
        }
        std::fs::write(&out_path, &bytes).expect("write tampered");

        let verify_json = wrapper
            .verify_image(out_path.to_str().unwrap(), "{}")
            .await
            .expect("verify");
        let parsed: Value = serde_json::from_str(&verify_json).expect("verify JSON");
        let status = parsed["status"].as_str().unwrap_or("");
        assert!(
            status == "hash_mismatch" || status == "invalid_signature",
            "tampered image should fail with hash_mismatch or invalid_signature, got {status}"
        );
    }

    #[tokio::test]
    async fn wrapper_extract_media_signature_returns_envelope() {
        let (temp_dir, config_path) = write_temp_media_fixture_config();
        let wrapper = build_media_wrapper(&config_path);
        let in_path = temp_dir.path().join("in.png");
        std::fs::write(&in_path, &make_media_test_png(32, 32)).expect("write");
        let out_path = temp_dir.path().join("out.png");
        wrapper
            .sign_image(in_path.to_str().unwrap(), out_path.to_str().unwrap(), "{}")
            .await
            .expect("sign");

        let env_json = wrapper
            .extract_media_signature(out_path.to_str().unwrap(), "{}")
            .await
            .expect("extract");
        let parsed: Value = serde_json::from_str(&env_json).expect("envelope JSON");
        assert_eq!(parsed["present"].as_bool(), Some(true));
        let payload = parsed["payload"].as_str().expect("payload string");
        let inner: Value =
            serde_json::from_str(payload).expect("decoded payload should be JSON");
        assert!(inner.is_object());
    }

    #[tokio::test]
    async fn wrapper_sign_image_invalid_opts_json_returns_invalid_argument() {
        let (_temp_dir, config_path) = write_temp_media_fixture_config();
        let wrapper = build_media_wrapper(&config_path);
        let result = wrapper
            .sign_image("a.png", "b.png", r#"{"robust": "yes"}"#)
            .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, ErrorKind::InvalidArgument);
    }

    #[tokio::test]
    async fn wrapper_sign_image_with_static_provider_returns_provider_error() {
        // No jacs_config_path → falls back to StaticJacsProvider → media op
        // returns Provider error from the test-only fallback impl.
        let wrapper =
            HaiClientWrapper::from_config_json_auto(r#"{"jacs_id":"static-only"}"#).expect("ok");
        let dir = tempfile::tempdir().unwrap();
        let in_path = dir.path().join("a.png");
        std::fs::write(&in_path, &make_media_test_png(32, 32)).unwrap();
        let result = wrapper
            .sign_image(
                in_path.to_str().unwrap(),
                dir.path().join("b.png").to_str().unwrap(),
                "{}",
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("test-only") || err.message.contains("StaticJacsProvider"),
            "expected test-only provider error, got: {}",
            err.message
        );
    }

    #[test]
    fn media_verify_result_to_json_flattens_malformed_variant() {
        // Regression for Issue 001: JACS serializes Malformed as
        // {"malformed": "<detail>"} which language SDKs decoded as a tagged
        // object (Python str(dict), Node "[object Object]", Go decode error).
        // The translation site must produce status == "malformed" (lowercase
        // string) plus a sibling malformed_detail field.
        use haiai::jacs::{MediaVerificationResult, MediaVerifyStatus};
        let result = MediaVerificationResult {
            status: MediaVerifyStatus::Malformed("garbled iTXt chunk".to_string()),
            signer_id: None,
            algorithm: None,
            format: Some("png".to_string()),
            embedding_channels: None,
        };
        let envelope = media_verify_result_to_json(&result);
        assert_eq!(envelope["status"].as_str(), Some("malformed"));
        assert_eq!(
            envelope["malformed_detail"].as_str(),
            Some("garbled iTXt chunk")
        );
        assert_eq!(envelope["format"].as_str(), Some("png"));
        // No tagged-object leak: status must be a plain string.
        assert!(envelope["status"].is_string());
    }

    #[test]
    fn media_verify_result_to_json_flat_string_for_simple_variants() {
        // Each unit-style variant should serialize as a plain snake_case
        // string and NOT carry malformed_detail.
        use haiai::jacs::{MediaVerificationResult, MediaVerifyStatus};
        for (variant, expected) in [
            (MediaVerifyStatus::Valid, "valid"),
            (MediaVerifyStatus::InvalidSignature, "invalid_signature"),
            (MediaVerifyStatus::HashMismatch, "hash_mismatch"),
            (MediaVerifyStatus::MissingSignature, "missing_signature"),
            (MediaVerifyStatus::KeyNotFound, "key_not_found"),
            (MediaVerifyStatus::UnsupportedFormat, "unsupported_format"),
        ] {
            let result = MediaVerificationResult {
                status: variant.clone(),
                signer_id: Some("agent-1".to_string()),
                algorithm: Some("ED25519".to_string()),
                format: Some("png".to_string()),
                embedding_channels: None,
            };
            let envelope = media_verify_result_to_json(&result);
            assert_eq!(
                envelope["status"].as_str(),
                Some(expected),
                "variant {:?} should serialize as {}",
                variant,
                expected
            );
            assert!(envelope.get("malformed_detail").is_none());
        }
    }
}
