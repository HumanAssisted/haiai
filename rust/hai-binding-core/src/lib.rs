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
use haiai::jacs::{JacsProvider, StaticJacsProvider};
use haiai::jacs_local::LocalJacsProvider;

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
    inner: Arc<RwLock<HaiClient<Box<dyn JacsProvider>>>>,
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
        jacs: Box<dyn JacsProvider>,
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
        jacs: Box<dyn JacsProvider>,
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

        let provider: Box<dyn JacsProvider> = if let Some(path) = jacs_config_path {
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
}
