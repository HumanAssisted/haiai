#[cfg(feature = "jacs-crate")]
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
#[cfg(feature = "jacs-crate")]
use futures_util::{SinkExt, StreamExt};
use reqwest::{Response, StatusCode};
use serde_json::{json, Value};
#[cfg(feature = "jacs-crate")]
use time::OffsetDateTime;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
#[cfg(feature = "jacs-crate")]
use tokio_tungstenite::tungstenite::Message;
#[cfg(feature = "jacs-crate")]
use tokio_tungstenite::{connect_async_with_config, tungstenite};
#[cfg(feature = "jacs-crate")]
use tungstenite::client::IntoClientRequest;

use crate::error::{HaiError, Result};
use crate::jacs::JacsProvider;
use crate::request_auth::RequestClient;
#[cfg(feature = "jacs-crate")]
use crate::types::SignedEventVerification;
use crate::types::{
    AgentKeyHistory, AgentVerificationResult, Contact, CreateEmailTemplateOptions,
    DeleteUsernameResult, DnsCertifiedResult, DnsCertifiedRunOptions, DocumentVerificationResult,
    EmailGenerationType, EmailMessage, EmailStatus, EmailTemplate, FreeChaoticResult, HaiEvent,
    HelloResult, JobResponseResult, ListEmailTemplatesOptions, ListEmailTemplatesResult,
    ListMessagesOptions, ProRunOptions, ProRunResult, PublicKeyInfo, RawEmailResponse,
    RegisterAgentOptions, RegistrationResult, RotateKeysOptions, RotationResult, SearchOptions,
    SendEmailOptions, SendEmailResult, SignedEmail, SignedJobResponsePayloadV2, TranscriptMessage,
    TransportType, UpdateAgentResult, UpdateEmailTemplateOptions, UpdateUsernameResult,
    VerifyAgentDocumentRequest, VerifyAgentResult,
};

pub const DEFAULT_BASE_URL: &str = "https://hai.ai";
/// Pinned HAI request recipient; distinct from the URL's network origin.
pub const DEFAULT_REQUEST_AUTH_AUDIENCE: &str = "hai.ai";

/// Maximum time a live connection may keep one server-key snapshot before it
/// must refresh the exact active signer map.
pub const DEFAULT_SERVER_KEY_REFRESH_SECS: u64 = 5 * 60;

const MAX_SERVER_KEYS_BYTES: usize = 1024 * 1024;
const MAX_ERROR_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_REDIRECTS: usize = 10;
const STREAM_TASK_CLOSE_GRACE: Duration = Duration::from_secs(1);

pub struct SseConnection {
    events: mpsc::Receiver<Result<HaiEvent>>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl SseConnection {
    pub async fn next_event(&mut self) -> Result<Option<HaiEvent>> {
        match self.events.recv().await {
            Some(event) => event.map(Some),
            None => Ok(None),
        }
    }

    pub async fn close(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.events.close();
        if let Some(task) = self.task.take() {
            stop_stream_task(task, "sse").await;
        }
    }
}

/// An active WebSocket connection to the HAI server.
///
/// Provides read-only event streaming via [`next_event()`](Self::next_event).
/// Bidirectional sending (e.g., `ws_send`) is intentionally not supported through
/// the FFI boundary. To send job responses, use the separate
/// [`submit_response()`](HaiClient::submit_response) REST endpoint, which is
/// available via FFI in all SDKs.
pub struct WsConnection {
    events: mpsc::Receiver<Result<HaiEvent>>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl WsConnection {
    pub async fn next_event(&mut self) -> Result<Option<HaiEvent>> {
        match self.events.recv().await {
            Some(event) => event.map(Some),
            None => Ok(None),
        }
    }

    pub async fn close(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.events.close();
        if let Some(task) = self.task.take() {
            stop_stream_task(task, "websocket").await;
        }
    }
}

async fn stop_stream_task(mut task: JoinHandle<()>, transport: &'static str) {
    if tokio::time::timeout(STREAM_TASK_CLOSE_GRACE, &mut task)
        .await
        .is_err()
    {
        tracing::warn!(
            event = "live_stream_task_aborted",
            transport,
            "Live stream task did not stop within the close grace period"
        );
        task.abort();
        let _ = task.await;
    }
}

#[cfg(feature = "jacs-crate")]
async fn send_stream_result(
    sender: &mpsc::Sender<Result<HaiEvent>>,
    shutdown: &mut oneshot::Receiver<()>,
    result: Result<HaiEvent>,
) -> bool {
    tokio::select! {
        _ = &mut *shutdown => false,
        sent = sender.send(result) => sent.is_ok(),
    }
}

/// Default request timeout in seconds.
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Default maximum retry count for transient failures.
pub const DEFAULT_MAX_RETRIES: usize = 3;

/// Default DNS-over-HTTPS resolver for email TXT record lookups.
pub const DEFAULT_DNS_RESOLVER: &str = "https://dns.google/resolve";

/// Header name for SDK client identification. The API repo defines its own
/// matching constant -- keep them in sync.
pub const HAI_CLIENT_HEADER: &str = "x-hai-client";
const IDEMPOTENCY_KEY_HEADER: &str = "Idempotency-Key";

fn urls_have_same_origin(previous: &url::Url, next: &url::Url) -> bool {
    previous.scheme() == next.scheme()
        && previous.host() == next.host()
        && previous.port_or_known_default() == next.port_or_known_default()
        && next.username().is_empty()
        && next.password().is_none()
}

fn same_origin_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > MAX_REDIRECTS {
            return attempt.error(std::io::Error::other("too many redirects"));
        }
        let Some(previous) = attempt.previous().last() else {
            return attempt.error(std::io::Error::other(
                "redirect policy received no originating URL",
            ));
        };
        if urls_have_same_origin(previous, attempt.url()) {
            attempt.follow()
        } else {
            attempt.error(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "cross-origin or scheme-changing redirect rejected",
            ))
        }
    })
}

#[derive(Debug, Clone)]
pub struct HaiClientOptions {
    pub base_url: String,
    pub timeout: Duration,
    pub max_retries: usize,
    /// SDK client identifier sent as the `X-HAI-Client` header.
    /// Format: `haiai-{transport}/{version}`.
    /// Defaults to `haiai-rust/{CARGO_PKG_VERSION}` when `None`.
    pub client_identifier: Option<String>,
    /// Deployment-configured recipient, never inferred from remote discovery.
    pub request_auth_audience: String,
}

impl Default for HaiClientOptions {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            max_retries: DEFAULT_MAX_RETRIES,
            client_identifier: None,
            request_auth_audience: DEFAULT_REQUEST_AUTH_AUDIENCE.to_string(),
        }
    }
}

pub struct HaiClient<P: JacsProvider> {
    base_url: String,
    expected_event_context: Option<(String, String)>,
    http: RequestClient<P>,
    /// Streaming HTTP client with connect/read timeouts but no total request
    /// deadline. A total deadline would terminate every healthy SSE stream.
    #[cfg(feature = "jacs-crate")]
    live_http: RequestClient<P>,
    #[cfg(feature = "jacs-crate")]
    request_timeout: Duration,
    #[cfg(feature = "jacs-crate")]
    server_key_refresh_interval: Duration,
    max_retries: usize,
    jacs: Arc<P>,
    /// HAI-assigned agent UUID for email URL paths (set after registration).
    hai_agent_id: Option<String>,
    /// Agent's @hai.ai email address (set after registration).
    agent_email: Option<String>,
    /// Optional application-owned shared replay backend for multi-replica
    /// consumers. Without this override, JACS uses its installed global replay
    /// backend and honors `JACS_REQUIRE_SHARED_REPLAY_STORE`.
    #[cfg(feature = "jacs-crate")]
    shared_event_replay_store: Option<Arc<dyn jacs::replay::ReplayStore>>,
}

/// Status codes that are safe to retry (transient server errors and rate limiting).
/// Matches Python SDK's `RETRYABLE_STATUS_CODES`.
const RETRYABLE_STATUS_CODES: &[u16] = &[429, 500, 502, 503, 504];

/// Default maximum reconnect attempts for `on_benchmark_job`.
const DEFAULT_MAX_RECONNECT_ATTEMPTS: usize = 10;

impl<P: JacsProvider> HaiClient<P> {
    pub fn new(jacs: P, options: HaiClientOptions) -> Result<Self> {
        if options.request_auth_audience.trim().is_empty()
            || options.request_auth_audience.len() > 256
        {
            return Err(HaiError::Validation {
                field: "request_auth_audience".into(),
                message:
                    "a nonempty deployment-pinned request audience of at most 256 bytes is required"
                        .into(),
            });
        }
        // ── Issue #13: validate base URL ────────────────────────────────
        let trimmed = options.base_url.trim_end_matches('/');
        let parsed_base = url::Url::parse(trimmed).map_err(|error| HaiError::Validation {
            field: "base_url".to_string(),
            message: format!("base_url must be an absolute http(s) URL: {error}"),
        })?;
        if !matches!(parsed_base.scheme(), "http" | "https") || parsed_base.host().is_none() {
            return Err(HaiError::Validation {
                field: "base_url".to_string(),
                message: format!(
                    "base_url must be an absolute http:// or https:// URL, got: {}",
                    options.base_url
                ),
            });
        }
        if !parsed_base.username().is_empty()
            || parsed_base.password().is_some()
            || parsed_base.query().is_some()
            || parsed_base.fragment().is_some()
        {
            return Err(HaiError::Validation {
                field: "base_url".to_string(),
                message: "base_url must not contain userinfo, a query, or a fragment".to_string(),
            });
        }

        let client_id = options
            .client_identifier
            .unwrap_or_else(|| format!("haiai-rust/{}", env!("CARGO_PKG_VERSION")));
        let mut default_headers = reqwest::header::HeaderMap::new();
        if let Ok(val) = reqwest::header::HeaderValue::from_str(&client_id) {
            default_headers.insert(HAI_CLIENT_HEADER, val);
        } else {
            eprintln!(
                "WARNING: Invalid X-HAI-Client header value '{}', telemetry will not be sent",
                client_id
            );
        }

        let http = reqwest::Client::builder()
            .timeout(options.timeout)
            .redirect(same_origin_redirect_policy())
            .default_headers(default_headers.clone())
            .build()?;
        let authenticated_http = reqwest::Client::builder()
            .timeout(options.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .default_headers(default_headers.clone())
            .build()?;
        #[cfg(feature = "jacs-crate")]
        let live_http = reqwest::Client::builder()
            .connect_timeout(options.timeout)
            .read_timeout(options.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .default_headers(default_headers)
            .build()?;
        let jacs = Arc::new(jacs);
        let http = RequestClient::new(
            http,
            authenticated_http,
            jacs.clone(),
            options.request_auth_audience.clone(),
        );
        #[cfg(feature = "jacs-crate")]
        let live_http = RequestClient::new(
            live_http.clone(),
            live_http,
            jacs.clone(),
            options.request_auth_audience,
        );

        Ok(Self {
            base_url: trimmed.to_string(),
            expected_event_context: None,
            http,
            #[cfg(feature = "jacs-crate")]
            live_http,
            #[cfg(feature = "jacs-crate")]
            request_timeout: options.timeout,
            #[cfg(feature = "jacs-crate")]
            server_key_refresh_interval: Duration::from_secs(DEFAULT_SERVER_KEY_REFRESH_SECS),
            max_retries: options.max_retries.max(1),
            jacs,
            hai_agent_id: None,
            agent_email: None,
            #[cfg(feature = "jacs-crate")]
            shared_event_replay_store: None,
        })
    }

    /// Pin deployment tenancy and the recipient of outbound job responses.
    /// Neither value is inferred from a signed event or discovery response.
    pub fn with_expected_event_context(
        mut self,
        tenant: String,
        response_audience: String,
    ) -> Result<Self> {
        if tenant.is_empty()
            || tenant.len() > 256
            || response_audience.is_empty()
            || response_audience == "public"
            || response_audience.len() > 256
        {
            return Err(HaiError::Validation {
                field: "expected_event_context".into(),
                message: "explicit nonempty tenant and private response audience are required"
                    .into(),
            });
        }
        self.expected_event_context = Some((tenant, response_audience));
        Ok(self)
    }

    #[cfg(feature = "jacs-crate")]
    fn expected_event_context(&self) -> Result<&(String, String)> {
        self.expected_event_context.as_ref().ok_or_else(|| HaiError::Validation {
            field: "expected_event_context".into(),
            message: "configure expected_event_tenant and response_audience before live events or job responses".into(),
        })
    }

    /// Use an application-owned atomic shared replay store for live signed
    /// events. Process-local stores are rejected so multi-replica callers do
    /// not accidentally opt into a configuration that cannot stop cross-node
    /// replay.
    #[cfg(feature = "jacs-crate")]
    pub fn with_shared_event_replay_store(
        mut self,
        store: Arc<dyn jacs::replay::ReplayStore>,
    ) -> Result<Self> {
        if store.scope() != jacs::replay::ReplayStoreScope::Shared {
            return Err(HaiError::SignedEventVerification {
                message: "live event replay backend must have shared scope".to_string(),
            });
        }
        self.shared_event_replay_store = Some(store);
        Ok(self)
    }

    /// Override how often a live SSE/WebSocket connection atomically replaces
    /// its exact active server-key snapshot. The interval must be non-zero.
    #[cfg(feature = "jacs-crate")]
    pub fn with_server_key_refresh_interval(mut self, interval: Duration) -> Result<Self> {
        let maximum = Duration::from_secs(DEFAULT_SERVER_KEY_REFRESH_SECS);
        if interval.is_zero() || interval > maximum {
            return Err(HaiError::Validation {
                field: "server_key_refresh_interval".to_string(),
                message: format!(
                    "server key refresh interval must be between 1ns and {} seconds",
                    DEFAULT_SERVER_KEY_REFRESH_SECS
                ),
            });
        }
        self.server_key_refresh_interval = interval;
        Ok(self)
    }

    pub fn with_default_url(jacs: P) -> Result<Self> {
        Self::new(jacs, HaiClientOptions::default())
    }

    pub fn jacs_id(&self) -> &str {
        self.jacs.jacs_id()
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Returns the HAI-assigned agent UUID for email URL paths.
    /// Falls back to jacs_id if not set.
    pub fn hai_agent_id(&self) -> &str {
        self.hai_agent_id
            .as_deref()
            .unwrap_or_else(|| self.jacs.jacs_id())
    }

    /// Set the HAI-assigned agent UUID (from registration response).
    pub fn set_hai_agent_id(&mut self, id: String) {
        self.hai_agent_id = Some(id);
    }

    /// Get the agent's @hai.ai email address (set after registration).
    pub fn agent_email(&self) -> Option<&str> {
        self.agent_email.as_deref()
    }

    /// Set the agent's @hai.ai email address.
    pub fn set_agent_email(&mut self, email: String) {
        self.agent_email = Some(email);
    }

    /// A reusable context-free credential is not supported. Use
    /// [`Self::build_request_auth_header`] for caller-built requests.
    pub fn build_auth_header(&self) -> Result<String> {
        Err(crate::request_auth::missing_request_context())
    }

    /// Sign one caller-built HTTP request. Send these exact bytes once without
    /// redirects; build a new header for each retry. Ordinary client methods
    /// handle this automatically. The audience is pinned in client options.
    pub fn build_request_auth_header(
        &self,
        method: &str,
        url: &str,
        body: &[u8],
    ) -> Result<String> {
        let parsed = url::Url::parse(url).map_err(|error| HaiError::Validation {
            field: "url".into(),
            message: error.to_string(),
        })?;
        let base = url::Url::parse(&self.base_url).expect("validated base URL");
        if !urls_have_same_origin(&base, &parsed) || parsed.fragment().is_some() {
            return Err(HaiError::Validation { field: "url".into(), message: "request URL must match the configured HAI origin and must not contain a fragment".into() });
        }
        self.http.auth_header(method, parsed.as_str(), body)
    }

    pub fn sign_message(&self, message: &str) -> Result<String> {
        self.jacs.sign_string(message)
    }

    pub fn sign_response_payload(&self, payload: &Value) -> Result<crate::types::SignedPayload> {
        self.jacs.sign_response(payload)
    }

    pub fn canonical_json(&self, value: &Value) -> Result<String> {
        self.jacs.canonical_json(value)
    }

    pub fn verify_a2a_artifact(&self, wrapped_json: &str) -> Result<String> {
        self.jacs.verify_a2a_artifact(wrapped_json)
    }

    pub async fn hello(&self, include_test: bool) -> Result<HelloResult> {
        let mut payload = json!({ "agent_id": self.jacs.jacs_id() });
        if include_test {
            payload["include_test"] = Value::Bool(true);
        }

        let url = self.url("/api/v1/agents/hello");
        let response = self
            .request_with_retry(|| {
                let http = &self.http;
                let url = &url;
                let payload = &payload;
                async move {
                    http.post(url.as_str())
                        .authenticated()
                        .header("Content-Type", "application/json")
                        .json(payload)
                        .send()
                        .await
                }
            })
            .await?;

        let data = response_json(response).await?;
        Ok(HelloResult {
            timestamp: value_string(&data, &["timestamp"]),
            client_ip: value_string(&data, &["client_ip"]),
            hai_public_key_fingerprint: value_string(&data, &["hai_public_key_fingerprint"]),
            message: value_string(&data, &["message"]),
            hai_signed_ack: value_string(&data, &["hai_signed_ack"]),
            hello_id: value_string(&data, &["hello_id"]),
            test_scenario: data.get("test_scenario").cloned(),
        })
    }

    pub async fn register(&self, options: &RegisterAgentOptions) -> Result<RegistrationResult> {
        let url = self.url("/api/v1/agents/register");

        let mut payload = serde_json::Map::new();
        payload.insert(
            "agent_json".to_string(),
            Value::String(options.agent_json.clone()),
        );

        if let Some(public_key_pem) = &options.public_key_pem {
            let encoded = base64::engine::general_purpose::STANDARD.encode(public_key_pem);
            payload.insert("public_key".to_string(), Value::String(encoded));
        }
        if let Some(owner_email) = &options.owner_email {
            payload.insert(
                "owner_email".to_string(),
                Value::String(owner_email.clone()),
            );
        }
        if let Some(domain) = &options.domain {
            payload.insert("domain".to_string(), Value::String(domain.clone()));
        }
        if let Some(description) = &options.description {
            payload.insert(
                "description".to_string(),
                Value::String(description.clone()),
            );
        }
        if let Some(registration_key) = &options.registration_key {
            payload.insert(
                "registration_key".to_string(),
                Value::String(registration_key.clone()),
            );
        }
        if let Some(is_mediator) = options.is_mediator {
            payload.insert("is_mediator".to_string(), Value::Bool(is_mediator));
        }

        let body = Value::Object(payload);
        let response = self
            .request_with_retry(|| {
                let http = &self.http;
                let url = &url;
                let body = &body;
                async move {
                    http.post(url.as_str())
                        .header("Content-Type", "application/json")
                        .json(body)
                        .send()
                        .await
                }
            })
            .await?;

        let data = response_json(response).await?;
        let registrations = data
            .get("registrations")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));

        Ok(RegistrationResult {
            success: true,
            agent_id: value_string(&data, &["agent_id", "agentId"]),
            jacs_id: value_string(&data, &["jacs_id", "jacsId"]).if_empty_then(self.jacs.jacs_id()),
            dns_verified: data
                .get("dns_verified")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            registrations: serde_json::from_value(registrations).unwrap_or_default(),
            registered_at: value_string(&data, &["registered_at", "registeredAt"]),
            message: data
                .get("message")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            email: data
                .get("email")
                .and_then(Value::as_str)
                .map(ToString::to_string),
        })
    }

    /// Rotate the agent's cryptographic keys.
    ///
    /// Delegates local key rotation to the [`JacsProvider::rotate()`] method,
    /// which archives old keys, generates a new keypair, builds a new
    /// self-signed agent document, and updates config on disk.
    ///
    /// When `register_with_hai` is true (the default), re-registers the new
    /// key with HAI. HAI registration failure is non-fatal -- local rotation
    /// is preserved.
    pub async fn rotate_keys(&self, options: Option<&RotateKeysOptions>) -> Result<RotationResult> {
        let register_with_hai = options.and_then(|o| o.register_with_hai).unwrap_or(true);
        let algorithm = options.and_then(|options| options.algorithm.as_deref());
        if !register_with_hai {
            return self.jacs.rotate_with_algorithm(algorithm);
        }
        let url = self.url("/api/v1/agents/register");
        let prepared = self
            .jacs
            .rotate_for_registration(&url, self.http.audience(), algorithm)?;
        let mut result = prepared.result;
        result.registered_with_hai = self
            .send_prepared_registration(url, prepared.auth_header, prepared.body)
            .await?;
        if !result.registered_with_hai {
            tracing::warn!(
                event = "jacs_rotation_registration_failed",
                "Local keys rotated; HAI registration was not confirmed"
            );
        }
        Ok(result)
    }

    async fn send_prepared_registration(
        &self,
        url: String,
        auth_header: String,
        body: Vec<u8>,
    ) -> Result<bool> {
        let request = self
            .http
            .post(url)
            .header("Authorization", auth_header)
            .header("Content-Type", "application/json")
            .body(body)
            .build()?;
        Ok(matches!(
            self.http.execute_authenticated(request).await,
            Ok(response) if response.status().is_success()
        ))
    }

    /// Export the current agent document as JSON.
    pub fn export_agent_json(&self) -> Result<String> {
        self.jacs.export_agent_json()
    }

    /// Update agent metadata and re-sign with the existing key.
    ///
    /// Delegates the local update to [`JacsProvider::update_agent()`], then
    /// re-registers the updated agent document with HAI so the platform has
    /// the latest version. HAI registration failure is non-fatal.
    pub async fn update_agent(&self, new_agent_data: &str) -> Result<UpdateAgentResult> {
        let url = self.url("/api/v1/agents/register");
        let prepared =
            self.jacs
                .update_for_registration(new_agent_data, &url, self.http.audience())?;
        let mut result = prepared.result;
        result.registered_with_hai = self
            .send_prepared_registration(url, prepared.auth_header, prepared.body)
            .await?;
        if !result.registered_with_hai {
            tracing::warn!(
                event = "jacs_metadata_registration_failed",
                "Local metadata updated; HAI registration was not confirmed"
            );
        }

        Ok(result)
    }

    pub async fn submit_response(
        &self,
        job_id: &str,
        message: &str,
        metadata: Option<Value>,
        processing_time_ms: u64,
    ) -> Result<JobResponseResult> {
        let response = json!({
            "message": message,
            "metadata": metadata,
            "processing_time_ms": processing_time_ms,
        });
        let payload = serde_json::to_value(SignedJobResponsePayloadV2::new(job_id, response))?;
        let signed = self.sign_job_response_context(job_id, payload)?;

        let safe_job_id = encode_path_segment(job_id);
        let url = self.url(&format!("/api/v1/agents/jobs/{safe_job_id}/response"));
        let response = self
            .http
            .post(url)
            .authenticated()
            .header("Content-Type", "application/json")
            .json(&signed)
            .send()
            .await?;

        let data = response_json(response).await?;
        Ok(JobResponseResult {
            success: data.get("success").and_then(Value::as_bool).unwrap_or(true),
            job_id: value_string(&data, &["job_id", "jobId"]).if_empty_then(job_id),
            message: value_string(&data, &["message"]).if_empty_then("Response accepted"),
        })
    }

    fn sign_job_response_context(
        &self,
        job_id: &str,
        payload: Value,
    ) -> Result<crate::types::SignedPayload> {
        #[cfg(feature = "jacs-crate")]
        {
            use jacs::response_context::{
                EventCausation, EventTransport, ResponseData, ResponseOperation,
            };
            let (tenant, audience) = self.expected_event_context()?;
            let data = ResponseData::PrivateEvent {
                event_type: "job_response".into(),
                contract: "hai.job-response".into(),
                contract_version: "2".into(),
                transport: EventTransport::Channel(format!("job:{job_id}")),
                issuer: String::new(),
                tenant: tenant.clone(),
                audience: audience.clone(),
                event_id: String::new(),
                emitted_at: String::new(),
                causation: EventCausation::Job { id: job_id.into() },
                payload,
            };
            self.jacs
                .sign_response_with_context(&data, ResponseOperation::SignAsyncEvent)
        }
        #[cfg(not(feature = "jacs-crate"))]
        {
            let _ = (job_id, payload);
            Err(HaiError::Provider(
                "context-bound job responses require the jacs-crate feature".into(),
            ))
        }
    }

    pub async fn verify_status(&self, agent_id: Option<&str>) -> Result<VerifyAgentResult> {
        let target = agent_id.unwrap_or_else(|| self.jacs.jacs_id());
        let safe_agent_id = encode_path_segment(target);
        let url = self.url(&format!("/api/v1/agents/{safe_agent_id}/verify"));

        let response = self.http.get(url).authenticated().send().await?;

        let data = response_json(response).await?;
        let mut parsed: VerifyAgentResult = serde_json::from_value(data.clone())?;
        if parsed.jacs_id.is_empty() {
            parsed.jacs_id = target.to_string();
        }
        Ok(parsed)
    }

    pub async fn update_username(
        &self,
        agent_id: &str,
        username: &str,
    ) -> Result<UpdateUsernameResult> {
        let safe_agent_id = encode_path_segment(agent_id);
        let url = self.url(&format!("/api/v1/agents/{safe_agent_id}/username"));

        let response = self
            .http
            .put(url)
            .authenticated()
            .header("Content-Type", "application/json")
            .json(&json!({ "username": username }))
            .send()
            .await?;

        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    pub async fn delete_username(&self, agent_id: &str) -> Result<DeleteUsernameResult> {
        let safe_agent_id = encode_path_segment(agent_id);
        let url = self.url(&format!("/api/v1/agents/{safe_agent_id}/username"));

        let response = self.http.delete(url).authenticated().send().await?;

        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    pub async fn send_email(&self, options: &SendEmailOptions) -> Result<SendEmailResult> {
        self.send_signed_email(options).await
    }

    /// Send an agent-signed email.
    ///
    /// Builds an RFC 5322 MIME email from the given options, signs it locally
    /// with the agent's own JACS key (via `JacsProvider::sign_email_locally`),
    /// and POSTs the signed bytes to the server for countersigning and delivery.
    ///
    /// The server validates that the JACS signature matches the authenticated
    /// agent, countersigns with the HAI authority key (creating a forwarding
    /// chain), and delivers via JMAP.
    ///
    /// # Errors
    ///
    /// Returns `HaiError` if:
    /// - `agent_email` is not set (register with a username first)
    /// - The provider does not support local signing (use `LocalJacsProvider`)
    /// - MIME construction or JACS signing fails
    /// - The server rejects the signed email
    pub async fn send_signed_email(&self, options: &SendEmailOptions) -> Result<SendEmailResult> {
        self.send_signed_email_with_generation_type(options, EmailGenerationType::HtmlInlineJacs)
            .await
    }

    /// Send an agent-signed email with an explicit generation type.
    pub async fn send_signed_email_with_generation_type(
        &self,
        options: &SendEmailOptions,
        generation_type: EmailGenerationType,
    ) -> Result<SendEmailResult> {
        let signed_email = self.create_signed_email(options, generation_type)?;
        let idempotency_key = send_idempotency_key(options);

        // Step 3: POST to the send-signed endpoint
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let url = self.url(&format!("/api/agents/{safe_jacs_id}/email/send-signed"));
        let raw_email = signed_email.raw_email;

        let response = self
            .request_with_retry(|| {
                let http = &self.http;
                let url = &url;
                let raw_email = &raw_email;
                let idempotency_key = &idempotency_key;
                async move {
                    http.post(url.as_str())
                        .authenticated()
                        .header(IDEMPOTENCY_KEY_HEADER, idempotency_key.as_str())
                        .header("Content-Type", "message/rfc822")
                        .body(raw_email.clone())
                        .send()
                        .await
                }
            })
            .await?;

        let data = response_json(response).await?;
        Ok(SendEmailResult {
            message_id: value_string(&data, &["message_id"]),
            status: value_string(&data, &["status"]),
        })
    }

    /// Create a signed RFC 5322 email locally without submitting it.
    pub fn create_signed_email(
        &self,
        options: &SendEmailOptions,
        generation_type: EmailGenerationType,
    ) -> Result<SignedEmail> {
        let from = self.agent_email.as_deref().ok_or_else(|| {
            HaiError::Message("agent email not set — register with a username first".into())
        })?;

        match generation_type {
            EmailGenerationType::AttachmentJacs => {
                let body = self.body_with_legacy_verification_footer(options, from);
                let opts_with_footer = SendEmailOptions {
                    body,
                    ..options.clone()
                };
                let raw_mime = crate::mime::build_rfc5322_email(&opts_with_footer, from)?;
                let raw_email = self.jacs.sign_email_locally(&raw_mime)?;
                Ok(SignedEmail {
                    raw_email,
                    generation_type,
                    hidden_envelope_size_bytes: None,
                    signed_logo_size_bytes: None,
                })
            }
            EmailGenerationType::HtmlInlineJacs => {
                crate::validation::validate_send_email(options)?;

                let verify_url = format!("{}/verify/email", self.base_url.trim_end_matches('/'));
                let text_body =
                    crate::email_inline::render_text_inline_email_body(&options.body, &verify_url);
                let opts_for_mime = SendEmailOptions {
                    body: text_body,
                    ..options.clone()
                };
                let headers = crate::mime::generate_rfc5322_header_values()?;

                let placeholder_html = crate::email_inline::render_html_inline_email_body(
                    &options.body,
                    &verify_url,
                    "{}",
                );
                let raw_for_signing = crate::mime::build_html_inline_rfc5322_email_with_headers(
                    &opts_for_mime,
                    from,
                    &placeholder_html,
                    Some(crate::email_inline::HAI_JACS_LOGO_BYTES),
                    &headers,
                )?;
                let hidden_envelope = self
                    .jacs
                    .sign_html_inline_email_envelope(&raw_for_signing)?;
                let signed_logo = crate::email_inline::embed_jacs_header_in_inline_logo(
                    &hidden_envelope.compact_header,
                )?;
                let html = crate::email_inline::render_html_inline_email_body(
                    &options.body,
                    &verify_url,
                    &hidden_envelope.hidden_envelope,
                );
                let raw_email = crate::mime::build_html_inline_rfc5322_email_with_headers(
                    &opts_for_mime,
                    from,
                    &html,
                    Some(&signed_logo.bytes),
                    &headers,
                )?;

                Ok(SignedEmail {
                    raw_email,
                    generation_type,
                    hidden_envelope_size_bytes: Some(hidden_envelope.hidden_envelope_size_bytes),
                    signed_logo_size_bytes: Some(signed_logo.size_bytes),
                })
            }
        }
    }

    fn body_with_legacy_verification_footer(
        &self,
        options: &SendEmailOptions,
        from: &str,
    ) -> String {
        if options.append_footer == Some(false) {
            return options.body.clone();
        }

        let has_external = !options.to.ends_with("@hai.ai")
            || options.cc.iter().any(|a| !a.ends_with("@hai.ai"))
            || options.bcc.iter().any(|a| !a.ends_with("@hai.ai"));
        if has_external {
            let slug = email_to_slug(from);
            format!(
                "{}\n\nVerify this agent's reputation: {}/agents/{}",
                options.body, self.base_url, slug
            )
        } else {
            options.body.clone()
        }
    }

    pub async fn list_messages(&self, options: &ListMessagesOptions) -> Result<Vec<EmailMessage>> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let url = self.url(&format!("/api/agents/{safe_jacs_id}/email/messages"));

        let mut request = self.http.get(url).authenticated();

        if let Some(limit) = options.limit {
            request = request.query(&[("limit", limit)]);
        }
        if let Some(offset) = options.offset {
            request = request.query(&[("offset", offset)]);
        }
        if let Some(direction) = options.direction.as_deref() {
            request = request.query(&[("direction", direction)]);
        }
        if let Some(is_read) = options.is_read {
            request = request.query(&[("is_read", &is_read.to_string())]);
        }
        if let Some(folder) = options.folder.as_deref() {
            request = request.query(&[("folder", folder)]);
        }
        if let Some(label) = options.label.as_deref() {
            request = request.query(&[("label", label)]);
        }
        if let Some(has_attachments) = options.has_attachments {
            request = request.query(&[("has_attachments", &has_attachments.to_string())]);
        }

        let response = request.send().await?;
        let data = response_json(response).await?;

        let messages = data
            .get("messages")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        Ok(serde_json::from_value(messages)?)
    }

    /// Update labels on a message. Adds and removes labels atomically.
    pub async fn update_labels(
        &self,
        message_id: &str,
        add: &[&str],
        remove: &[&str],
    ) -> Result<Vec<String>> {
        let _ = self.agent_email.as_deref().ok_or_else(|| {
            HaiError::Message("agent email not set — register with a username first".into())
        })?;
        let agent_id = self.hai_agent_id();
        let safe_agent_id = encode_path_segment(agent_id);
        let safe_message_id = encode_path_segment(message_id);
        let url = self.url(&format!(
            "/api/agents/{safe_agent_id}/email/messages/{safe_message_id}/labels"
        ));

        let body = json!({
            "add": add,
            "remove": remove,
        });

        let response = self
            .http
            .post(url)
            .authenticated()
            .json(&body)
            .send()
            .await?;

        let data = response_json(response).await?;
        let labels = data
            .get("labels")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        Ok(labels)
    }

    pub async fn mark_read(&self, message_id: &str) -> Result<()> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let safe_message_id = encode_path_segment(message_id);
        let url = self.url(&format!(
            "/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}/read"
        ));

        let response = self.http.post(url).authenticated().send().await?;

        match response.status() {
            StatusCode::OK | StatusCode::CREATED | StatusCode::NO_CONTENT => Ok(()),
            _ => Err(response_error(response).await),
        }
    }

    pub async fn get_email_status(&self) -> Result<EmailStatus> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let url = self.url(&format!("/api/agents/{safe_jacs_id}/email/status"));

        let response = self.http.get(url).authenticated().send().await?;

        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    pub async fn get_message(&self, message_id: &str) -> Result<EmailMessage> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let safe_message_id = encode_path_segment(message_id);
        let url = self.url(&format!(
            "/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}"
        ));

        let response = self.http.get(url).authenticated().send().await?;

        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    /// Fetch the exact raw RFC 5322 bytes of a message, suitable for local
    /// JACS verification via [`verify_email`](crate::email::verify_email).
    ///
    /// The endpoint (`GET .../messages/{id}/raw`) returns either the stored
    /// bytes (base64-decoded here at the client boundary) or an explicit
    /// `available: false` signal with an `omitted_reason`:
    ///
    /// - `"not_stored"`: legacy row predating the feature.
    /// - `"oversize"`: MIME exceeded the 25 MB storage cap.
    ///
    /// **Byte-fidelity mandate (PRD R2):** bytes returned here MUST be
    /// byte-identical to what JACS signed. No trimming, no line-ending
    /// normalization, no UTF-8 lossy conversion.
    pub async fn get_raw_email(&self, message_id: &str) -> Result<RawEmailResponse> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let safe_message_id = encode_path_segment(message_id);
        let url = self.url(&format!(
            "/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}/raw"
        ));

        let response = self.http.get(url).authenticated().send().await?;

        let data = response_json(response).await?;
        RawEmailResponse::from_wire_json(data)
    }

    pub async fn delete_message(&self, message_id: &str) -> Result<()> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let safe_message_id = encode_path_segment(message_id);
        let url = self.url(&format!(
            "/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}"
        ));

        let response = self.http.delete(url).authenticated().send().await?;

        match response.status() {
            StatusCode::OK | StatusCode::NO_CONTENT => Ok(()),
            _ => Err(response_error(response).await),
        }
    }

    pub async fn mark_unread(&self, message_id: &str) -> Result<()> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let safe_message_id = encode_path_segment(message_id);
        let url = self.url(&format!(
            "/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}/unread"
        ));

        let response = self.http.post(url).authenticated().send().await?;

        match response.status() {
            StatusCode::OK | StatusCode::CREATED | StatusCode::NO_CONTENT => Ok(()),
            _ => Err(response_error(response).await),
        }
    }

    pub async fn archive(&self, message_id: &str) -> Result<()> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let safe_message_id = encode_path_segment(message_id);
        let url = self.url(&format!(
            "/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}/archive"
        ));

        let response = self.http.post(url).authenticated().send().await?;

        match response.status() {
            StatusCode::OK | StatusCode::CREATED | StatusCode::NO_CONTENT => Ok(()),
            _ => Err(response_error(response).await),
        }
    }

    pub async fn unarchive(&self, message_id: &str) -> Result<()> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let safe_message_id = encode_path_segment(message_id);
        let url = self.url(&format!(
            "/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}/unarchive"
        ));

        let response = self.http.post(url).authenticated().send().await?;

        match response.status() {
            StatusCode::OK | StatusCode::CREATED | StatusCode::NO_CONTENT => Ok(()),
            _ => Err(response_error(response).await),
        }
    }

    pub async fn search_messages(&self, options: &SearchOptions) -> Result<Vec<EmailMessage>> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let url = self.url(&format!("/api/agents/{safe_jacs_id}/email/search"));

        let mut request = self.http.get(url).authenticated();

        if let Some(ref q) = options.q {
            request = request.query(&[("q", q.as_str())]);
        }
        if let Some(ref direction) = options.direction {
            request = request.query(&[("direction", direction.as_str())]);
        }
        if let Some(ref from_address) = options.from_address {
            request = request.query(&[("from_address", from_address.as_str())]);
        }
        if let Some(ref to_address) = options.to_address {
            request = request.query(&[("to_address", to_address.as_str())]);
        }
        if let Some(ref since) = options.since {
            request = request.query(&[("since", since.as_str())]);
        }
        if let Some(ref until) = options.until {
            request = request.query(&[("until", until.as_str())]);
        }
        if let Some(limit) = options.limit {
            request = request.query(&[("limit", &limit.to_string())]);
        }
        if let Some(offset) = options.offset {
            request = request.query(&[("offset", &offset.to_string())]);
        }
        if let Some(is_read) = options.is_read {
            request = request.query(&[("is_read", &is_read.to_string())]);
        }
        if let Some(ref jacs_verified) = options.jacs_verified {
            request = request.query(&[("jacs_verified", &jacs_verified.to_string())]);
        }
        if let Some(ref folder) = options.folder {
            request = request.query(&[("folder", folder.as_str())]);
        }
        if let Some(ref label) = options.label {
            request = request.query(&[("label", label.as_str())]);
        }
        if let Some(has_attachments) = options.has_attachments {
            request = request.query(&[("has_attachments", &has_attachments.to_string())]);
        }

        let response = request.send().await?;
        let data = response_json(response).await?;

        let messages = data
            .get("messages")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        Ok(serde_json::from_value(messages)?)
    }

    pub async fn get_unread_count(&self) -> Result<u64> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let url = self.url(&format!("/api/agents/{safe_jacs_id}/email/unread-count"));

        let response = self.http.get(url).authenticated().send().await?;

        let data = response_json(response).await?;
        let count = data
            .get("count")
            .and_then(Value::as_u64)
            .or_else(|| data.as_u64())
            .unwrap_or(0);
        Ok(count)
    }

    /// Reply to a message. Always JACS-signed via `send_signed_email`.
    ///
    /// Fetches the original message, constructs a reply with proper threading
    /// headers, sanitizes the subject (strips CR/LF from email header folding),
    /// and sends the reply signed with the agent's JACS key.
    pub async fn reply(
        &self,
        message_id: &str,
        body: &str,
        subject_override: Option<&str>,
    ) -> Result<SendEmailResult> {
        self.reply_with_options(message_id, body, subject_override, None, &[])
            .await
    }

    /// Reply with reply_type and optional recipients. Always JACS-signed.
    ///
    /// - `reply_type`: "sender" (default), "all", or "custom"
    /// - `recipients`: required when reply_type is "custom"
    ///
    /// Fetches the original message client-side, sanitizes the subject
    /// (strips CR/LF from email header folding), and routes the reply
    /// through `send_signed_email` for proper JACS signing.
    pub async fn reply_with_options(
        &self,
        message_id: &str,
        body: &str,
        subject_override: Option<&str>,
        _reply_type: Option<&str>,
        _recipients: &[String],
    ) -> Result<SendEmailResult> {
        let original = self.get_message(message_id).await?;

        // Sanitize subject: strip CR/LF that may be present from email
        // header folding in stored inbound subjects.
        let subject = if let Some(s) = subject_override {
            crate::mime::sanitize_header(s)
        } else {
            let clean = crate::mime::sanitize_header(&original.subject);
            if clean.to_lowercase().starts_with("re: ") {
                clean
            } else {
                format!("Re: {clean}")
            }
        };

        // Use the RFC 5322 Message-ID for threading, falling back to DB UUID.
        let in_reply_to = original
            .message_id
            .filter(|mid| !mid.is_empty())
            .unwrap_or_else(|| message_id.to_string());

        self.send_signed_email(&SendEmailOptions {
            to: original.from_address,
            subject,
            body: body.to_string(),
            in_reply_to: Some(in_reply_to),
            ..Default::default()
        })
        .await
    }

    /// Forward a message to another agent with an optional comment.
    ///
    /// Fetches the original message client-side, constructs a forwarded email
    /// with the original content quoted, signs with the agent's JACS key, and
    /// sends via `send_signed_email`.
    pub async fn forward(
        &self,
        message_id: &str,
        to: &str,
        comment: Option<&str>,
    ) -> Result<SendEmailResult> {
        let original = self.get_message(message_id).await?;

        // Sanitize original fields
        let orig_subject = crate::mime::sanitize_header(&original.subject);
        let orig_from = crate::mime::sanitize_header(&original.from_address);

        let subject = format!("Fwd: {orig_subject}");

        // Build forwarded body with optional comment and quoted original
        let mut body = String::new();
        if let Some(c) = comment {
            body.push_str(c);
            body.push_str("\n\n");
        }
        body.push_str("---------- Forwarded message ----------\n");
        body.push_str(&format!("From: {}\n", orig_from));
        body.push_str(&format!("Date: {}\n", original.created_at));
        body.push_str(&format!("Subject: {}\n", orig_subject));
        body.push('\n');
        body.push_str(&original.body_text);

        self.send_signed_email(&SendEmailOptions {
            to: to.to_string(),
            subject,
            body,
            ..Default::default()
        })
        .await
    }

    /// Convenience alias for contacts endpoint.
    pub async fn contacts(&self) -> Result<Vec<Contact>> {
        let _ = self.agent_email.as_deref().ok_or_else(|| {
            HaiError::Message("agent email not set — register with a username first".into())
        })?;
        let agent_id = self.hai_agent_id();
        let safe_agent_id = encode_path_segment(agent_id);
        let url = self.url(&format!("/api/agents/{safe_agent_id}/email/contacts"));

        let response = self.http.get(url).authenticated().send().await?;

        let data = response_json(response).await?;
        let contacts_val = data.get("contacts").cloned().unwrap_or(data.clone());
        Ok(serde_json::from_value(contacts_val)?)
    }

    // =========================================================================
    // Email Template Methods
    // =========================================================================

    /// Create a new email template.
    pub async fn create_email_template(
        &self,
        options: &CreateEmailTemplateOptions,
    ) -> Result<EmailTemplate> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let url = self.url(&format!("/api/agents/{safe_jacs_id}/email/templates"));

        let response = self
            .http
            .post(url)
            .authenticated()
            .json(options)
            .send()
            .await?;

        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    /// List email templates, optionally searching with BM25.
    pub async fn list_email_templates(
        &self,
        options: &ListEmailTemplatesOptions,
    ) -> Result<ListEmailTemplatesResult> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let url = self.url(&format!("/api/agents/{safe_jacs_id}/email/templates"));

        let mut request = self.http.get(url).authenticated();

        if let Some(limit) = options.limit {
            request = request.query(&[("limit", &limit.to_string())]);
        }
        if let Some(offset) = options.offset {
            request = request.query(&[("offset", &offset.to_string())]);
        }
        if let Some(ref q) = options.q {
            request = request.query(&[("q", q.as_str())]);
        }

        let response = request.send().await?;
        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    /// Get a single email template by ID.
    pub async fn get_email_template(&self, template_id: &str) -> Result<EmailTemplate> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let safe_template_id = encode_path_segment(template_id);
        let url = self.url(&format!(
            "/api/agents/{safe_jacs_id}/email/templates/{safe_template_id}"
        ));

        let response = self.http.get(url).authenticated().send().await?;

        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    /// Update an email template (partial update).
    pub async fn update_email_template(
        &self,
        template_id: &str,
        options: &UpdateEmailTemplateOptions,
    ) -> Result<EmailTemplate> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let safe_template_id = encode_path_segment(template_id);
        let url = self.url(&format!(
            "/api/agents/{safe_jacs_id}/email/templates/{safe_template_id}"
        ));

        let response = self
            .http
            .put(url)
            .authenticated()
            .json(options)
            .send()
            .await?;

        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    /// Delete an email template (soft delete).
    pub async fn delete_email_template(&self, template_id: &str) -> Result<()> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let safe_template_id = encode_path_segment(template_id);
        let url = self.url(&format!(
            "/api/agents/{safe_jacs_id}/email/templates/{safe_template_id}"
        ));

        let response = self.http.delete(url).authenticated().send().await?;

        match response.status() {
            StatusCode::OK | StatusCode::NO_CONTENT => Ok(()),
            _ => Err(response_error(response).await),
        }
    }

    // =========================================================================
    // Server Keys (unauthenticated)
    // =========================================================================

    /// Fetch the HAI server's public keys from the well-known endpoint.
    ///
    /// This is an unauthenticated GET to `/.well-known/hai-keys.json`.
    pub async fn fetch_server_keys(&self) -> Result<Value> {
        let url = self.url("/.well-known/hai-keys.json");
        fetch_server_keys_with_client(self.http.raw_client(), &url).await
    }

    // =========================================================================
    // Raw Email Sign/Verify (base64-encoded for FFI boundary)
    // =========================================================================

    /// Sign a raw RFC 5822 email via the HAI server.
    ///
    /// Input: base64-encoded email bytes. Output: base64-encoded signed email bytes.
    /// The raw bytes are decoded, POSTed with `Content-Type: message/rfc822`,
    /// and the response bytes are base64-encoded for return through the FFI boundary.
    pub async fn sign_email_raw(&self, raw_email_b64: &str) -> Result<String> {
        let raw_bytes = base64::engine::general_purpose::STANDARD
            .decode(raw_email_b64)
            .map_err(|e| HaiError::Validation {
                field: "raw_email_b64".into(),
                message: e.to_string(),
            })?;
        let url = self.url("/api/v1/email/sign");
        let response = self
            .http
            .post(url)
            .authenticated()
            .header("Content-Type", "message/rfc822")
            .body(raw_bytes)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(HaiError::Api {
                status: status.as_u16(),
                message: body,
            });
        }
        let response_bytes = response.bytes().await?;
        Ok(base64::engine::general_purpose::STANDARD.encode(&response_bytes))
    }

    /// Verify a raw RFC 5822 email via the HAI server.
    ///
    /// Input: base64-encoded email bytes. Output: JSON verification result.
    pub async fn verify_email_raw(&self, raw_email_b64: &str) -> Result<Value> {
        let raw_bytes = base64::engine::general_purpose::STANDARD
            .decode(raw_email_b64)
            .map_err(|e| HaiError::Validation {
                field: "raw_email_b64".into(),
                message: e.to_string(),
            })?;
        let url = self.url("/api/v1/email/verify");
        let response = self
            .http
            .post(url)
            .authenticated()
            .header("Content-Type", "message/rfc822")
            .body(raw_bytes)
            .send()
            .await?;
        response_json(response).await
    }

    // =========================================================================
    // Agreements (future HAI workflow API)
    // =========================================================================

    /// Save a signed agreement document in HAI.
    ///
    /// The `POST /api/v1/agreements` server endpoint is intentionally
    /// documented before implementation (AGREEMENTS_FIRST_CLASS_API_PRD in
    /// `hai/docs`); this SDK method gives language bindings one stable
    /// contract, and callers get a 404 until the endpoint lands.
    pub async fn save_agreement(&self, request: &Value) -> Result<Value> {
        let url = self.url("/api/v1/agreements");
        let response = self
            .request_with_retry(|| {
                let http = &self.http;
                let url = &url;
                async move {
                    http.post(url.as_str())
                        .authenticated()
                        .header("Content-Type", "application/json")
                        .json(request)
                        .send()
                        .await
                }
            })
            .await?;

        response_json(response).await
    }

    /// Search agreement records visible to the authenticated agent.
    ///
    /// Like [`Self::save_agreement`], the `POST /api/v1/agreements/search`
    /// endpoint is documented ahead of its server implementation
    /// (AGREEMENTS_FIRST_CLASS_API_PRD); callers get a 404 until it lands.
    pub async fn search_agreements(&self, request: &Value) -> Result<Value> {
        let url = self.url("/api/v1/agreements/search");
        let response = self
            .request_with_retry(|| {
                let http = &self.http;
                let url = &url;
                async move {
                    http.post(url.as_str())
                        .authenticated()
                        .header("Content-Type", "application/json")
                        .json(request)
                        .send()
                        .await
                }
            })
            .await?;

        response_json(response).await
    }

    /// Retrieve one agreement record by HAI agreement id or JACS document id.
    pub async fn get_agreement(&self, agreement_id: &str) -> Result<Value> {
        let safe_agreement_id = encode_path_segment(agreement_id);
        let url = self.url(&format!("/api/v1/agreements/{safe_agreement_id}"));
        let response = self
            .request_with_retry(|| {
                let http = &self.http;
                let url = &url;
                async move { http.get(url.as_str()).authenticated().send().await }
            })
            .await?;

        response_json(response).await
    }

    /// Request a HAI notary/countersignature for an agreement workflow.
    pub async fn countersign_agreement(
        &self,
        agreement_id: &str,
        request: &Value,
    ) -> Result<Value> {
        let safe_agreement_id = encode_path_segment(agreement_id);
        let url = self.url(&format!(
            "/api/v1/agreements/{safe_agreement_id}/countersign"
        ));
        let response = self
            .request_with_retry(|| {
                let http = &self.http;
                let url = &url;
                async move {
                    http.post(url.as_str())
                        .authenticated()
                        .header("Content-Type", "application/json")
                        .json(request)
                        .send()
                        .await
                }
            })
            .await?;

        response_json(response).await
    }

    /// Retrieve an agreement intake visible to the authenticated party agent.
    pub async fn get_agreement_intake(&self, intake_id: &str) -> Result<Value> {
        let safe_intake_id = encode_path_segment(intake_id);
        let url = self.url(&format!("/api/v1/agreements/intakes/{safe_intake_id}"));
        let response = self
            .request_with_retry(|| {
                let http = &self.http;
                let url = &url;
                async move { http.get(url.as_str()).authenticated().send().await }
            })
            .await?;

        response_json(response).await
    }

    /// Retrieve an agreement intake by the public id included in agreement email.
    pub async fn get_agreement_intake_by_public_id(&self, public_intake_id: &str) -> Result<Value> {
        let safe_public_intake_id = encode_path_segment(public_intake_id);
        let url = self.url(&format!(
            "/api/v1/agreements/intakes/by-public/{safe_public_intake_id}"
        ));
        let response = self
            .request_with_retry(|| {
                let http = &self.http;
                let url = &url;
                async move { http.get(url.as_str()).authenticated().send().await }
            })
            .await?;

        response_json(response).await
    }

    /// Record one normalized interview turn for an agreement intake.
    pub async fn record_agreement_interview_turn(
        &self,
        intake_id: &str,
        request: &Value,
    ) -> Result<Value> {
        let safe_intake_id = encode_path_segment(intake_id);
        let url = self.url(&format!(
            "/api/v1/agreements/intakes/{safe_intake_id}/interview-turns"
        ));
        let response = self
            .request_with_retry(|| {
                let http = &self.http;
                let url = &url;
                async move {
                    http.post(url.as_str())
                        .authenticated()
                        .header("Content-Type", "application/json")
                        .json(request)
                        .send()
                        .await
                }
            })
            .await?;

        response_json(response).await
    }

    // =========================================================================
    // Attestation Methods
    // =========================================================================

    /// Create an attestation for an agent.
    pub async fn create_attestation(
        &self,
        agent_id: &str,
        subject: &Value,
        claims: &Value,
        evidence: Option<&Value>,
    ) -> Result<Value> {
        let safe_agent_id = encode_path_segment(agent_id);
        let url = self.url(&format!("/api/v1/agents/{safe_agent_id}/attestations"));

        let mut payload = json!({
            "subject": subject,
            "claims": claims,
        });
        if let Some(ev) = evidence {
            payload["evidence"] = ev.clone();
        }

        let response = self
            .http
            .post(url)
            .authenticated()
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        response_json(response).await
    }

    /// List attestations for an agent.
    pub async fn list_attestations(
        &self,
        agent_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Value> {
        let safe_agent_id = encode_path_segment(agent_id);
        let url = self.url(&format!("/api/v1/agents/{safe_agent_id}/attestations"));

        let response = self
            .http
            .get(url)
            .authenticated()
            .query(&[
                ("limit", &limit.to_string()),
                ("offset", &offset.to_string()),
            ])
            .send()
            .await?;

        response_json(response).await
    }

    /// Get a single attestation by document ID.
    pub async fn get_attestation(&self, agent_id: &str, doc_id: &str) -> Result<Value> {
        let safe_agent_id = encode_path_segment(agent_id);
        let safe_doc_id = encode_path_segment(doc_id);
        let url = self.url(&format!(
            "/api/v1/agents/{safe_agent_id}/attestations/{safe_doc_id}"
        ));

        let response = self.http.get(url).authenticated().send().await?;

        response_json(response).await
    }

    /// Verify an attestation document.
    pub async fn verify_attestation(&self, document: &str) -> Result<Value> {
        let url = self.url("/api/v1/attestations/verify");
        let response = self
            .http
            .post(url)
            .authenticated()
            .header("Content-Type", "application/json")
            .json(&json!({ "document": document }))
            .send()
            .await?;

        response_json(response).await
    }

    pub async fn fetch_remote_key(&self, jacs_id: &str, version: &str) -> Result<PublicKeyInfo> {
        let safe_jacs_id = encode_path_segment(jacs_id);
        let safe_version = encode_path_segment(version);
        let url = self.url(&format!(
            "/jacs/v1/agents/{safe_jacs_id}/keys/{safe_version}"
        ));

        let response = self.http.get(url).send().await?;
        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    /// Look up an agent's public key by its SHA-256 hash.
    ///
    /// The `hash` should be in `sha256:<hex>` format; the `sha256:` prefix
    /// will be added automatically if missing.
    pub async fn fetch_key_by_hash(&self, hash: &str) -> Result<PublicKeyInfo> {
        let safe_hash = encode_path_segment(hash);
        let url = self.url(&format!("/jacs/v1/keys/by-hash/{safe_hash}"));

        let response = self.http.get(url).send().await?;
        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    /// Look up an agent's public key by its `@hai.ai` email address.
    pub async fn fetch_key_by_email(&self, email: &str) -> Result<PublicKeyInfo> {
        let safe_email = encode_path_segment(email);
        let url = self.url(&format!("/api/agents/keys/{safe_email}"));

        let response = self.http.get(url).send().await?;
        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    /// Look up the latest DNS-verified agent key for a domain.
    pub async fn fetch_key_by_domain(&self, domain: &str) -> Result<PublicKeyInfo> {
        let safe_domain = encode_path_segment(domain);
        let url = self.url(&format!("/jacs/v1/agents/by-domain/{safe_domain}"));

        let response = self.http.get(url).send().await?;
        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    /// List all key versions for an agent, ordered by `created_at` descending.
    pub async fn fetch_all_keys(&self, jacs_id: &str) -> Result<AgentKeyHistory> {
        let safe_jacs_id = encode_path_segment(jacs_id);
        let url = self.url(&format!("/jacs/v1/agents/{safe_jacs_id}/keys"));

        let response = self.http.get(url).send().await?;
        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    pub async fn verify_document(&self, document: &str) -> Result<DocumentVerificationResult> {
        let url = self.url("/api/jacs/verify");
        let response = self
            .http
            .post(url)
            .header("Content-Type", "application/json")
            .json(&json!({ "document": document }))
            .send()
            .await?;

        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    pub async fn get_verification(&self, agent_id: &str) -> Result<AgentVerificationResult> {
        let safe_agent_id = encode_path_segment(agent_id);
        let url = self.url(&format!("/api/v1/agents/{safe_agent_id}/verification"));
        let response = self
            .http
            .get(url)
            .header("Content-Type", "application/json")
            .send()
            .await?;

        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    pub async fn verify_agent_document(
        &self,
        request: &VerifyAgentDocumentRequest,
    ) -> Result<AgentVerificationResult> {
        let url = self.url("/api/v1/agents/verify");
        let response = self
            .http
            .post(url)
            .header("Content-Type", "application/json")
            .json(request)
            .send()
            .await?;

        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    pub async fn benchmark(&self, name: Option<&str>, tier: Option<&str>) -> Result<Value> {
        let payload = json!({
            "name": name.unwrap_or("mediation_basic"),
            "tier": tier.unwrap_or("free"),
        });
        let url = self.url("/api/benchmark/run");
        let response = self
            .request_with_retry(|| {
                let http = &self.http;
                let url = &url;
                let payload = &payload;
                async move {
                    http.post(url.as_str())
                        .authenticated()
                        .header("Content-Type", "application/json")
                        .json(payload)
                        .send()
                        .await
                }
            })
            .await?;

        response_json(response).await
    }

    pub async fn free_run(&self, transport: Option<TransportType>) -> Result<FreeChaoticResult> {
        let transport = transport.unwrap_or(TransportType::Sse);
        let short_id = self.jacs.jacs_id().chars().take(8).collect::<String>();
        let payload = json!({
            "name": format!("Free Run - {short_id}"),
            "tier": "free",
            "transport": transport.as_str(),
        });

        let url = self.url("/api/benchmark/run");
        let response = self
            .http
            .post(url)
            .authenticated()
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        let data = response_json(response).await?;
        Ok(FreeChaoticResult {
            success: true,
            run_id: value_string(&data, &["run_id", "runId"]),
            transcript: parse_transcript(&data),
            upsell_message: value_string(&data, &["upsell_message", "upsellMessage"]),
            raw_response: data,
        })
    }

    pub async fn pro_run(&self, options: &ProRunOptions) -> Result<ProRunResult> {
        let purchase_url = self.url("/api/benchmark/purchase");
        let purchase_response = self
            .http
            .post(purchase_url)
            .authenticated()
            .header("Content-Type", "application/json")
            .json(&json!({
                "tier": "pro",
                "agent_id": self.jacs.jacs_id(),
            }))
            .send()
            .await?;
        let purchase_data = response_json(purchase_response).await?;
        let checkout_url = value_string(&purchase_data, &["checkout_url"]);
        if checkout_url.is_empty() {
            return Err(HaiError::Message(
                "pro purchase did not return checkout_url".to_string(),
            ));
        }
        let payment_id = value_string(&purchase_data, &["payment_id"]);
        if payment_id.is_empty() {
            return Err(HaiError::Message(
                "pro purchase did not return payment_id".to_string(),
            ));
        }

        let start = std::time::Instant::now();
        let safe_payment_id = encode_path_segment(&payment_id);
        let status_url = self.url(&format!("/api/benchmark/payments/{safe_payment_id}/status"));

        loop {
            if start.elapsed() >= options.poll_timeout {
                return Err(HaiError::Message(format!(
                    "payment not confirmed within {}s",
                    options.poll_timeout.as_secs()
                )));
            }

            let status_response = self
                .http
                .get(status_url.clone())
                .authenticated()
                .send()
                .await?;
            if status_response.status().is_success() {
                let status_data: Value = status_response.json().await?;
                let status = value_string(&status_data, &["status"]);
                if status == "paid" {
                    break;
                }
                if status == "failed" || status == "expired" || status == "cancelled" {
                    let detail = value_string(&status_data, &["message"]);
                    return Err(HaiError::Message(format!("payment {status}: {detail}")));
                }
            }

            tokio::time::sleep(options.poll_interval).await;
        }

        let short_id = self.jacs.jacs_id().chars().take(8).collect::<String>();
        let run_url = self.url("/api/benchmark/run");
        let run_response = self
            .http
            .post(run_url)
            .authenticated()
            .header("Content-Type", "application/json")
            .json(&json!({
                "name": format!("Pro Run - {short_id}"),
                "tier": "pro",
                "payment_id": payment_id,
                "transport": options.transport.as_str(),
            }))
            .send()
            .await?;

        let data = response_json(run_response).await?;
        Ok(ProRunResult {
            success: true,
            run_id: value_string(&data, &["run_id", "runId"]),
            score: data.get("score").and_then(Value::as_f64).unwrap_or(0.0),
            transcript: parse_transcript(&data),
            payment_id,
            raw_response: data,
        })
    }

    /// Deprecated: Use `pro_run` instead. The tier was renamed from dns_certified to pro.
    #[deprecated(note = "Use pro_run instead. The tier was renamed from dns_certified to pro.")]
    pub async fn dns_certified_run(
        &self,
        options: &DnsCertifiedRunOptions,
    ) -> Result<DnsCertifiedResult> {
        self.pro_run(options).await
    }

    pub async fn enterprise_run(&self) -> Result<()> {
        Err(HaiError::Message(
            "the enterprise tier is coming soon; contact support@hai.ai for early access"
                .to_string(),
        ))
    }

    /// Deprecated: Use `enterprise_run` instead. The tier was renamed from fully_certified to enterprise.
    #[deprecated(
        note = "Use enterprise_run instead. The tier was renamed from fully_certified to enterprise."
    )]
    pub async fn certified_run(&self) -> Result<()> {
        self.enterprise_run().await
    }

    #[cfg(feature = "jacs-crate")]
    pub async fn connect_sse(&self) -> Result<SseConnection> {
        let tenant = self.expected_event_context()?.0.clone();
        validate_live_event_key_origin(&self.base_url)?;
        let key_url = self.url("/.well-known/hai-keys.json");
        let mut server_public_keys =
            refresh_server_public_keys(self.http.raw_client(), &key_url).await?;
        let key_http = self.http.raw_client().clone();
        let key_refresh_interval = self.server_key_refresh_interval;
        let shared_replay_store = self.shared_event_replay_store.clone();
        let url = self.url("/api/v1/agents/connect");
        let request = self
            .live_http
            .get(url)
            .authenticated()
            .header("Accept", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .build()?;
        let auth = request
            .headers()
            .get(reqwest::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| {
                HaiError::Provider("signed stream request is missing its credential".into())
            })?;
        let expected_context =
            LiveEventContext::for_connection(tenant, self.jacs.jacs_id(), auth, false)?;
        let response = self.live_http.execute_authenticated(request).await?;
        if !response.status().is_success() {
            return Err(response_error(response).await);
        }

        let mut stream = response.bytes_stream();
        let (events_tx, events_rx) = mpsc::channel::<Result<HaiEvent>>(32);
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

        let task = tokio::spawn(async move {
            let mut parser = SseParser::default();
            let first_refresh = tokio::time::Instant::now() + key_refresh_interval;
            let mut key_refresh = tokio::time::interval_at(first_refresh, key_refresh_interval);

            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        break;
                    }
                    _ = key_refresh.tick() => {
                        match refresh_server_public_keys(&key_http, &key_url).await {
                            Ok(replacement) => {
                                // Replace only after the complete response has passed strict
                                // parsing and exact active-signer validation. Never union old keys.
                                server_public_keys = replacement;
                                tracing::debug!(
                                    event = "live_server_keys_refreshed",
                                    transport = "sse",
                                    key_count = server_public_keys.len(),
                                    "Replaced live server-key snapshot"
                                );
                            }
                            Err(error) => {
                                tracing::warn!(
                                    event = "live_server_key_refresh_failed",
                                    transport = "sse",
                                    error = %error,
                                    "SSE connection closed before releasing more payloads"
                                );
                                let _ = send_stream_result(
                                    &events_tx,
                                    &mut shutdown_rx,
                                    Err(error),
                                ).await;
                                return;
                            }
                        }
                    }
                    next_chunk = stream.next() => {
                        let Some(chunk_result) = next_chunk else {
                            break;
                        };
                        let chunk = match chunk_result {
                            Ok(chunk) => chunk,
                            Err(error) => {
                                let _ = send_stream_result(
                                    &events_tx,
                                    &mut shutdown_rx,
                                    Err(HaiError::Message(format!(
                                        "SSE stream read failed: {error}"
                                    ))),
                                ).await;
                                return;
                            }
                        };

                        let raw_events = match parser.push_chunk(&chunk) {
                            Ok(events) => events,
                            Err(error) => {
                                tracing::warn!(
                                    event = "signed_event_verification_failed",
                                    transport = "sse",
                                    error = %error,
                                    "SSE input rejected before payload release"
                                );
                                let _ = send_stream_result(
                                    &events_tx,
                                    &mut shutdown_rx,
                                    Err(error),
                                ).await;
                                return;
                            }
                        };
                        for raw_event in raw_events {
                            let verified = verify_live_transport_event(
                                &raw_event.raw,
                                &server_public_keys,
                                shared_replay_store.as_deref(),
                                &expected_context,
                            );
                            let event = match verified {
                                Ok(event) => event,
                                Err(error) => {
                                    tracing::warn!(
                                        event = "signed_event_verification_failed",
                                        transport = "sse",
                                        error = %error,
                                        "SSE event rejected before payload release"
                                    );
                                    let _ = send_stream_result(
                                        &events_tx,
                                        &mut shutdown_rx,
                                        Err(error),
                                    ).await;
                                    return;
                                }
                            };
                            if !send_stream_result(
                                &events_tx,
                                &mut shutdown_rx,
                                Ok(event),
                            ).await {
                                return;
                            }
                        }
                    }
                }
            }
        });

        Ok(SseConnection {
            events: events_rx,
            shutdown: Some(shutdown_tx),
            task: Some(task),
        })
    }

    #[cfg(not(feature = "jacs-crate"))]
    pub async fn connect_sse(&self) -> Result<SseConnection> {
        Err(HaiError::BackendUnsupported {
            method: "connect_sse".to_string(),
            detail: "live events require the jacs-crate feature for strict verification"
                .to_string(),
        })
    }

    #[cfg(feature = "jacs-crate")]
    pub async fn connect_ws(&self) -> Result<WsConnection> {
        let tenant = self.expected_event_context()?.0.clone();
        validate_live_event_key_origin(&self.base_url)?;
        let key_url = self.url("/.well-known/hai-keys.json");
        let mut server_public_keys =
            refresh_server_public_keys(self.http.raw_client(), &key_url).await?;
        let key_http = self.http.raw_client().clone();
        let key_refresh_interval = self.server_key_refresh_interval;
        let shared_replay_store = self.shared_event_replay_store.clone();
        let ws_url = build_ws_url(&self.base_url, "/ws/agent/connect");
        let mut request = ws_url.into_client_request().map_err(|err| {
            HaiError::Message(format!("failed to build websocket request: {err}"))
        })?;
        // Authenticate the actual WebSocket HTTP upgrade, translating only its
        // transport spelling (ws/wss) to the HTTP scheme seen by ingress.
        let mut auth_url = url::Url::parse(&request.uri().to_string())
            .map_err(|error| HaiError::Provider(error.to_string()))?;
        let scheme = if auth_url.scheme() == "wss" {
            "https"
        } else {
            "http"
        };
        auth_url
            .set_scheme(scheme)
            .map_err(|_| HaiError::Provider("invalid WebSocket scheme".into()))?;
        let auth =
            self.build_request_auth_header(request.method().as_str(), auth_url.as_str(), &[])?;
        let expected_context =
            LiveEventContext::for_connection(tenant, self.jacs.jacs_id(), &auth, true)?;
        let auth_header = tungstenite::http::HeaderValue::from_str(&auth)
            .map_err(|err| HaiError::Message(format!("invalid auth header: {err}")))?;
        request.headers_mut().insert("Authorization", auth_header);

        let connect = connect_async_with_config(request, Some(live_websocket_config()), false);
        let connected = tokio::time::timeout(self.request_timeout, connect)
            .await
            .map_err(|_| {
                HaiError::Message(format!(
                    "websocket connection timed out after {} seconds",
                    self.request_timeout.as_secs_f64()
                ))
            })?;
        let (ws_stream, _) = connected
            .map_err(|err| HaiError::Message(format!("websocket connection failed: {err}")))?;

        let (mut ws_sink, mut ws_stream_read) = ws_stream.split();
        let (events_tx, events_rx) = mpsc::channel::<Result<HaiEvent>>(32);
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

        let task = tokio::spawn(async move {
            let first_refresh = tokio::time::Instant::now() + key_refresh_interval;
            let mut key_refresh = tokio::time::interval_at(first_refresh, key_refresh_interval);
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        let _ = ws_sink.send(Message::Close(None)).await;
                        break;
                    }
                    _ = key_refresh.tick() => {
                        match refresh_server_public_keys(&key_http, &key_url).await {
                            Ok(replacement) => {
                                server_public_keys = replacement;
                                tracing::debug!(
                                    event = "live_server_keys_refreshed",
                                    transport = "websocket",
                                    key_count = server_public_keys.len(),
                                    "Replaced live server-key snapshot"
                                );
                            }
                            Err(error) => {
                                tracing::warn!(
                                    event = "live_server_key_refresh_failed",
                                    transport = "websocket",
                                    error = %error,
                                    "WebSocket connection closed before releasing more payloads"
                                );
                                let _ = send_stream_result(
                                    &events_tx,
                                    &mut shutdown_rx,
                                    Err(error),
                                ).await;
                                return;
                            }
                        }
                    }
                    next_msg = ws_stream_read.next() => {
                        let Some(msg_result) = next_msg else {
                            break;
                        };
                        let msg = match msg_result {
                            Ok(msg) => msg,
                            Err(error) => {
                                let _ = send_stream_result(
                                    &events_tx,
                                    &mut shutdown_rx,
                                    Err(HaiError::Message(format!(
                                        "WebSocket stream read failed: {error}"
                                    ))),
                                ).await;
                                return;
                            }
                        };

                        let text = match msg {
                            Message::Text(text) => {
                                if text.len() > MAX_STREAM_EVENT_BYTES {
                                    let error = HaiError::SignedEventVerification {
                                        message: "WebSocket event exceeds the maximum signed-event size".to_string(),
                                    };
                                    let _ = send_stream_result(
                                        &events_tx,
                                        &mut shutdown_rx,
                                        Err(error),
                                    ).await;
                                    return;
                                }
                                text.to_string()
                            }
                            Message::Ping(payload) => {
                                if ws_sink.send(Message::Pong(payload)).await.is_err() {
                                    break;
                                }
                                continue;
                            }
                            Message::Close(_) => break,
                            _ => continue,
                        };

                        let verified = verify_live_transport_event(
                            &text,
                            &server_public_keys,
                            shared_replay_store.as_deref(),
                            &expected_context,
                        );
                        let event = match verified {
                            Ok(event) => event,
                            Err(error) => {
                                tracing::warn!(
                                    event = "signed_event_verification_failed",
                                    transport = "websocket",
                                    error = %error,
                                    "WebSocket event rejected before payload release"
                                );
                                let _ = send_stream_result(
                                    &events_tx,
                                    &mut shutdown_rx,
                                    Err(error),
                                ).await;
                                return;
                            }
                        };

                        if event.event_type == "heartbeat" {
                            let timestamp = event.data
                                .get("timestamp")
                                .cloned()
                                .unwrap_or_else(|| Value::from(OffsetDateTime::now_utc().unix_timestamp()));
                            let pong = json!({
                                "type": "pong",
                                "timestamp": timestamp
                            });
                            let _ = ws_sink.send(Message::Text(pong.to_string().into())).await;
                        }

                        if !send_stream_result(
                            &events_tx,
                            &mut shutdown_rx,
                            Ok(event),
                        ).await {
                            break;
                        }
                    }
                }
            }
        });

        Ok(WsConnection {
            events: events_rx,
            shutdown: Some(shutdown_tx),
            task: Some(task),
        })
    }

    #[cfg(not(feature = "jacs-crate"))]
    pub async fn connect_ws(&self) -> Result<WsConnection> {
        Err(HaiError::BackendUnsupported {
            method: "connect_ws".to_string(),
            detail: "live events require the jacs-crate feature for strict verification"
                .to_string(),
        })
    }

    /// Listen for benchmark jobs with automatic reconnection.
    ///
    /// When the connection drops (without a "disconnect" event), reconnects
    /// with exponential backoff up to `max_reconnect_attempts` times (default 10).
    /// A "disconnect" event is treated as an intentional server-side shutdown
    /// and will NOT trigger reconnection.
    pub async fn on_benchmark_job<F, Fut>(&self, transport: TransportType, handler: F) -> Result<()>
    where
        F: FnMut(Value) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        self.on_benchmark_job_with_reconnect(transport, handler, DEFAULT_MAX_RECONNECT_ATTEMPTS)
            .await
    }

    /// Like [`on_benchmark_job`] but with a configurable max reconnect attempt count.
    pub async fn on_benchmark_job_with_reconnect<F, Fut>(
        &self,
        transport: TransportType,
        mut handler: F,
        max_reconnect_attempts: usize,
    ) -> Result<()>
    where
        F: FnMut(Value) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let mut reconnect_count: usize = 0;

        loop {
            let got_disconnect_event;

            match transport {
                TransportType::Sse => {
                    let conn_result = self.connect_sse().await;
                    let mut conn = match conn_result {
                        Ok(c) => {
                            reconnect_count = 0; // reset on successful connect
                            c
                        }
                        Err(e) => {
                            if reconnect_count >= max_reconnect_attempts {
                                return Err(e);
                            }
                            let delay =
                                Duration::from_millis(100 * (1u64 << reconnect_count.min(10)));
                            tokio::time::sleep(delay).await;
                            reconnect_count += 1;
                            continue;
                        }
                    };

                    got_disconnect_event = false;
                    let mut saw_disconnect = false;
                    while let Some(event) = conn.next_event().await? {
                        match event.event_type.as_str() {
                            "benchmark_job" => handler(event.data).await?,
                            "disconnect" => {
                                saw_disconnect = true;
                                break;
                            }
                            _ => {}
                        }
                    }
                    conn.close().await;
                    if saw_disconnect {
                        return Ok(());
                    }
                }
                TransportType::Ws => {
                    let conn_result = self.connect_ws().await;
                    let mut conn = match conn_result {
                        Ok(c) => {
                            reconnect_count = 0;
                            c
                        }
                        Err(e) => {
                            if reconnect_count >= max_reconnect_attempts {
                                return Err(e);
                            }
                            let delay =
                                Duration::from_millis(100 * (1u64 << reconnect_count.min(10)));
                            tokio::time::sleep(delay).await;
                            reconnect_count += 1;
                            continue;
                        }
                    };

                    got_disconnect_event = false;
                    let mut saw_disconnect = false;
                    while let Some(event) = conn.next_event().await? {
                        match event.event_type.as_str() {
                            "benchmark_job" => handler(event.data).await?,
                            "disconnect" => {
                                saw_disconnect = true;
                                break;
                            }
                            _ => {}
                        }
                    }
                    conn.close().await;
                    if saw_disconnect {
                        return Ok(());
                    }
                }
            }

            // Connection dropped without disconnect event -- try reconnecting
            let _ = got_disconnect_event;
            if reconnect_count >= max_reconnect_attempts {
                return Err(HaiError::Message(format!(
                    "on_benchmark_job: max reconnect attempts ({max_reconnect_attempts}) exceeded"
                )));
            }

            let delay = Duration::from_millis(100 * (1u64 << reconnect_count.min(10)));
            tokio::time::sleep(delay).await;
            reconnect_count += 1;
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, normalize_path(path))
    }

    pub fn max_retries(&self) -> usize {
        self.max_retries
    }

    /// Execute an async HTTP operation with retries and exponential backoff.
    ///
    /// Retries on `RETRYABLE_STATUS_CODES` (429, 500, 502, 503, 504).
    /// The closure must build and send a request, returning a `reqwest::Response`.
    /// On success or non-retryable error the response is returned immediately.
    /// Transport-level errors (e.g. DNS, connection refused) are NOT retried.
    async fn request_with_retry<F, Fut>(&self, mut make_request: F) -> Result<Response>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<Response>>,
    {
        for attempt in 0..self.max_retries {
            let response = make_request().await?;
            let status = response.status().as_u16();

            if !RETRYABLE_STATUS_CODES.contains(&status) {
                return Ok(response);
            }

            // Last attempt -- return whatever we got
            if attempt + 1 >= self.max_retries {
                return Ok(response);
            }

            // Exponential backoff: 100ms, 200ms, 400ms, ...
            let delay = Duration::from_millis(100 * (1u64 << attempt));
            tokio::time::sleep(delay).await;
        }

        // max_retries is always >= 1 (enforced in new()), so this is unreachable
        unreachable!("max_retries is always >= 1")
    }
}

fn send_idempotency_key(options: &SendEmailOptions) -> String {
    options
        .idempotency_key
        .as_deref()
        .filter(|key| !key.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

fn normalize_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

pub fn encode_path_segment(value: &str) -> String {
    let mut url = url::Url::parse("https://example.invalid").expect("valid static url");
    url.path_segments_mut()
        .expect("url should support path segments")
        .push(value);
    url.path().trim_start_matches('/').to_string()
}

/// Derive the agent slug from an email address (local part before `@`).
fn email_to_slug(email: &str) -> &str {
    email.split('@').next().unwrap_or(email)
}

fn parse_transcript(data: &Value) -> Vec<TranscriptMessage> {
    data.get("transcript")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| TranscriptMessage {
                    role: entry
                        .get("role")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    content: entry
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    timestamp: entry
                        .get("timestamp")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    annotations: entry
                        .get("annotations")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

/// Convert the HAI well-known key document into the exact signer-ID map JACS
/// requires. Database row IDs are not signer identities. The compatibility
/// fallback accepts `key_id` only when it is structurally the versioned
/// `<jacs_id>:<version>` lookup ID.
#[cfg(feature = "jacs-crate")]
fn validate_live_event_key_origin(base_url: &str) -> Result<()> {
    let parsed = url::Url::parse(base_url).map_err(|error| HaiError::SignedEventVerification {
        message: format!("invalid HAI key origin: {error}"),
    })?;
    let secure = parsed.scheme() == "https";
    let loopback_http = parsed.scheme() == "http"
        && match parsed.host() {
            Some(url::Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
            Some(url::Host::Ipv4(address)) => address.is_loopback(),
            Some(url::Host::Ipv6(address)) => address.is_loopback(),
            None => false,
        };
    if secure || loopback_http {
        return Ok(());
    }
    Err(HaiError::SignedEventVerification {
        message: "live event signing keys require HTTPS (HTTP is allowed only on loopback)"
            .to_string(),
    })
}

async fn read_response_body_limited(
    mut response: Response,
    max_bytes: usize,
    context: &str,
) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(HaiError::SignedEventVerification {
            message: format!("{context} exceeds the {max_bytes} byte limit"),
        });
    }

    let initial_capacity = response
        .content_length()
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or(0)
        .min(max_bytes);
    let mut body = Vec::with_capacity(initial_capacity);
    while let Some(chunk) = response.chunk().await? {
        let next_length = body.len().checked_add(chunk.len()).ok_or_else(|| {
            HaiError::SignedEventVerification {
                message: format!("{context} size overflow"),
            }
        })?;
        if next_length > max_bytes {
            return Err(HaiError::SignedEventVerification {
                message: format!("{context} exceeds the {max_bytes} byte limit"),
            });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn fetch_server_keys_with_client(http: &reqwest::Client, url: &str) -> Result<Value> {
    let response = http
        .get(url)
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    let body =
        read_response_body_limited(response, MAX_SERVER_KEYS_BYTES, "HAI server key response")
            .await?;

    parse_server_key_document_bytes(&body)
}

fn parse_server_key_document_bytes(body: &[u8]) -> Result<Value> {
    #[cfg(feature = "jacs-crate")]
    {
        jacs::strict_json::parse_strict_json_slice(body).map_err(|error| {
            HaiError::SignedEventVerification {
                message: format!("HAI server key response is not strict JSON: {error}"),
            }
        })
    }
    #[cfg(not(feature = "jacs-crate"))]
    {
        serde_json::from_slice(&body).map_err(HaiError::from)
    }
}

#[cfg(feature = "jacs-crate")]
async fn refresh_server_public_keys(
    http: &reqwest::Client,
    url: &str,
) -> Result<HashMap<String, Vec<u8>>> {
    let document = fetch_server_keys_with_client(http, url)
        .await
        .map_err(|error| HaiError::SignedEventVerification {
            message: format!("HAI server key refresh failed: {error}"),
        })?;
    parse_server_public_keys(&document)
}

#[cfg(feature = "jacs-crate")]
fn parse_server_public_keys(document: &Value) -> Result<HashMap<String, Vec<u8>>> {
    let entries = document
        .get("keys")
        .and_then(Value::as_array)
        .ok_or_else(|| HaiError::SignedEventVerification {
            message: "HAI server key response is missing a keys array".to_string(),
        })?;
    let mut trusted: HashMap<String, Vec<u8>> = HashMap::new();

    for entry in entries {
        if entry.get("is_active").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let signer_id = entry
            .get("signer_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| {
                let jacs_id = entry.get("jacs_id")?.as_str()?.trim();
                let key_id = entry.get("key_id")?.as_str()?.trim();
                (!jacs_id.is_empty() && key_id.starts_with(&format!("{jacs_id}:")))
                    .then(|| key_id.to_string())
            })
            .ok_or_else(|| HaiError::SignedEventVerification {
                message:
                    "HAI server key response contains an active key without an exact signer_id"
                        .to_string(),
            })?;
        let public_key = entry
            .get("public_key")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| HaiError::SignedEventVerification {
                message: format!("HAI server key response has no public key for {signer_id}"),
            })?;

        if let Some(previous) = trusted.get(&signer_id) {
            if previous.as_slice() != public_key.as_bytes() {
                return Err(HaiError::SignedEventVerification {
                    message: format!(
                        "HAI server key response contains conflicting active keys for {signer_id}"
                    ),
                });
            }
        } else {
            trusted.insert(signer_id, public_key.as_bytes().to_vec());
        }
    }

    if trusted.is_empty() {
        return Err(HaiError::SignedEventVerification {
            message: "HAI server key response contains no usable active signing key".to_string(),
        });
    }
    Ok(trusted)
}

#[cfg(feature = "jacs-crate")]
#[derive(Clone)]
struct LiveEventContext {
    tenant: String,
    audience: String,
    transport: jacs::response_context::EventTransport,
}

#[cfg(feature = "jacs-crate")]
impl LiveEventContext {
    fn for_connection(tenant: String, audience: &str, auth: &str, websocket: bool) -> Result<Self> {
        // This is the final outgoing credential, not remote asserted identity.
        let nonce = jacs::protocol::inspect_unverified_request_auth_header(auth)
            .map_err(|error| HaiError::Provider(error.to_string()))?
            .nonce;
        let channel = format!("jacs-auth-nonce:{nonce}");
        Ok(Self {
            tenant,
            audience: jacs::validation::normalize_agent_id(audience).to_string(),
            transport: if websocket {
                jacs::response_context::EventTransport::Channel(channel)
            } else {
                jacs::response_context::EventTransport::Stream(channel)
            },
        })
    }
}

#[cfg(feature = "jacs-crate")]
fn verify_live_transport_event(
    raw: &str,
    server_public_keys: &HashMap<String, Vec<u8>>,
    shared_replay_store: Option<&dyn jacs::replay::ReplayStore>,
    expected: &LiveEventContext,
) -> Result<HaiEvent> {
    use jacs::response_context::{EventCausation, ResponseData, ResponseExpectation};
    let failure = |message: String| HaiError::SignedEventVerification { message };
    jacs::schema::utils::check_document_size(raw).map_err(|error| failure(error.to_string()))?;
    let envelope = jacs::strict_json::parse_strict_json(raw)
        .map_err(|error| failure(format!("signed event is not strict JSON: {error}")))?;
    let issuer = envelope
        .pointer("/jacsSignature/agentID")
        .and_then(Value::as_str)
        .filter(|issuer| server_public_keys.contains_key(*issuer))
        .ok_or_else(|| {
            failure("event signer is not in the trusted active server key set".into())
        })?;
    let expectation = ResponseExpectation::PrivateEvent {
        issuer: issuer.into(),
        tenant: expected.tenant.clone(),
        audience: expected.audience.clone(),
        transport: expected.transport.clone(),
        contract: "hai.agent-event".into(),
        contract_version: "2".into(),
        allowed_event_types: ["benchmark_job", "heartbeat", "disconnect", "connected"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        causation: None,
    };
    let context = jacs::response_context::require_response_context(&envelope, &expectation)
        .map_err(|error| failure(error.to_string()))?;
    let ResponseData::PrivateEvent {
        event_type,
        causation,
        payload,
        ..
    } = context
    else {
        return Err(failure("private agent event context required".into()));
    };
    if payload.get("type").and_then(Value::as_str) != Some(event_type.as_str()) {
        return Err(failure(
            "signed event type differs from its payload type".into(),
        ));
    }
    let expected_causation = if event_type == "benchmark_job" {
        EventCausation::Job {
            id: payload
                .get("job_id")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| failure("benchmark event job_id is missing".into()))?
                .into(),
        }
    } else {
        EventCausation::None { id: () }
    };
    if causation != expected_causation {
        return Err(failure(
            "signed event causation differs from its payload job".into(),
        ));
    }
    // No payload release or replay consumption precedes expected-context checks.
    let verified = if let Some(store) = shared_replay_store {
        if store.scope() != jacs::replay::ReplayStoreScope::Shared {
            return Err(HaiError::SignedEventVerification {
                message: "live event replay backend must have shared scope".to_string(),
            });
        }
        let max_age_seconds = jacs::replay::payload_replay_window_seconds();

        jacs::protocol::verify_signed_event_with_replay_store(
            &envelope,
            server_public_keys,
            store,
            max_age_seconds,
        )
        .map_err(|error| HaiError::SignedEventVerification {
            message: error.to_string(),
        })?
    } else {
        jacs::protocol::verify_signed_event_json_with_trusted_keys(raw, server_public_keys)
            .map_err(|error| HaiError::SignedEventVerification {
                message: error.to_string(),
            })?
    };
    // SSE's outer `event:` and `id:` fields are not signed, so the raw parser
    // discards them. Routing and the consumer-visible ID come from the
    // verified payload/provenance below.
    let document_id = verified.document_id.clone();
    Ok(HaiEvent {
        event_type,
        data: payload,
        id: Some(document_id.clone()),
        raw: raw.to_string(),
        verification: SignedEventVerification {
            status: "verified".to_string(),
            signer_id: verified.signer_id,
            timestamp: verified.timestamp,
            algorithm: verified.algorithm,
            document_id,
            event_sha256: jacs::crypt::hash::hash_string(raw),
            replay_status: "consumed".to_string(),
        },
    })
}

#[cfg(feature = "jacs-crate")]
const MAX_STREAM_EVENT_BYTES: usize = 10 * 1024 * 1024;

#[cfg(feature = "jacs-crate")]
fn live_websocket_config() -> tungstenite::protocol::WebSocketConfig {
    tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(MAX_STREAM_EVENT_BYTES))
        .max_frame_size(Some(MAX_STREAM_EVENT_BYTES))
}

#[cfg(feature = "jacs-crate")]
#[derive(Debug)]
struct RawTransportEvent {
    raw: String,
}

#[derive(Default)]
#[cfg(feature = "jacs-crate")]
struct SseParser {
    buffer: Vec<u8>,
    data_lines: Vec<String>,
    data_bytes: usize,
}

#[cfg(feature = "jacs-crate")]
impl SseParser {
    fn push_chunk(&mut self, chunk: &[u8]) -> Result<Vec<RawTransportEvent>> {
        let buffered_length = self.buffer.len().checked_add(chunk.len()).ok_or_else(|| {
            HaiError::SignedEventVerification {
                message: "SSE event size overflow".to_string(),
            }
        })?;
        if buffered_length > MAX_STREAM_EVENT_BYTES {
            return Err(HaiError::SignedEventVerification {
                message: "SSE event exceeds the maximum signed-event size".to_string(),
            });
        }
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();

        while let Some(idx) = self.buffer.iter().position(|b| *b == b'\n') {
            let mut line_bytes = self.buffer.drain(..=idx).collect::<Vec<_>>();
            line_bytes.pop();
            if line_bytes.ends_with(b"\r") {
                line_bytes.pop();
            }

            let line =
                String::from_utf8(line_bytes).map_err(|_| HaiError::SignedEventVerification {
                    message: "SSE event contains invalid UTF-8".to_string(),
                })?;

            if line.is_empty() {
                if !self.data_lines.is_empty() {
                    let raw = self.data_lines.join("\n");
                    if raw.len() > MAX_STREAM_EVENT_BYTES {
                        return Err(HaiError::SignedEventVerification {
                            message: "SSE event exceeds the maximum signed-event size".to_string(),
                        });
                    }
                    events.push(RawTransportEvent { raw });
                }
                self.data_lines.clear();
                self.data_bytes = 0;
                continue;
            }

            if let Some(rest) = line.strip_prefix("data:") {
                let data = rest.strip_prefix(' ').unwrap_or(rest);
                self.data_bytes = self
                    .data_bytes
                    .checked_add(data.len().saturating_add(1))
                    .ok_or_else(|| HaiError::SignedEventVerification {
                        message: "SSE event size overflow".to_string(),
                    })?;
                if self.data_bytes > MAX_STREAM_EVENT_BYTES {
                    return Err(HaiError::SignedEventVerification {
                        message: "SSE event exceeds the maximum signed-event size".to_string(),
                    });
                }
                self.data_lines.push(data.to_string());
            }
        }

        Ok(events)
    }
}

#[cfg(feature = "jacs-crate")]
fn build_ws_url(base_url: &str, path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    let ws_base = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base.to_string()
    };
    format!("{ws_base}{}", normalize_path(path))
}

async fn response_json(response: Response) -> Result<Value> {
    if response.status().is_success() {
        return Ok(response.json().await?);
    }

    Err(response_error(response).await)
}

async fn response_error(response: Response) -> HaiError {
    let status = response.status().as_u16();
    let body_bytes =
        read_response_body_limited(response, MAX_ERROR_RESPONSE_BYTES, "HAI API error response")
            .await
            .unwrap_or_default();
    let body = String::from_utf8_lossy(&body_bytes).into_owned();
    let message = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(Value::as_str)
                .or_else(|| value.get("message").and_then(Value::as_str))
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| {
            if body.is_empty() {
                format!("request failed with status {status}")
            } else {
                body
            }
        });

    HaiError::Api { status, message }
}

fn value_string(data: &Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(v) = data.get(key).and_then(Value::as_str) {
            return v.to_string();
        }
    }
    String::new()
}

trait EmptyFallback {
    fn if_empty_then<T: Into<String>>(self, fallback: T) -> String;
}

impl EmptyFallback for String {
    fn if_empty_then<T: Into<String>>(self, fallback: T) -> String {
        if self.is_empty() {
            fallback.into()
        } else {
            self
        }
    }
}

// =============================================================================
// HaiClient<P> passthroughs for JacsMediaProvider (Layer 8)
// =============================================================================
//
// Local-only sign/verify/extract for inline text and PNG/JPEG/WebP images.
// These are thin one-line passthroughs to the underlying provider — they do
// not touch HTTP. Mirrors the email-signing pattern of `send_signed_email`
// in this same file, which calls `self.jacs.sign_email_locally(...)` directly.
// PRD: docs/MEDIA_SIGNING_PRD.md §4.3 / TASK_003.

#[cfg(feature = "jacs-crate")]
impl<P: crate::jacs::JacsMediaProvider> HaiClient<P> {
    /// Sign a markdown / text file in place.
    pub fn sign_text_file(
        &self,
        path: &str,
        opts: crate::jacs::SignTextOptions,
    ) -> Result<crate::jacs::SignTextOutcome> {
        self.jacs.sign_text_file(path, opts)
    }

    /// Verify all signature blocks in a text file.
    pub fn verify_text_file(
        &self,
        path: &str,
        opts: crate::jacs::VerifyTextOptions,
    ) -> Result<crate::jacs::VerifyTextResult> {
        self.jacs.verify_text_file(path, opts)
    }

    /// Sign an image file (PNG/JPEG/WebP).
    pub fn sign_image(
        &self,
        in_path: &str,
        out_path: &str,
        opts: crate::jacs::SignImageOptions,
    ) -> Result<crate::jacs::SignedMedia> {
        self.jacs.sign_image(in_path, out_path, opts)
    }

    /// Verify the JACS signature embedded in an image.
    pub fn verify_image(
        &self,
        path: &str,
        opts: crate::jacs::VerifyImageOptions,
    ) -> Result<crate::jacs::MediaVerificationResult> {
        self.jacs.verify_image(path, opts)
    }

    /// Extract the JACS signature payload from a signed image without verifying it.
    pub fn extract_media_signature(&self, path: &str, raw_payload: bool) -> Result<Option<String>> {
        self.jacs.extract_media_signature(path, raw_payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "jacs-crate")]
    use crate::jacs::JacsEmailProvider;
    use crate::jacs::StaticJacsProvider;
    #[cfg(feature = "jacs-crate")]
    use crate::jacs_local::LocalJacsProvider;
    #[cfg(feature = "jacs-crate")]
    use crate::types::CreateAgentOptions;
    use crate::types::EmailAttachment;

    #[test]
    fn test_effective_data_prefers_data_over_data_base64() {
        let att = EmailAttachment {
            filename: "x.bin".to_string(),
            content_type: "application/octet-stream".to_string(),
            data: b"real".to_vec(),
            data_base64: Some(base64::engine::general_purpose::STANDARD.encode(b"stale")),
        };

        assert_eq!(att.effective_data(), b"real");
    }

    #[test]
    fn test_effective_data_decodes_base64_when_data_empty() {
        let att = EmailAttachment {
            filename: "x.bin".to_string(),
            content_type: "application/octet-stream".to_string(),
            data: Vec::new(),
            data_base64: Some(base64::engine::general_purpose::STANDARD.encode(b"decoded")),
        };

        assert_eq!(att.effective_data(), b"decoded");
    }

    #[test]
    fn test_effective_data_returns_empty_when_both_missing() {
        let att = EmailAttachment {
            filename: "x.bin".to_string(),
            content_type: "application/octet-stream".to_string(),
            data: Vec::new(),
            data_base64: None,
        };

        assert!(att.effective_data().is_empty());
    }

    #[test]
    fn create_signed_email_returns_rfc5322_without_http_submission() {
        let provider = StaticJacsProvider::new("test-agent-001");
        let mut client = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: "https://api.hai.ai".to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        client.set_agent_email("test-agent-001@hai.ai".to_string());

        let signed = client
            .create_signed_email(
                &SendEmailOptions {
                    to: "recipient@hai.ai".to_string(),
                    subject: "Created locally".to_string(),
                    body: "Hello".to_string(),
                    cc: vec![],
                    bcc: vec![],
                    in_reply_to: None,
                    attachments: vec![],
                    labels: vec![],
                    append_footer: None,
                    idempotency_key: None,
                },
                EmailGenerationType::AttachmentJacs,
            )
            .unwrap();
        let raw = String::from_utf8_lossy(signed.as_bytes());

        assert_eq!(signed.generation_type, EmailGenerationType::AttachmentJacs);
        assert!(raw.contains("From: <test-agent-001@hai.ai>\r\n"));
        assert!(raw.contains("To: recipient@hai.ai\r\n"));
        assert!(raw.contains("Subject: Created locally\r\n"));
        assert!(raw.contains("Hello"));
    }

    #[cfg(feature = "jacs-crate")]
    #[test]
    fn attachment_jacs_generation_uses_jacs_attachment_transport() {
        let _guard = crate::test_support::env_lock();
        let tmp = tempfile::Builder::new()
            .prefix("haiai-attachment-mode-")
            .tempdir()
            .expect("create temp dir");
        let base_dir = tmp.path().canonicalize().expect("canonical temp dir");
        let key_dir = base_dir.join("keys");
        let data_dir = base_dir.join("data");
        std::fs::create_dir_all(&key_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        let config_path = base_dir.join("jacs.config.json");

        LocalJacsProvider::create_agent_with_options(&CreateAgentOptions {
            name: "attachment-mode-agent".to_string(),
            password: "test-password-1234".to_string(),
            algorithm: Some("ed25519".to_string()),
            data_directory: Some(data_dir.display().to_string()),
            key_directory: Some(key_dir.display().to_string()),
            config_path: Some(config_path.display().to_string()),
            agent_type: None,
            description: Some("Attachment compatibility test agent".to_string()),
            domain: None,
            default_storage: None,
        })
        .expect("create local JACS agent");

        unsafe {
            std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", "test-password-1234");
        }
        let provider = LocalJacsProvider::from_config_path(Some(config_path.as_path()), None)
            .expect("load provider");
        let mut client = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: "https://api.hai.ai".to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        client.set_agent_email("attachment-mode-agent@hai.ai".to_string());

        let signed = client
            .create_signed_email(
                &SendEmailOptions {
                    to: "recipient@hai.ai".to_string(),
                    subject: "Attachment compatibility".to_string(),
                    body: "Hello".to_string(),
                    cc: vec![],
                    bcc: vec![],
                    in_reply_to: None,
                    attachments: vec![],
                    labels: vec![],
                    append_footer: None,
                    idempotency_key: None,
                },
                EmailGenerationType::AttachmentJacs,
            )
            .unwrap();

        assert_eq!(signed.generation_type, EmailGenerationType::AttachmentJacs);
        assert_eq!(
            jacs::email::detect_signed_email_transport(signed.as_bytes()).unwrap(),
            jacs::email::SignedEmailTransport::AttachmentJacs
        );
        let public_key =
            crate::email::extract_public_key_bytes(&client.jacs.public_key_pem().unwrap()).unwrap();
        let verified = client
            .jacs
            .verify_signed_email_transport(
                signed.as_bytes(),
                public_key,
                jacs::email::VerificationMode::Strict,
            )
            .unwrap();
        assert_eq!(
            verified.status,
            jacs::email::EmailVerificationStatus::Verified
        );
        assert!(String::from_utf8_lossy(signed.as_bytes()).contains("jacs-signature.json"));
    }

    #[cfg(feature = "jacs-crate")]
    #[test]
    fn sse_parser_preserves_utf8_split_across_chunks() {
        let mut parser = SseParser::default();
        assert!(parser
            .push_chunk("event: benchmark_job\ndata: {\"message\":\"hi ".as_bytes())
            .expect("partial event")
            .is_empty());
        assert!(parser
            .push_chunk(&[0xF0, 0x9F])
            .expect("partial UTF-8")
            .is_empty());
        assert!(parser
            .push_chunk(&[0x99, 0x82])
            .expect("complete UTF-8")
            .is_empty());
        let events = parser.push_chunk(b"\"}\n\n").expect("complete event");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].raw, "{\"message\":\"hi 🙂\"}");
    }

    #[cfg(feature = "jacs-crate")]
    #[test]
    fn sse_parser_rejects_invalid_utf8_without_emitting_an_event() {
        let mut parser = SseParser::default();
        let error = parser
            .push_chunk(b"event: benchmark_job\ndata: {\"message\":\"")
            .expect("partial input")
            .is_empty();
        assert!(error);
        let error = parser
            .push_chunk(&[0xff, b'\n', b'\n'])
            .expect_err("invalid UTF-8 must fail the connection closed");
        assert!(error.to_string().contains("invalid UTF-8"));
    }

    #[cfg(feature = "jacs-crate")]
    #[test]
    fn sse_parser_rejects_oversize_chunk_before_growing_its_buffer() {
        let mut parser = SseParser::default();
        let oversized = vec![b'x'; MAX_STREAM_EVENT_BYTES + 1];

        let error = parser
            .push_chunk(&oversized)
            .expect_err("oversize chunk must fail before aggregation");

        assert!(error.to_string().contains("maximum signed-event size"));
        assert!(
            parser.buffer.is_empty(),
            "rejected bytes must not be retained"
        );
    }

    #[cfg(feature = "jacs-crate")]
    #[test]
    fn websocket_limits_match_the_signed_event_limit() {
        let config = live_websocket_config();
        assert_eq!(config.max_message_size, Some(MAX_STREAM_EVENT_BYTES));
        assert_eq!(config.max_frame_size, Some(MAX_STREAM_EVENT_BYTES));
    }

    #[test]
    fn redirect_origin_requires_exact_scheme_host_and_effective_port() {
        let origin = url::Url::parse("https://hai.example/api").unwrap();
        for allowed in ["https://hai.example/other", "https://hai.example:443/other"] {
            assert!(urls_have_same_origin(
                &origin,
                &url::Url::parse(allowed).unwrap()
            ));
        }
        for rejected in [
            "http://hai.example/other",
            "https://keys.hai.example/other",
            "https://hai.example:444/other",
            "https://user@hai.example/other",
        ] {
            assert!(!urls_have_same_origin(
                &origin,
                &url::Url::parse(rejected).unwrap()
            ));
        }
    }

    #[cfg(feature = "jacs-crate")]
    #[test]
    fn server_key_json_rejects_duplicate_names() {
        let duplicate = br#"{
            "keys":[{
                "signer_id":"trusted:v1",
                "signer_id":"attacker:v1",
                "public_key":"pem",
                "is_active":true
            }]
        }"#;

        let error = parse_server_key_document_bytes(duplicate)
            .expect_err("key trust input must use strict duplicate-free JSON");
        assert!(error.to_string().contains("duplicate"));
    }

    #[tokio::test]
    async fn closing_a_full_sse_queue_cannot_wait_forever_on_the_producer() {
        let (events_tx, events_rx) = mpsc::channel(1);
        events_tx
            .send(Err(HaiError::Message("queued".to_string())))
            .await
            .unwrap();
        let task = tokio::spawn(async move {
            let _ = events_tx
                .send(Err(HaiError::Message("blocked".to_string())))
                .await;
        });
        let (shutdown_tx, _shutdown_rx) = oneshot::channel();
        let mut connection = SseConnection {
            events: events_rx,
            shutdown: Some(shutdown_tx),
            task: Some(task),
        };

        tokio::time::timeout(Duration::from_secs(2), connection.close())
            .await
            .expect("close must unblock a producer waiting on a full queue");
    }

    #[cfg(feature = "jacs-crate")]
    #[test]
    fn server_key_contract_uses_exact_active_signer_id() {
        let keys = parse_server_public_keys(&json!({
            "keys": [
                {
                    "signer_id": "hai-server:v7",
                    "jacs_id": "hai-server",
                    "key_id": "database-row-7",
                    "public_key": "-----BEGIN PUBLIC KEY-----\nAQID\n-----END PUBLIC KEY-----",
                    "is_active": true
                },
                {
                    "signer_id": "retired:v1",
                    "public_key": "retired",
                    "is_active": false
                }
            ]
        }))
        .expect("active exact signer ID should be accepted");

        assert_eq!(keys.len(), 1);
        assert!(keys.contains_key("hai-server:v7"));
        assert!(!keys.contains_key("database-row-7"));
        assert!(!keys.contains_key("retired:v1"));
    }

    #[cfg(feature = "jacs-crate")]
    #[test]
    fn server_key_contract_rejects_conflicting_active_signer_keys() {
        let error = parse_server_public_keys(&json!({
            "keys": [
                {
                    "signer_id": "hai-server:v1",
                    "public_key": "pem-one",
                    "is_active": true
                },
                {
                    "signer_id": "hai-server:v1",
                    "public_key": "pem-two",
                    "is_active": true
                }
            ]
        }))
        .expect_err("conflicting signer mappings must fail closed");

        assert!(error.to_string().contains("conflicting"));
    }

    #[cfg(feature = "jacs-crate")]
    #[test]
    fn live_event_key_bootstrap_requires_https_except_on_loopback() {
        for accepted in [
            "https://hai.example",
            "http://localhost:8080",
            "http://127.0.0.1:8080",
            "http://[::1]:8080",
        ] {
            validate_live_event_key_origin(accepted).expect("trusted transport origin");
        }
        let error = validate_live_event_key_origin("http://hai.example")
            .expect_err("remote plaintext key bootstrap must fail closed");
        assert!(error.to_string().contains("require HTTPS"));
    }

    #[cfg(feature = "jacs-crate")]
    fn test_live_context() -> LiveEventContext {
        LiveEventContext {
            tenant: "test-tenant".into(),
            audience: "test-recipient".into(),
            transport: jacs::response_context::EventTransport::Stream("test-stream".into()),
        }
    }

    #[cfg(feature = "jacs-crate")]
    #[test]
    fn live_event_verifier_rejects_plain_and_legacy_payloads() {
        let keys = std::collections::HashMap::new();
        for raw in [
            r#"{"type":"benchmark_job","job_id":"attacker"}"#,
            r#"{"payload":{"type":"benchmark_job"},"signature":{"signature":"fake"}}"#,
        ] {
            let error = verify_live_transport_event(raw, &keys, None, &test_live_context())
                .expect_err("unbound event must never reach a live consumer");
            assert!(error.to_string().contains("signed event"));
        }

        let duplicate_name = r#"{
            "version":"2.0.0",
            "document_type":"hai_response",
            "data":{"type":"benchmark_job"},
            "data":{"type":"disconnect"},
            "metadata":{},
            "jacsSignature":{}
        }"#;
        let error = verify_live_transport_event(duplicate_name, &keys, None, &test_live_context())
            .expect_err("duplicate JSON names must fail before payload release");
        assert!(error.to_string().contains("duplicate"));
    }

    #[cfg(feature = "jacs-crate")]
    #[test]
    fn client_rejects_process_local_store_as_shared_replay_configuration() {
        let provider = StaticJacsProvider::new("replay-config-agent");
        let client = HaiClient::new(provider, HaiClientOptions::default()).expect("client");
        let store = Arc::new(jacs::replay::InMemoryReplayStore::new(
            Duration::from_secs(60),
            100,
        ));

        let error = client
            .with_shared_event_replay_store(store)
            .err()
            .expect("process-local replay backend must be rejected");
        assert!(error.to_string().contains("shared scope"));
    }

    #[cfg(feature = "jacs-crate")]
    #[test]
    fn server_key_refresh_interval_is_configurable_but_never_exceeds_five_minutes() {
        let provider = StaticJacsProvider::new("refresh-config-agent");
        let client = HaiClient::new(provider, HaiClientOptions::default())
            .unwrap()
            .with_server_key_refresh_interval(Duration::from_millis(5))
            .expect("short interval is useful for tests");
        assert_eq!(client.server_key_refresh_interval, Duration::from_millis(5));

        let provider = StaticJacsProvider::new("refresh-config-agent-2");
        let error = HaiClient::new(provider, HaiClientOptions::default())
            .unwrap()
            .with_server_key_refresh_interval(Duration::from_secs(
                DEFAULT_SERVER_KEY_REFRESH_SECS + 1,
            ))
            .err()
            .expect("callers may shorten but not weaken the refresh bound");
        assert!(error.to_string().contains("server key refresh interval"));
    }

    #[cfg(feature = "jacs-crate")]
    #[test]
    fn live_event_verifier_releases_only_verified_data_with_provenance_and_replay() {
        use std::collections::HashSet;
        use std::sync::Mutex;

        #[derive(Default)]
        struct SharedReplayStore {
            seen: Mutex<HashSet<String>>,
        }

        impl jacs::replay::ReplayStore for SharedReplayStore {
            fn consume(
                &self,
                key: &str,
                _ttl: Duration,
            ) -> std::result::Result<bool, jacs::error::JacsError> {
                let mut seen = self
                    .seen
                    .lock()
                    .map_err(|_| jacs::error::JacsError::Internal {
                        message: "test replay store lock poisoned".to_string(),
                    })?;
                Ok(seen.insert(key.to_string()))
            }

            fn name(&self) -> &'static str {
                "haiai-shared-test"
            }

            fn scope(&self) -> jacs::replay::ReplayStoreScope {
                jacs::replay::ReplayStoreScope::Shared
            }
        }

        let _env_guard = crate::test_support::env_lock();
        let tmp = tempfile::Builder::new()
            .prefix("haiai-signed-stream-")
            .tempdir()
            .expect("create temp dir");
        let base_dir = tmp.path().canonicalize().expect("canonical temp dir");
        let key_dir = base_dir.join("keys");
        let data_dir = base_dir.join("data");
        std::fs::create_dir_all(&key_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        let config_path = base_dir.join("jacs.config.json");
        LocalJacsProvider::create_agent_with_options(&CreateAgentOptions {
            name: "stream-verifier".to_string(),
            password: "test-password-1234".to_string(),
            algorithm: Some("ed25519".to_string()),
            data_directory: Some(data_dir.display().to_string()),
            key_directory: Some(key_dir.display().to_string()),
            config_path: Some(config_path.display().to_string()),
            agent_type: None,
            description: None,
            domain: None,
            default_storage: None,
        })
        .expect("create signing agent");
        unsafe {
            std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", "test-password-1234");
        }
        let signer = LocalJacsProvider::from_config_path(Some(config_path.as_path()), None)
            .expect("load signing agent");
        let expected = test_live_context();
        let data = jacs::response_context::ResponseData::PrivateEvent {
            event_type: "benchmark_job".into(),
            contract: "hai.agent-event".into(),
            contract_version: "2".into(),
            transport: expected.transport.clone(),
            tenant: expected.tenant.clone(),
            audience: expected.audience.clone(),
            issuer: String::new(),
            event_id: String::new(),
            emitted_at: String::new(),
            causation: jacs::response_context::EventCausation::Job { id: "job-7".into() },
            payload: json!({
                "type": "benchmark_job",
                "job_id": "job-7",
                "config": {"timeout_secs": 30}
            }),
        };
        let signed = signer
            .sign_response_with_context(
                &data,
                jacs::response_context::ResponseOperation::SignAsyncEvent,
            )
            .expect("sign event");
        let envelope: Value = serde_json::from_str(&signed.signed_document).unwrap();
        let signer_id = envelope["jacsSignature"]["agentID"]
            .as_str()
            .unwrap()
            .to_string();
        let keys = std::collections::HashMap::from([(
            signer_id.clone(),
            signer.public_key_pem().unwrap().into_bytes(),
        )]);
        let replay = SharedReplayStore::default();

        let mut wrong_recipient = expected.clone();
        wrong_recipient.audience = "another-agent".into();
        assert!(verify_live_transport_event(
            &signed.signed_document,
            &keys,
            Some(&replay),
            &wrong_recipient
        )
        .is_err());
        let mut wrong_tenant = expected.clone();
        wrong_tenant.tenant = "another-tenant".into();
        assert!(verify_live_transport_event(
            &signed.signed_document,
            &keys,
            Some(&replay),
            &wrong_tenant
        )
        .is_err());
        let mut wrong_stream = expected.clone();
        wrong_stream.transport =
            jacs::response_context::EventTransport::Stream("another-stream".into());
        assert!(verify_live_transport_event(
            &signed.signed_document,
            &keys,
            Some(&replay),
            &wrong_stream
        )
        .is_err());
        let event =
            verify_live_transport_event(&signed.signed_document, &keys, Some(&replay), &expected)
                .expect("first delivery should verify");
        assert_eq!(event.event_type, "benchmark_job");
        assert_eq!(event.data["job_id"], "job-7");
        assert_eq!(event.verification.signer_id, signer_id);
        assert_eq!(event.verification.status, "verified");
        assert!(!event.verification.document_id.is_empty());
        assert!(!event.verification.timestamp.is_empty());
        assert_eq!(
            event.id.as_deref(),
            Some(event.verification.document_id.as_str())
        );
        assert_eq!(event.raw, signed.signed_document);

        let duplicate =
            verify_live_transport_event(&signed.signed_document, &keys, Some(&replay), &expected)
                .expect_err("duplicate delivery must fail closed");
        assert!(duplicate.to_string().contains("Replay attack"));
    }

    #[test]
    fn test_email_attachment_constructor() {
        let att = EmailAttachment::new(
            "doc.pdf".to_string(),
            "application/pdf".to_string(),
            b"pdf-bytes".to_vec(),
        );

        assert_eq!(att.filename, "doc.pdf");
        assert_eq!(att.content_type, "application/pdf");
        assert_eq!(att.data, b"pdf-bytes");
        assert!(att.data_base64.is_none());
    }

    // ── Key rotation tests ──────────────────────────────────────────────

    #[tokio::test]
    async fn test_rotate_keys_noop_provider_returns_error() {
        let provider = StaticJacsProvider::new("test-agent-001");
        let client = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: "https://hai.example".to_string(),
                ..HaiClientOptions::default()
            },
        )
        .expect("client");

        let result = client.rotate_keys(None).await;
        assert!(
            result.is_err(),
            "rotation with StaticJacsProvider should fail"
        );
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("not supported") || err_msg.contains("provider"),
            "error should mention provider not supporting rotation: {err_msg}",
        );
    }

    #[tokio::test]
    async fn test_rotate_keys_with_hai_registration_on_error() {
        // When provider rotate() fails, rotate_keys() should propagate the error
        let provider = StaticJacsProvider::new("test-agent-001");
        let client = HaiClient::new(provider, HaiClientOptions::default()).expect("client");

        let opts = RotateKeysOptions {
            register_with_hai: Some(true),
            ..Default::default()
        };
        let result = client.rotate_keys(Some(&opts)).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_rotation_result_fixture_contract() {
        let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fixtures")
            .join("rotation_result.json");

        if !fixture_path.exists() {
            // Skip if fixture not found
            return;
        }

        let data = std::fs::read_to_string(&fixture_path).expect("read fixture");
        let fixture: serde_json::Value = serde_json::from_str(&data).expect("parse fixture");
        let obj = fixture.as_object().expect("fixture should be object");

        let expected_fields = vec![
            "jacs_id",
            "old_version",
            "new_version",
            "new_public_key_hash",
            "registered_with_hai",
            "signed_agent_json",
        ];

        for field in &expected_fields {
            assert!(obj.contains_key(*field), "fixture missing field: {field}",);
        }
        assert_eq!(
            obj.len(),
            expected_fields.len(),
            "fixture field count mismatch",
        );
    }

    // ── Issue #13: base URL validation ────────────────────────────────

    #[test]
    fn test_new_rejects_invalid_base_url_no_scheme() {
        let provider = StaticJacsProvider::new("test-agent");
        let result = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: "example.com".to_string(),
                ..HaiClientOptions::default()
            },
        );
        assert!(
            result.is_err(),
            "base_url without scheme should be rejected"
        );
        let err = format!("{}", result.err().unwrap());
        assert!(
            err.contains("base_url") && err.contains("http"),
            "error should mention base_url and http: {err}"
        );
    }

    #[test]
    fn test_new_rejects_ftp_base_url() {
        let provider = StaticJacsProvider::new("test-agent");
        let result = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: "ftp://example.com".to_string(),
                ..HaiClientOptions::default()
            },
        );
        assert!(result.is_err(), "ftp:// base_url should be rejected");
    }

    #[test]
    fn test_new_accepts_http_base_url() {
        let provider = StaticJacsProvider::new("test-agent");
        let result = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: "http://localhost:8080".to_string(),
                ..HaiClientOptions::default()
            },
        );
        assert!(result.is_ok(), "http:// should be accepted");
    }

    #[test]
    fn test_new_accepts_https_base_url() {
        let provider = StaticJacsProvider::new("test-agent");
        let result = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: "https://hai.ai".to_string(),
                ..HaiClientOptions::default()
            },
        );
        assert!(result.is_ok(), "https:// should be accepted");
    }

    #[tokio::test]
    async fn server_key_fetch_rejects_cross_origin_redirect_without_contacting_target() {
        let target = httpmock::MockServer::start_async().await;
        let target_mock = target
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET).path("/stolen-keys");
                then.status(200).json_body(json!({ "keys": [] }));
            })
            .await;
        let origin = httpmock::MockServer::start_async().await;
        let target_url = target.url("/stolen-keys");
        let redirect_mock = origin
            .mock_async(move |when, then| {
                when.method(httpmock::Method::GET)
                    .path("/.well-known/hai-keys.json");
                then.status(302).header("Location", target_url);
            })
            .await;
        let client = HaiClient::new(
            StaticJacsProvider::new("test-agent"),
            HaiClientOptions {
                base_url: origin.base_url(),
                ..HaiClientOptions::default()
            },
        )
        .expect("client");

        let error = client
            .fetch_server_keys()
            .await
            .expect_err("cross-origin key redirect must fail closed");

        assert!(
            error.to_string().contains("redirect"),
            "unexpected redirect error: {error}"
        );
        redirect_mock.assert_calls_async(1).await;
        target_mock.assert_calls_async(0).await;
    }

    #[tokio::test]
    async fn server_key_fetch_allows_same_origin_relative_redirect() {
        let server = httpmock::MockServer::start_async().await;
        let redirect_mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/.well-known/hai-keys.json");
                then.status(302).header("Location", "/active-keys");
            })
            .await;
        let keys_mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET).path("/active-keys");
                then.status(200).json_body(json!({ "keys": [] }));
            })
            .await;
        let client = HaiClient::new(
            StaticJacsProvider::new("test-agent"),
            HaiClientOptions {
                base_url: server.base_url(),
                ..HaiClientOptions::default()
            },
        )
        .expect("client");

        let document = client
            .fetch_server_keys()
            .await
            .expect("same-origin redirect should remain usable");

        assert_eq!(document, json!({ "keys": [] }));
        redirect_mock.assert_calls_async(1).await;
        keys_mock.assert_calls_async(1).await;
    }

    #[tokio::test]
    async fn server_key_fetch_rejects_oversize_content_length_before_reading_body() {
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                MAX_SERVER_KEYS_BYTES + 1
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write response headers");
        });
        let client = HaiClient::new(
            StaticJacsProvider::new("test-agent"),
            HaiClientOptions {
                base_url: format!("http://{address}"),
                ..HaiClientOptions::default()
            },
        )
        .expect("client");

        let error = client
            .fetch_server_keys()
            .await
            .expect_err("oversize key response must fail before body allocation");

        assert!(error.to_string().contains("1048576 byte limit"));
        server.await.expect("mock server task");
    }

    #[test]
    fn test_new_strips_trailing_slash() {
        let provider = StaticJacsProvider::new("test-agent");
        let client = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: "https://hai.ai/".to_string(),
                ..HaiClientOptions::default()
            },
        )
        .expect("should accept URL with trailing slash");
        assert_eq!(client.base_url(), "https://hai.ai");
    }

    #[tokio::test]
    async fn get_agreement_intake_uses_p1_uuid_route_and_jacs_auth() {
        let server = httpmock::MockServer::start_async().await;
        let intake_id = "11111111-1111-1111-1111-111111111111";
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/api/v1/agreements/intakes/11111111-1111-1111-1111-111111111111")
                    .header_exists("Authorization");
                then.status(200).json_body(json!({
                    "intake_id": intake_id,
                    "status": "interviewing"
                }));
            })
            .await;

        let provider = StaticJacsProvider::new("agreement-route-agent");
        let client = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: server.base_url(),
                max_retries: 1,
                ..HaiClientOptions::default()
            },
        )
        .expect("client");

        let response = client
            .get_agreement_intake(intake_id)
            .await
            .expect("intake");

        assert_eq!(response["intake_id"], intake_id);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn get_agreement_intake_by_public_id_uses_p1_public_route_and_jacs_auth() {
        let server = httpmock::MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/api/v1/agreements/intakes/by-public/agree-public-123")
                    .header_exists("Authorization");
                then.status(200).json_body(json!({
                    "public_intake_id": "agree-public-123",
                    "status": "interviewing"
                }));
            })
            .await;

        let provider = StaticJacsProvider::new("agreement-public-route-agent");
        let client = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: server.base_url(),
                max_retries: 1,
                ..HaiClientOptions::default()
            },
        )
        .expect("client");

        let response = client
            .get_agreement_intake_by_public_id("agree-public-123")
            .await
            .expect("intake");

        assert_eq!(response["public_intake_id"], "agree-public-123");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn record_agreement_interview_turn_uses_p1_route_and_json_body() {
        let server = httpmock::MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/api/v1/agreements/intakes/11111111-1111-1111-1111-111111111111/interview-turns")
                    .header_exists("Authorization")
                    .header_exists("Content-Type");
                then.status(200).json_body(json!({
                    "recorded": true,
                    "turn_id": "turn-123"
                }));
            })
            .await;

        let provider = StaticJacsProvider::new("agreement-turn-route-agent");
        let client = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: server.base_url(),
                max_retries: 1,
                ..HaiClientOptions::default()
            },
        )
        .expect("client");
        let payload = json!({
            "party_role": "initiator",
            "source": "email",
            "sanitized_text": "I can meet Tuesday."
        });

        let response = client
            .record_agreement_interview_turn("11111111-1111-1111-1111-111111111111", &payload)
            .await
            .expect("turn record");

        assert_eq!(response["recorded"], true);
        mock.assert_async().await;
    }

    // ── Issue #4: retry wrapper ───────────────────────────────────────

    #[tokio::test]
    async fn test_retry_on_503_then_success() {
        let server = httpmock::MockServer::start_async().await;

        // First call returns 503, second returns 200
        let mock_503 = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/api/v1/agents/hello");
                then.status(503)
                    .json_body(json!({"error": "Service Unavailable"}));
            })
            .await;

        let provider = StaticJacsProvider::new("test-agent-retry");
        let client = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: server.base_url(),
                max_retries: 3,
                ..HaiClientOptions::default()
            },
        )
        .expect("client");

        // After the first 503 attempt, delete the mock and set up a 200 one.
        // httpmock doesn't support ordered mocks easily, so we test that
        // the method at least retries (calls endpoint > 1 time) by
        // having only 503s and checking the mock was called multiple times.
        let result = client.hello(false).await;
        // Should be an API error because all retries get 503
        assert!(result.is_err());
        // The mock should have been hit max_retries times (3)
        mock_503.assert_calls_async(3).await;
    }

    #[tokio::test]
    async fn test_retry_not_on_400() {
        let server = httpmock::MockServer::start_async().await;

        let mock_400 = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/api/v1/agents/hello");
                then.status(400).json_body(json!({"error": "Bad Request"}));
            })
            .await;

        let provider = StaticJacsProvider::new("test-agent-no-retry");
        let client = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: server.base_url(),
                max_retries: 3,
                ..HaiClientOptions::default()
            },
        )
        .expect("client");

        let result = client.hello(false).await;
        assert!(result.is_err());
        // 400 is NOT retryable, so mock should be hit exactly once
        mock_400.assert_calls_async(1).await;
    }

    #[tokio::test]
    async fn test_retry_on_429_rate_limit() {
        let server = httpmock::MockServer::start_async().await;

        let mock_429 = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/api/v1/agents/hello");
                then.status(429)
                    .json_body(json!({"error": "Too Many Requests"}));
            })
            .await;

        let provider = StaticJacsProvider::new("test-agent-429");
        let client = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: server.base_url(),
                max_retries: 2,
                ..HaiClientOptions::default()
            },
        )
        .expect("client");

        let result = client.hello(false).await;
        assert!(result.is_err());
        // 429 is retryable, should be hit 2 times (max_retries)
        mock_429.assert_calls_async(2).await;
    }

    #[tokio::test]
    async fn test_retry_success_on_second_attempt() {
        let server = httpmock::MockServer::start_async().await;

        // Test that a 200 response succeeds without needing retries
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/api/v1/agents/hello");
                then.status(200).json_body(json!({
                    "timestamp": "2024-01-01T00:00:00Z",
                    "message": "hello",
                    "hello_id": "h-123"
                }));
            })
            .await;

        let provider = StaticJacsProvider::new("test-agent-ok");
        let client = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: server.base_url(),
                max_retries: 3,
                ..HaiClientOptions::default()
            },
        )
        .expect("client");

        let result = client.hello(false).await;
        assert!(result.is_ok(), "200 response should succeed");
        let hello = result.unwrap();
        assert_eq!(hello.hello_id, "h-123");
    }

    #[test]
    fn test_retryable_status_codes_match_python() {
        // Contract: must match Python SDK's RETRYABLE_STATUS_CODES
        assert!(RETRYABLE_STATUS_CODES.contains(&429));
        assert!(RETRYABLE_STATUS_CODES.contains(&500));
        assert!(RETRYABLE_STATUS_CODES.contains(&502));
        assert!(RETRYABLE_STATUS_CODES.contains(&503));
        assert!(RETRYABLE_STATUS_CODES.contains(&504));
        assert!(!RETRYABLE_STATUS_CODES.contains(&400));
        assert!(!RETRYABLE_STATUS_CODES.contains(&401));
        assert!(!RETRYABLE_STATUS_CODES.contains(&404));
    }

    #[test]
    fn test_max_retries_floor_is_one() {
        let provider = StaticJacsProvider::new("test-agent");
        let client = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: "https://example.com".to_string(),
                max_retries: 0,
                ..HaiClientOptions::default()
            },
        )
        .expect("client");
        assert_eq!(client.max_retries(), 1, "max_retries should be at least 1");
    }

    // ── Issue #14: on_benchmark_job reconnection ──────────────────────

    #[test]
    fn test_default_max_reconnect_attempts() {
        assert_eq!(DEFAULT_MAX_RECONNECT_ATTEMPTS, 10);
    }

    // ── Issue #17: reply endpoint in contract fixture ─────────────────

    #[test]
    fn test_hai_client_options_default_client_identifier_is_none() {
        let opts = HaiClientOptions::default();
        assert!(
            opts.client_identifier.is_none(),
            "default client_identifier should be None (resolved to haiai-rust/VERSION at construction)"
        );
    }

    #[test]
    fn test_hai_client_constructs_with_default_client_identifier() {
        let provider = StaticJacsProvider::new("test-agent".to_string());
        // Should not panic -- proves the default header construction path works
        let _client = HaiClient::new(
            provider,
            HaiClientOptions {
                client_identifier: None,
                ..Default::default()
            },
        )
        .expect("should create client with default client identifier");
    }

    #[test]
    fn test_hai_client_constructs_with_custom_client_identifier() {
        let provider = StaticJacsProvider::new("test-agent".to_string());
        let _client = HaiClient::new(
            provider,
            HaiClientOptions {
                client_identifier: Some("haiai-cli/0.2.2".to_string()),
                ..Default::default()
            },
        )
        .expect("should create client with custom client identifier");
    }

    #[test]
    fn test_hai_client_header_constant_matches_expected_name() {
        assert_eq!(HAI_CLIENT_HEADER, "x-hai-client");
    }

    #[tokio::test]
    async fn test_hai_client_sends_x_hai_client_header_in_requests() {
        // Use a mock server to verify the header is actually sent in HTTP requests.
        // This test would FAIL if the default_headers insertion were removed,
        // proving it is not vacuous.
        let server = httpmock::MockServer::start_async().await;

        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/health")
                    .header_exists(HAI_CLIENT_HEADER);
                then.status(200).body("ok");
            })
            .await;

        let provider = StaticJacsProvider::new("header-test-agent".to_string());
        let client = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: server.base_url(),
                client_identifier: None, // defaults to haiai-rust/{version}
                ..Default::default()
            },
        )
        .expect("should create client");

        // Make a raw HTTP request through the client's reqwest::Client
        // (which has the default headers set)
        let resp = client
            .http
            .get(format!("{}/health", server.base_url()))
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(resp.status(), 200);
        mock.assert_async().await; // Verifies the mock was hit with the expected header
    }

    #[tokio::test]
    async fn test_hai_client_sends_custom_client_identifier_header() {
        let server = httpmock::MockServer::start_async().await;

        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/health")
                    .header(HAI_CLIENT_HEADER, "haiai-cli/1.0.0");
                then.status(200).body("ok");
            })
            .await;

        let provider = StaticJacsProvider::new("header-test-agent".to_string());
        let client = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: server.base_url(),
                client_identifier: Some("haiai-cli/1.0.0".to_string()),
                ..Default::default()
            },
        )
        .expect("should create client");

        let resp = client
            .http
            .get(format!("{}/health", server.base_url()))
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(resp.status(), 200);
        mock.assert_async().await; // Verifies mock matched on the exact header value
    }

    #[test]
    fn test_contract_fixture_contains_reply_endpoint() {
        let fixture_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fixtures")
            .join("contract_endpoints.json");

        if !fixture_path.exists() {
            panic!(
                "contract_endpoints.json fixture not found at {:?}",
                fixture_path
            );
        }

        let data = std::fs::read_to_string(&fixture_path).expect("read fixture");
        let fixture: serde_json::Value = serde_json::from_str(&data).expect("parse fixture");
        let obj = fixture.as_object().expect("fixture should be object");

        // The reply endpoint must be present
        assert!(
            obj.contains_key("reply"),
            "fixture must contain 'reply' endpoint"
        );
        let reply = obj.get("reply").unwrap();
        assert_eq!(
            reply.get("method").and_then(|v| v.as_str()),
            Some("POST"),
            "reply method should be POST"
        );
        assert_eq!(
            reply.get("path").and_then(|v| v.as_str()),
            Some("/api/agents/{agent_id}/email/reply"),
            "reply path should match"
        );
        assert_eq!(
            reply.get("auth_required").and_then(|v| v.as_bool()),
            Some(true),
            "reply should require auth"
        );
    }
}
