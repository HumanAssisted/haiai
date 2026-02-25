use std::future::Future;
use std::time::Duration;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use reqwest::{Response, StatusCode};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, tungstenite};
use tungstenite::client::IntoClientRequest;

use crate::error::{HaiError, Result};
use crate::jacs::JacsProvider;
use crate::types::{
    AgentVerificationResult, CheckUsernameResult, ClaimUsernameResult, DeleteUsernameResult,
    DnsCertifiedResult, DnsCertifiedRunOptions, DocumentVerificationResult, EmailMessage,
    EmailStatus, FreeChaoticResult, HaiEvent, HelloResult, JobResponseResult, ListMessagesOptions,
    PublicKeyInfo, RegisterAgentOptions, RegistrationResult, SearchOptions, SendEmailOptions,
    SendEmailResult, TranscriptMessage, TransportType, UpdateUsernameResult,
    VerifyAgentDocumentRequest, VerifyAgentResult,
};

const DEFAULT_BASE_URL: &str = "https://hai.ai";

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

#[derive(Debug, Clone)]
pub struct HaiClientOptions {
    pub base_url: String,
    pub timeout: Duration,
    pub max_retries: usize,
}

impl Default for HaiClientOptions {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            timeout: Duration::from_secs(30),
            max_retries: 3,
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
}

impl<P: JacsProvider> HaiClient<P> {
    pub fn new(jacs: P, options: HaiClientOptions) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(options.timeout)
            .build()?;

        Ok(Self {
            base_url: options.base_url.trim_end_matches('/').to_string(),
            http,
            max_retries: options.max_retries.max(1),
            jacs,
            hai_agent_id: None,
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
        self.hai_agent_id.as_deref().unwrap_or_else(|| self.jacs.jacs_id())
    }

    /// Set the HAI-assigned agent UUID (from registration response).
    pub fn set_hai_agent_id(&mut self, id: String) {
        self.hai_agent_id = Some(id);
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

    pub async fn hello(&self, include_test: bool) -> Result<HelloResult> {
        let mut payload = json!({ "agent_id": self.jacs.jacs_id() });
        if include_test {
            payload["include_test"] = Value::Bool(true);
        }

        let url = self.url("/api/v1/agents/hello");
        let response = self
            .http
            .post(url)
            .header("Authorization", self.build_auth_header()?)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
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
        let response = self
            .http
            .get(url)
            .query(&[("username", username)])
            .send()
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

        let response = self
            .http
            .post(url)
            .header("Content-Type", "application/json")
            .json(&Value::Object(payload))
            .send()
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
        &self,
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
        Ok(ClaimUsernameResult {
            username: value_string(&data, &["username"]).if_empty_then(username),
            email: value_string(&data, &["email"]),
            agent_id: value_string(&data, &["agent_id", "agentId"]).if_empty_then(agent_id),
        })
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
        let safe_jacs_id = encode_path_segment(self.hai_agent_id());
        let url = self.url(&format!("/api/agents/{safe_jacs_id}/email/send"));

        let timestamp = OffsetDateTime::now_utc().unix_timestamp();

        let content_hash = {
            let mut hasher = Sha256::new();
            hasher.update(options.subject.as_bytes());
            hasher.update(b"\n");
            hasher.update(options.body.as_bytes());
            format!("sha256:{:x}", hasher.finalize())
        };

        let sign_input = format!("{content_hash}:{timestamp}");
        let jacs_signature = self.jacs.sign_string(&sign_input)?;

        let mut payload = json!({
            "to": options.to,
            "subject": options.subject,
            "body": options.body,
            "jacs_signature": jacs_signature,
            "jacs_timestamp": timestamp,
        });
        if let Some(ref in_reply_to) = options.in_reply_to {
            payload["in_reply_to"] = Value::String(in_reply_to.clone());
        }

        let response = self
            .http
            .post(url)
            .header("Authorization", self.build_auth_header()?)
            .header("Content-Type", "application/json")
            .json(&payload)
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

        let response = request.send().await?;
        let data = response_json(response).await?;

        let messages = data
            .get("messages")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        Ok(serde_json::from_value(messages)?)
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

    pub async fn reply(
        &self,
        message_id: &str,
        body: &str,
        subject_override: Option<&str>,
    ) -> Result<SendEmailResult> {
        let original = self.get_message(message_id).await?;

        let subject = match subject_override {
            Some(s) => s.to_string(),
            None => {
                if original.subject.starts_with("Re: ") {
                    original.subject.clone()
                } else {
                    format!("Re: {}", original.subject)
                }
            }
        };

        let options = SendEmailOptions {
            to: original.from_address,
            subject,
            body: body.to_string(),
            in_reply_to: original.message_id.clone().or(Some(original.id)),
        };

        self.send_email(&options).await
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
            .http
            .post(url)
            .header("Authorization", self.build_auth_header()?)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
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

    pub async fn dns_certified_run(
        &self,
        options: &DnsCertifiedRunOptions,
    ) -> Result<DnsCertifiedResult> {
        let purchase_url = self.url("/api/benchmark/purchase");
        let purchase_response = self
            .http
            .post(purchase_url)
            .header("Authorization", self.build_auth_header()?)
            .header("Content-Type", "application/json")
            .json(&json!({
                "tier": "dns_certified",
                "agent_id": self.jacs.jacs_id(),
            }))
            .send()
            .await?;
        let purchase_data = response_json(purchase_response).await?;
        let checkout_url = value_string(&purchase_data, &["checkout_url"]);
        if checkout_url.is_empty() {
            return Err(HaiError::Message(
                "dns_certified purchase did not return checkout_url".to_string(),
            ));
        }
        let payment_id = value_string(&purchase_data, &["payment_id"]);
        if payment_id.is_empty() {
            return Err(HaiError::Message(
                "dns_certified purchase did not return payment_id".to_string(),
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
                "name": format!("DNS Certified Run - {short_id}"),
                "tier": "dns_certified",
                "payment_id": payment_id,
                "transport": options.transport.as_str(),
            }))
            .send()
            .await?;

        let data = response_json(run_response).await?;
        Ok(DnsCertifiedResult {
            success: true,
            run_id: value_string(&data, &["run_id", "runId"]),
            score: data.get("score").and_then(Value::as_f64).unwrap_or(0.0),
            transcript: parse_transcript(&data),
            payment_id,
            raw_response: data,
        })
    }

    pub async fn certified_run(&self) -> Result<()> {
        Err(HaiError::Message(
            "the fully_certified tier ($499/month) is coming soon; contact support@hai.ai for early access".to_string(),
        ))
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

    pub async fn on_benchmark_job<F, Fut>(
        &self,
        transport: TransportType,
        mut handler: F,
    ) -> Result<()>
    where
        F: FnMut(Value) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        match transport {
            TransportType::Sse => {
                let mut conn = self.connect_sse().await?;
                while let Some(event) = conn.next_event().await {
                    match event.event_type.as_str() {
                        "benchmark_job" => handler(event.data).await?,
                        "disconnect" => break,
                        _ => {}
                    }
                }
                conn.close().await;
            }
            TransportType::Ws => {
                let mut conn = self.connect_ws().await?;
                while let Some(event) = conn.next_event().await {
                    match event.event_type.as_str() {
                        "benchmark_job" => handler(event.data).await?,
                        "disconnect" => break,
                        _ => {}
                    }
                }
                conn.close().await;
            }
        }
        Ok(())
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, normalize_path(path))
    }

    pub fn max_retries(&self) -> usize {
        self.max_retries
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
