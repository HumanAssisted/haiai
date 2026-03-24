//! # hai-binding-core
//!
//! Shared core logic for HAI SDK language bindings (Python, Node.js, Go).
//!
//! This crate wraps `HaiClient` with a JSON-in/JSON-out interface suitable for
//! FFI consumption. All methods accept and return JSON strings — no
//! language-specific types cross this boundary.
//!
//! Follows the same pattern as JACS's `jacs-binding-core` crate.

use std::fmt;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::RwLock;

use haiai::client::{HaiClient, HaiClientOptions};
use haiai::error::HaiError;
use haiai::jacs::JacsProvider;

// =============================================================================
// Static tokio runtime for FFI callers
// =============================================================================

/// Global tokio runtime shared by all FFI bindings.
///
/// `LazyLock` is stable since Rust 1.80. This is the exact pattern used by
/// napi-rs internally.
pub static RT: std::sync::LazyLock<tokio::runtime::Runtime> =
    std::sync::LazyLock::new(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime")
    });

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
/// Uses `Arc<RwLock<...>>` because `HaiClient` has three `&mut self` methods
/// (`claim_username`, `set_hai_agent_id`, `set_agent_email`) that require
/// interior mutability. Standard read-only methods acquire a read lock;
/// the three mutating methods acquire a write lock.
pub struct HaiClientWrapper {
    inner: Arc<RwLock<HaiClient<Box<dyn JacsProvider>>>>,
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
        let client = HaiClient::new(jacs, options)
            .map_err(HaiBindingError::from)?;
        Ok(Self {
            inner: Arc::new(RwLock::new(client)),
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

        let options = HaiClientOptions {
            base_url,
            timeout: std::time::Duration::from_secs(timeout_secs),
            max_retries,
        };

        Self::new(jacs, options)
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

    /// Check if a username is available.
    pub async fn check_username(&self, username: &str) -> HaiBindingResult<String> {
        let client = self.inner.read().await;
        let result = client.check_username(username).await?;
        Ok(serde_json::to_string(&result)?)
    }

    /// Register an agent.
    pub async fn register(&self, options_json: &str) -> HaiBindingResult<String> {
        let options: haiai::types::RegisterAgentOptions = serde_json::from_str(options_json)?;
        let client = self.inner.read().await;
        let result = client.register(&options).await?;
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

    /// Claim a username for an agent. **Requires write lock.**
    pub async fn claim_username(&self, agent_id: &str, username: &str) -> HaiBindingResult<String> {
        let mut client = self.inner.write().await;
        let result = client.claim_username(agent_id, username).await?;
        Ok(serde_json::to_string(&result)?)
    }

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

    #[test]
    fn static_runtime_initializes() {
        // Force the lazy runtime to initialize
        let handle = RT.handle();
        // If we get here without panic, the runtime is initialized
        assert!(handle.metrics().num_workers() > 0);
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
}
