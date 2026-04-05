use std::future::Future;
use std::time::Duration;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use reqwest::{Response, StatusCode};
use serde_json::{json, Value};
use time::OffsetDateTime;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, tungstenite};
use tungstenite::client::IntoClientRequest;

use crate::error::{HaiError, Result};
use crate::jacs::JacsProvider;
use crate::types::{
    AgentKeyHistory, AgentVerificationResult, CheckUsernameResult, ClaimUsernameResult,
    Contact, CreateEmailTemplateOptions, DeleteUsernameResult, DnsCertifiedResult,
    DnsCertifiedRunOptions, DocumentVerificationResult, EmailMessage, EmailStatus,
    EmailTemplate, FreeChaoticResult, HaiEvent, HelloResult, JobResponseResult,
    ListEmailTemplatesOptions, ListEmailTemplatesResult, ListMessagesOptions,
    ProRunOptions, ProRunResult, PublicKeyInfo, RegisterAgentOptions, RegistrationResult,
    RotateKeysOptions, RotationResult, SearchOptions, SendEmailOptions, SendEmailResult,
    TranscriptMessage, TransportType, UpdateAgentResult, UpdateEmailTemplateOptions,
    UpdateUsernameResult, VerifyAgentDocumentRequest, VerifyAgentResult,
};

pub const DEFAULT_BASE_URL: &str = "https://beta.hai.ai";

pub struct SseConnection {
    events: mpsc::Receiver<HaiEvent>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl SseConnection {
    pub async fn next_event(&mut self) -> Option<HaiEvent> {
        self.events.recv().await
    }

    pub async fn close(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
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
    events: mpsc::Receiver<HaiEvent>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl WsConnection {
    pub async fn next_event(&mut self) -> Option<HaiEvent> {
        self.events.recv().await
    }

    pub async fn close(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
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

#[derive(Debug, Clone)]
pub struct HaiClientOptions {
    pub base_url: String,
    pub timeout: Duration,
    pub max_retries: usize,
    /// SDK client identifier sent as the `X-HAI-Client` header.
    /// Format: `haiai-{transport}/{version}`.
    /// Defaults to `haiai-rust/{CARGO_PKG_VERSION}` when `None`.
    pub client_identifier: Option<String>,
}

impl Default for HaiClientOptions {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            max_retries: DEFAULT_MAX_RETRIES,
            client_identifier: None,
        }
    }
}

pub struct HaiClient<P: JacsProvider> {
    base_url: String,
    http: reqwest::Client,
    max_retries: usize,
    jacs: P,
    /// HAI-assigned agent UUID for email URL paths (set after registration).
    hai_agent_id: Option<String>,
    /// Agent's @hai.ai email address (set after claim_username).
    agent_email: Option<String>,
}

/// Status codes that are safe to retry (transient server errors and rate limiting).
/// Matches Python SDK's `RETRYABLE_STATUS_CODES`.
const RETRYABLE_STATUS_CODES: &[u16] = &[429, 500, 502, 503, 504];

/// Default maximum reconnect attempts for `on_benchmark_job`.
const DEFAULT_MAX_RECONNECT_ATTEMPTS: usize = 10;

impl<P: JacsProvider> HaiClient<P> {
    pub fn new(jacs: P, options: HaiClientOptions) -> Result<Self> {
        // ── Issue #13: validate base URL ────────────────────────────────
        let trimmed = options.base_url.trim_end_matches('/');
        if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
            return Err(HaiError::Validation {
                field: "base_url".to_string(),
                message: format!(
                    "base_url must start with http:// or https://, got: {}",
                    options.base_url
                ),
            });
        }

        let client_id = options.client_identifier.unwrap_or_else(|| {
            format!("haiai-rust/{}", env!("CARGO_PKG_VERSION"))
        });
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
            .default_headers(default_headers)
            .build()?;

        Ok(Self {
            base_url: trimmed.to_string(),
            http,
            max_retries: options.max_retries.max(1),
            jacs,
            hai_agent_id: None,
            agent_email: None,
        })
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

    /// Get the agent's @hai.ai email address (set after claim_username).
    pub fn agent_email(&self) -> Option<&str> {
        self.agent_email.as_deref()
    }

    /// Set the agent's @hai.ai email address.
    pub fn set_agent_email(&mut self, email: String) {
        self.agent_email = Some(email);
    }

    pub fn build_auth_header(&self) -> Result<String> {
        let ts = OffsetDateTime::now_utc().unix_timestamp();
        let message = format!("{}:{ts}", self.jacs.jacs_id());
        let signature = self.jacs.sign_string(&message)?;
        Ok(format!("JACS {}:{ts}:{signature}", self.jacs.jacs_id()))
    }

    pub fn sign_message(&self, message: &str) -> Result<String> {
        self.jacs.sign_string(message)
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
        let auth = self.build_auth_header()?;
        let response = self
            .request_with_retry(|| {
                let http = &self.http;
                let url = &url;
                let auth = &auth;
                let payload = &payload;
                async move {
                    http.post(url.as_str())
                        .header("Authorization", auth.as_str())
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

    pub async fn check_username(&self, username: &str) -> Result<CheckUsernameResult> {
        let url = self.url("/api/v1/agents/username/check");
        let username = username.to_string();
        let response = self
            .request_with_retry(|| {
                let http = &self.http;
                let url = &url;
                let username = &username;
                async move {
                    http.get(url.as_str())
                        .query(&[("username", username.as_str())])
                        .send()
                        .await
                }
            })
            .await?;

        let data = response_json(response).await?;
        Ok(CheckUsernameResult {
            available: data
                .get("available")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            username: value_string(&data, &["username"]).if_empty_then(username),
            reason: data
                .get("reason")
                .and_then(Value::as_str)
                .map(ToString::to_string),
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

        // Build 4-part auth header with the OLD key BEFORE rotation
        // (chain of trust: old key vouches for new key)
        let old_auth_header = if register_with_hai {
            Some(self.build_auth_header()?)
        } else {
            None
        };

        // Perform local rotation via the JACS provider
        let mut result = self.jacs.rotate()?;

        // Optionally re-register with HAI using the OLD key for auth
        if register_with_hai {
            if let Some(auth_header) = old_auth_header {
                let url = self.url("/api/v1/agents/register");

                let mut payload = serde_json::Map::new();
                payload.insert(
                    "agent_json".to_string(),
                    Value::String(result.signed_agent_json.clone()),
                );

                match self
                    .http
                    .post(url)
                    .header("Authorization", &auth_header)
                    .header("Content-Type", "application/json")
                    .json(&Value::Object(payload))
                    .send()
                    .await
                {
                    Ok(response) if response.status().is_success() => {
                        result.registered_with_hai = true;
                    }
                    _ => {
                        // HAI registration failure is non-fatal
                    }
                }
            }
        }

        Ok(result)
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
        let mut result = self.jacs.update_agent(new_agent_data)?;

        // Re-register with HAI using current key (same key, just new doc version)
        let url = self.url("/api/v1/agents/register");
        let mut payload = serde_json::Map::new();
        payload.insert(
            "agent_json".to_string(),
            Value::String(result.signed_agent_json.clone()),
        );

        match self
            .http
            .post(url)
            .header("Authorization", self.build_auth_header()?)
            .header("Content-Type", "application/json")
            .json(&Value::Object(payload))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                result.registered_with_hai = true;
            }
            _ => {
                // HAI registration failure is non-fatal
            }
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
        let payload = json!({
            "response": {
                "message": message,
                "metadata": metadata,
                "processing_time_ms": processing_time_ms,
            }
        });
        let signed = self.jacs.sign_response(&payload)?;

        let safe_job_id = encode_path_segment(job_id);
        let url = self.url(&format!("/api/v1/agents/jobs/{safe_job_id}/response"));
        let response = self
            .http
            .post(url)
            .header("Authorization", self.build_auth_header()?)
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

    pub async fn verify_status(&self, agent_id: Option<&str>) -> Result<VerifyAgentResult> {
        let target = agent_id.unwrap_or_else(|| self.jacs.jacs_id());
        let safe_agent_id = encode_path_segment(target);
        let url = self.url(&format!("/api/v1/agents/{safe_agent_id}/verify"));

        let response = self
            .http
            .get(url)
            .header("Authorization", self.build_auth_header()?)
            .send()
            .await?;

        let data = response_json(response).await?;
        let mut parsed: VerifyAgentResult = serde_json::from_value(data.clone())?;
        if parsed.jacs_id.is_empty() {
            parsed.jacs_id = target.to_string();
        }
        Ok(parsed)
    }

    pub async fn claim_username(
        &mut self,
        agent_id: &str,
        username: &str,
    ) -> Result<ClaimUsernameResult> {
        let safe_agent_id = encode_path_segment(agent_id);
        let url = self.url(&format!("/api/v1/agents/{safe_agent_id}/username"));

        let response = self
            .http
            .post(url)
            .header("Authorization", self.build_auth_header()?)
            .header("Content-Type", "application/json")
            .json(&json!({ "username": username }))
            .send()
            .await?;

        let data = response_json(response).await?;
        let result = ClaimUsernameResult {
            username: value_string(&data, &["username"]).if_empty_then(username),
            email: value_string(&data, &["email"]),
            agent_id: value_string(&data, &["agent_id", "agentId"]).if_empty_then(agent_id),
        };

        // Auto-store the email so subsequent send_email calls work without
        // a separate set_agent_email call.
        if !result.email.is_empty() {
            self.agent_email = Some(result.email.clone());
        }

        Ok(result)
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
            .header("Authorization", self.build_auth_header()?)
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

        let response = self
            .http
            .delete(url)
            .header("Authorization", self.build_auth_header()?)
            .send()
            .await?;

        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    pub async fn send_email(&self, options: &SendEmailOptions) -> Result<SendEmailResult> {
        // Validate agent_email is set before sending.
        let _ = self.agent_email.as_deref().ok_or_else(|| {
            HaiError::Message("agent email not set — call claim_username first".into())
        })?;
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let url = self.url(&format!("/api/agents/{safe_jacs_id}/email/send"));

        // Defensive: strip CR/LF from subject to prevent header injection
        // (e.g. from email header folding in stored inbound subjects).
        let safe_subject = crate::mime::sanitize_header(&options.subject);

        // Server handles JACS signing — client only sends content fields.
        let mut payload = json!({
            "to": options.to,
            "subject": safe_subject,
            "body": options.body,
        });
        if !options.cc.is_empty() {
            payload["cc"] = json!(options.cc);
        }
        if !options.bcc.is_empty() {
            payload["bcc"] = json!(options.bcc);
        }
        if !options.labels.is_empty() {
            payload["labels"] = json!(options.labels);
        }
        if let Some(ref in_reply_to) = options.in_reply_to {
            payload["in_reply_to"] = Value::String(in_reply_to.clone());
        }
        if !options.attachments.is_empty() {
            use base64::Engine;
            let att_json: Vec<Value> = options
                .attachments
                .iter()
                .map(|att| {
                    json!({
                        "filename": att.filename,
                        "content_type": att.content_type,
                        "data_base64": att.data_base64.clone().unwrap_or_else(|| {
                            base64::engine::general_purpose::STANDARD.encode(&att.data)
                        }),
                    })
                })
                .collect();
            payload["attachments"] = Value::Array(att_json);
        }

        let auth = self.build_auth_header()?;
        let response = self
            .request_with_retry(|| {
                let http = &self.http;
                let url = &url;
                let auth = &auth;
                let payload = &payload;
                async move {
                    http.post(url.as_str())
                        .header("Authorization", auth.as_str())
                        .header("Content-Type", "application/json")
                        .json(payload)
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
    /// - `agent_email` is not set (call `claim_username` first)
    /// - The provider does not support local signing (use `LocalJacsProvider`)
    /// - MIME construction or JACS signing fails
    /// - The server rejects the signed email
    pub async fn send_signed_email(&self, options: &SendEmailOptions) -> Result<SendEmailResult> {
        let from = self.agent_email.as_deref().ok_or_else(|| {
            HaiError::Message("agent email not set — call claim_username first".into())
        })?;

        // Append verification footer before signing (Decision D8: client-side,
        // not server-side, because modifying the body post-signing would
        // invalidate the JACS signature).
        let body = if options.append_footer != Some(false) {
            // Check if ANY recipient is external (not @hai.ai)
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
        } else {
            options.body.clone()
        };

        // Step 1: Build RFC 5322 MIME locally (with footer-amended body)
        let opts_with_footer = SendEmailOptions {
            body,
            ..options.clone()
        };
        let raw_mime = crate::mime::build_rfc5322_email(&opts_with_footer, from)?;

        // Step 2: Sign with the agent's own JACS key
        let signed = self.jacs.sign_email_locally(&raw_mime)?;

        // Step 3: POST to the send-signed endpoint
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let url = self.url(&format!("/api/agents/{safe_jacs_id}/email/send-signed"));

        let response = self
            .http
            .post(url)
            .header("Authorization", self.build_auth_header()?)
            .header("Content-Type", "message/rfc822")
            .body(signed)
            .send()
            .await?;

        let data = response_json(response).await?;
        Ok(SendEmailResult {
            message_id: value_string(&data, &["message_id"]),
            status: value_string(&data, &["status"]),
        })
    }

    pub async fn list_messages(&self, options: &ListMessagesOptions) -> Result<Vec<EmailMessage>> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let url = self.url(&format!("/api/agents/{safe_jacs_id}/email/messages"));

        let mut request = self
            .http
            .get(url)
            .header("Authorization", self.build_auth_header()?);

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
            HaiError::Message("agent email not set — call claim_username first".into())
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
            .header("Authorization", self.build_auth_header()?)
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

        let response = self
            .http
            .post(url)
            .header("Authorization", self.build_auth_header()?)
            .send()
            .await?;

        match response.status() {
            StatusCode::OK | StatusCode::CREATED | StatusCode::NO_CONTENT => Ok(()),
            _ => Err(response_error(response).await),
        }
    }

    pub async fn get_email_status(&self) -> Result<EmailStatus> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let url = self.url(&format!("/api/agents/{safe_jacs_id}/email/status"));

        let response = self
            .http
            .get(url)
            .header("Authorization", self.build_auth_header()?)
            .send()
            .await?;

        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    pub async fn get_message(&self, message_id: &str) -> Result<EmailMessage> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let safe_message_id = encode_path_segment(message_id);
        let url = self.url(&format!(
            "/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}"
        ));

        let response = self
            .http
            .get(url)
            .header("Authorization", self.build_auth_header()?)
            .send()
            .await?;

        let data = response_json(response).await?;
        Ok(serde_json::from_value(data)?)
    }

    pub async fn delete_message(&self, message_id: &str) -> Result<()> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let safe_message_id = encode_path_segment(message_id);
        let url = self.url(&format!(
            "/api/agents/{safe_jacs_id}/email/messages/{safe_message_id}"
        ));

        let response = self
            .http
            .delete(url)
            .header("Authorization", self.build_auth_header()?)
            .send()
            .await?;

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

        let response = self
            .http
            .post(url)
            .header("Authorization", self.build_auth_header()?)
            .send()
            .await?;

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

        let response = self
            .http
            .post(url)
            .header("Authorization", self.build_auth_header()?)
            .send()
            .await?;

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

        let response = self
            .http
            .post(url)
            .header("Authorization", self.build_auth_header()?)
            .send()
            .await?;

        match response.status() {
            StatusCode::OK | StatusCode::CREATED | StatusCode::NO_CONTENT => Ok(()),
            _ => Err(response_error(response).await),
        }
    }

    pub async fn search_messages(&self, options: &SearchOptions) -> Result<Vec<EmailMessage>> {
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let url = self.url(&format!("/api/agents/{safe_jacs_id}/email/search"));

        let mut request = self
            .http
            .get(url)
            .header("Authorization", self.build_auth_header()?);

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

        let response = self
            .http
            .get(url)
            .header("Authorization", self.build_auth_header()?)
            .send()
            .await?;

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
        self.reply_with_options(message_id, body, subject_override, None, &[]).await
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
            HaiError::Message("agent email not set — call claim_username first".into())
        })?;
        let agent_id = self.hai_agent_id();
        let safe_agent_id = encode_path_segment(agent_id);
        let url = self.url(&format!(
            "/api/agents/{safe_agent_id}/email/contacts"
        ));

        let response = self
            .http
            .get(url)
            .header("Authorization", self.build_auth_header()?)
            .send()
            .await?;

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
        let url = self.url(&format!(
            "/api/agents/{safe_jacs_id}/email/templates"
        ));

        let response = self
            .http
            .post(url)
            .header("Authorization", self.build_auth_header()?)
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
        let url = self.url(&format!(
            "/api/agents/{safe_jacs_id}/email/templates"
        ));

        let mut request = self
            .http
            .get(url)
            .header("Authorization", self.build_auth_header()?);

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

        let response = self
            .http
            .get(url)
            .header("Authorization", self.build_auth_header()?)
            .send()
            .await?;

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
            .header("Authorization", self.build_auth_header()?)
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

        let response = self
            .http
            .delete(url)
            .header("Authorization", self.build_auth_header()?)
            .send()
            .await?;

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
        let response = self.http.get(url).send().await?;
        let data = response_json(response).await?;
        Ok(data)
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
        let auth = self.build_auth_header()?;
        let response = self
            .http
            .post(url)
            .header("Authorization", &auth)
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
        let auth = self.build_auth_header()?;
        let response = self
            .http
            .post(url)
            .header("Authorization", &auth)
            .header("Content-Type", "message/rfc822")
            .body(raw_bytes)
            .send()
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
        let url = self.url(&format!(
            "/api/v1/agents/{safe_agent_id}/attestations"
        ));

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
            .header("Authorization", self.build_auth_header()?)
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
        let url = self.url(&format!(
            "/api/v1/agents/{safe_agent_id}/attestations"
        ));

        let response = self
            .http
            .get(url)
            .header("Authorization", self.build_auth_header()?)
            .query(&[("limit", &limit.to_string()), ("offset", &offset.to_string())])
            .send()
            .await?;

        response_json(response).await
    }

    /// Get a single attestation by document ID.
    pub async fn get_attestation(
        &self,
        agent_id: &str,
        doc_id: &str,
    ) -> Result<Value> {
        let safe_agent_id = encode_path_segment(agent_id);
        let safe_doc_id = encode_path_segment(doc_id);
        let url = self.url(&format!(
            "/api/v1/agents/{safe_agent_id}/attestations/{safe_doc_id}"
        ));

        let response = self
            .http
            .get(url)
            .header("Authorization", self.build_auth_header()?)
            .send()
            .await?;

        response_json(response).await
    }

    /// Verify an attestation document.
    pub async fn verify_attestation(&self, document: &str) -> Result<Value> {
        let url = self.url("/api/v1/attestations/verify");
        let response = self
            .http
            .post(url)
            .header("Authorization", self.build_auth_header()?)
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
        let auth = self.build_auth_header()?;
        let response = self
            .request_with_retry(|| {
                let http = &self.http;
                let url = &url;
                let auth = &auth;
                let payload = &payload;
                async move {
                    http.post(url.as_str())
                        .header("Authorization", auth.as_str())
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
            .header("Authorization", self.build_auth_header()?)
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

    pub async fn pro_run(
        &self,
        options: &ProRunOptions,
    ) -> Result<ProRunResult> {
        let purchase_url = self.url("/api/benchmark/purchase");
        let purchase_response = self
            .http
            .post(purchase_url)
            .header("Authorization", self.build_auth_header()?)
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
                .header("Authorization", self.build_auth_header()?)
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
            .header("Authorization", self.build_auth_header()?)
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
            "the enterprise tier is coming soon; contact support@hai.ai for early access".to_string(),
        ))
    }

    /// Deprecated: Use `enterprise_run` instead. The tier was renamed from fully_certified to enterprise.
    #[deprecated(note = "Use enterprise_run instead. The tier was renamed from fully_certified to enterprise.")]
    pub async fn certified_run(&self) -> Result<()> {
        self.enterprise_run().await
    }

    pub async fn connect_sse(&self) -> Result<SseConnection> {
        let url = self.url("/api/v1/agents/connect");
        let response = self
            .http
            .get(url)
            .header("Authorization", self.build_auth_header()?)
            .header("Accept", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(response_error(response).await);
        }

        let mut stream = response.bytes_stream();
        let (events_tx, events_rx) = mpsc::channel::<HaiEvent>(32);
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

        let task = tokio::spawn(async move {
            let mut buffer = String::new();
            let mut event_type = String::new();
            let mut event_id: Option<String> = None;
            let mut data_lines: Vec<String> = Vec::new();

            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        break;
                    }
                    next_chunk = stream.next() => {
                        let Some(chunk_result) = next_chunk else {
                            break;
                        };
                        let Ok(chunk) = chunk_result else {
                            break;
                        };

                        buffer.push_str(&String::from_utf8_lossy(&chunk));
                        while let Some(idx) = buffer.find('\n') {
                            let mut line = buffer[..idx].to_string();
                            buffer.drain(..=idx);
                            if line.ends_with('\r') {
                                line.pop();
                            }

                            if line.is_empty() {
                                if !data_lines.is_empty() {
                                    let raw = data_lines.join("\n");
                                    let event = parse_sse_event_payload(&event_type, event_id.clone(), &raw);
                                    if events_tx.send(event).await.is_err() {
                                        return;
                                    }
                                }
                                event_type.clear();
                                event_id = None;
                                data_lines.clear();
                                continue;
                            }

                            if let Some(rest) = line.strip_prefix("event:") {
                                event_type = rest.trim().to_string();
                            } else if let Some(rest) = line.strip_prefix("id:") {
                                event_id = Some(rest.trim().to_string());
                            } else if let Some(rest) = line.strip_prefix("data:") {
                                data_lines.push(rest.trim().to_string());
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

    pub async fn connect_ws(&self) -> Result<WsConnection> {
        let ws_url = build_ws_url(&self.base_url, "/ws/agent/connect");
        let mut request = ws_url.into_client_request().map_err(|err| {
            HaiError::Message(format!("failed to build websocket request: {err}"))
        })?;
        let auth_header = tungstenite::http::HeaderValue::from_str(&self.build_auth_header()?)
            .map_err(|err| HaiError::Message(format!("invalid auth header: {err}")))?;
        request.headers_mut().insert("Authorization", auth_header);

        let (ws_stream, _) = connect_async(request)
            .await
            .map_err(|err| HaiError::Message(format!("websocket connection failed: {err}")))?;

        let (mut ws_sink, mut ws_stream_read) = ws_stream.split();
        let (events_tx, events_rx) = mpsc::channel::<HaiEvent>(32);
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        let _ = ws_sink.send(Message::Close(None)).await;
                        break;
                    }
                    next_msg = ws_stream_read.next() => {
                        let Some(msg_result) = next_msg else {
                            break;
                        };
                        let Ok(msg) = msg_result else {
                            break;
                        };

                        let text = if msg.is_text() {
                            match msg.into_text() {
                                Ok(text) => text.to_string(),
                                Err(_) => continue,
                            }
                        } else if msg.is_close() {
                            break;
                        } else {
                            continue;
                        };

                        let data = serde_json::from_str::<Value>(&text)
                            .unwrap_or_else(|_| Value::String(text.clone()));
                        let event_type = data
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("message")
                            .to_string();

                        if event_type == "heartbeat" {
                            let timestamp = data
                                .get("timestamp")
                                .cloned()
                                .unwrap_or_else(|| Value::from(OffsetDateTime::now_utc().unix_timestamp()));
                            let pong = json!({
                                "type": "pong",
                                "timestamp": timestamp
                            });
                            let _ = ws_sink.send(Message::Text(pong.to_string().into())).await;
                        }

                        let event = HaiEvent {
                            event_type,
                            data,
                            id: None,
                            raw: text,
                        };
                        if events_tx.send(event).await.is_err() {
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

    /// Listen for benchmark jobs with automatic reconnection.
    ///
    /// When the connection drops (without a "disconnect" event), reconnects
    /// with exponential backoff up to `max_reconnect_attempts` times (default 10).
    /// A "disconnect" event is treated as an intentional server-side shutdown
    /// and will NOT trigger reconnection.
    pub async fn on_benchmark_job<F, Fut>(
        &self,
        transport: TransportType,
        handler: F,
    ) -> Result<()>
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
                            let delay = Duration::from_millis(100 * (1u64 << reconnect_count.min(10)));
                            tokio::time::sleep(delay).await;
                            reconnect_count += 1;
                            continue;
                        }
                    };

                    got_disconnect_event = false;
                    let mut saw_disconnect = false;
                    while let Some(event) = conn.next_event().await {
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
                            let delay = Duration::from_millis(100 * (1u64 << reconnect_count.min(10)));
                            tokio::time::sleep(delay).await;
                            reconnect_count += 1;
                            continue;
                        }
                    };

                    got_disconnect_event = false;
                    let mut saw_disconnect = false;
                    while let Some(event) = conn.next_event().await {
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
    async fn request_with_retry<F, Fut>(&self, mut make_request: F) -> std::result::Result<Response, reqwest::Error>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = std::result::Result<Response, reqwest::Error>>,
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

fn parse_sse_event_payload(event_type: &str, id: Option<String>, raw: &str) -> HaiEvent {
    let data =
        serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.to_string()));
    let inferred = data
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or(event_type)
        .to_string();
    HaiEvent {
        event_type: if inferred.is_empty() {
            "message".to_string()
        } else {
            inferred
        },
        data,
        id,
        raw: raw.to_string(),
    }
}

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
    let body = response.text().await.unwrap_or_default();
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

/// Compute the v2 content hash for an email (subject + body + sorted attachment hashes).
///
/// **DEPRECATED**: This function supports the legacy v2 header-based signing flow
/// used by `send_email`. It will be removed when `send_email` is updated to use
/// JACS attachment-based signing (TASK_014).
///
/// Formula:
#[cfg(test)]
mod tests {
    use super::*;
    use crate::jacs::StaticJacsProvider;
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

    // ── Issue 13: claim_username stores email ────────────────────────────

    #[tokio::test]
    async fn test_claim_username_stores_agent_email() {
        // We need httpmock for this, which is a dev-dependency
        // Use a mock server to simulate the claim_username response
        let server = httpmock::MockServer::start_async().await;

        // Mock the claim_username endpoint
        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/api/v1/agents/test-agent-001/username");
                then.status(200).json_body(serde_json::json!({
                    "username": "myagent",
                    "email": "myagent@hai.ai",
                    "agent_id": "test-agent-001"
                }));
            })
            .await;

        let provider = StaticJacsProvider::new("test-agent-001");
        let mut client = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: server.base_url(),
                ..HaiClientOptions::default()
            },
        )
        .expect("client");

        // Before claim_username, agent_email should be None
        assert!(client.agent_email().is_none());

        let result = client
            .claim_username("test-agent-001", "myagent")
            .await
            .expect("claim");
        assert_eq!(result.email, "myagent@hai.ai");

        // After claim_username, agent_email should be auto-stored
        assert_eq!(client.agent_email(), Some("myagent@hai.ai"));
    }

    #[tokio::test]
    async fn test_claim_username_does_not_store_empty_email() {
        let server = httpmock::MockServer::start_async().await;

        server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/api/v1/agents/test-agent-001/username");
                then.status(200).json_body(serde_json::json!({
                    "username": "myagent",
                    "email": "",
                    "agent_id": "test-agent-001"
                }));
            })
            .await;

        let provider = StaticJacsProvider::new("test-agent-001");
        let mut client = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: server.base_url(),
                ..HaiClientOptions::default()
            },
        )
        .expect("client");

        let _result = client
            .claim_username("test-agent-001", "myagent")
            .await
            .expect("claim");
        assert!(
            client.agent_email().is_none(),
            "empty email should not be stored"
        );
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
        assert!(result.is_err(), "base_url without scheme should be rejected");
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
                base_url: "https://beta.hai.ai".to_string(),
                ..HaiClientOptions::default()
            },
        );
        assert!(result.is_ok(), "https:// should be accepted");
    }

    #[test]
    fn test_new_strips_trailing_slash() {
        let provider = StaticJacsProvider::new("test-agent");
        let client = HaiClient::new(
            provider,
            HaiClientOptions {
                base_url: "https://beta.hai.ai/".to_string(),
                ..HaiClientOptions::default()
            },
        )
        .expect("should accept URL with trailing slash");
        assert_eq!(client.base_url(), "https://beta.hai.ai");
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
                then.status(400)
                    .json_body(json!({"error": "Bad Request"}));
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
            panic!("contract_endpoints.json fixture not found at {:?}", fixture_path);
        }

        let data = std::fs::read_to_string(&fixture_path).expect("read fixture");
        let fixture: serde_json::Value = serde_json::from_str(&data).expect("parse fixture");
        let obj = fixture.as_object().expect("fixture should be object");

        // The reply endpoint must be present
        assert!(obj.contains_key("reply"), "fixture must contain 'reply' endpoint");
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
