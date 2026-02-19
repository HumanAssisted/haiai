use std::time::Duration;

use base64::Engine;
use reqwest::{Response, StatusCode};
use serde_json::{json, Value};
use time::OffsetDateTime;

use crate::error::{HaiError, Result};
use crate::jacs::JacsProvider;
use crate::types::{
    CheckUsernameResult, ClaimUsernameResult, EmailMessage, EmailStatus, HelloResult,
    JobResponseResult, ListMessagesOptions, PublicKeyInfo, RegisterAgentOptions,
    RegistrationResult, SendEmailOptions, SendEmailResult, VerifyAgentResult,
};

const DEFAULT_BASE_URL: &str = "https://hai.ai";

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

    pub fn build_auth_header(&self) -> Result<String> {
        let ts = OffsetDateTime::now_utc().unix_timestamp();
        let message = format!("{}:{ts}", self.jacs.jacs_id());
        let signature = self.jacs.sign_string(&message)?;
        Ok(format!("JACS {}:{ts}:{signature}", self.jacs.jacs_id()))
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

    pub async fn send_email(&self, options: &SendEmailOptions) -> Result<SendEmailResult> {
        let safe_jacs_id = encode_path_segment(self.jacs.jacs_id());
        let url = self.url(&format!("/api/agents/{safe_jacs_id}/email/send"));

        let response = self
            .http
            .post(url)
            .header("Authorization", self.build_auth_header()?)
            .header("Content-Type", "application/json")
            .json(options)
            .send()
            .await?;

        let data = response_json(response).await?;
        Ok(SendEmailResult {
            message_id: value_string(&data, &["message_id"]),
            status: value_string(&data, &["status"]),
        })
    }

    pub async fn list_messages(&self, options: &ListMessagesOptions) -> Result<Vec<EmailMessage>> {
        let safe_jacs_id = encode_path_segment(self.jacs.jacs_id());
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
        if let Some(folder) = options.folder.as_deref() {
            request = request.query(&[("folder", folder)]);
        }

        let response = request.send().await?;
        let data = response_json(response).await?;

        if data.is_array() {
            return Ok(serde_json::from_value(data)?);
        }

        let messages = data
            .get("messages")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        Ok(serde_json::from_value(messages)?)
    }

    pub async fn mark_read(&self, message_id: &str) -> Result<()> {
        let safe_jacs_id = encode_path_segment(self.jacs.jacs_id());
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
        let safe_jacs_id = encode_path_segment(self.jacs.jacs_id());
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
