use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::client::HaiClient;
use crate::error::{HaiError, Result};
use crate::jacs::JacsProvider;
use crate::types::{
    RegisterAgentOptions, RegistrationResult, SendEmailOptions, SendEmailResult, TransportType,
};

pub const A2A_PROTOCOL_VERSION_04: &str = "0.4.0";
pub const A2A_PROTOCOL_VERSION_10: &str = "1.0";
pub const A2A_JACS_EXTENSION_URI: &str = "urn:jacs:provenance-v1";

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum A2ATrustPolicy {
    Open,
    #[default]
    Verified,
    Strict,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct A2AAgentInterface {
    pub url: String,
    pub protocol_binding: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct A2AAgentExtension {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct A2AAgentCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub streaming: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub push_notifications: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extended_agent_card: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<A2AAgentExtension>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct A2AAgentSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_modes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_modes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct A2AAgentCard {
    pub name: String,
    pub description: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protocol_versions: Vec<String>,
    pub supported_interfaces: Vec<A2AAgentInterface>,
    pub default_input_modes: Vec<String>,
    pub default_output_modes: Vec<String>,
    pub capabilities: A2AAgentCapabilities,
    pub skills: Vec<A2AAgentSkill>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct A2AArtifactSignature {
    #[serde(rename = "agentID")]
    pub agent_id: String,
    pub date: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct A2AWrappedArtifact {
    pub jacs_id: String,
    pub jacs_version: String,
    pub jacs_type: String,
    pub jacs_level: String,
    pub jacs_version_date: String,
    pub a2a_artifact: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jacs_parent_signatures: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jacs_signature: Option<A2AArtifactSignature>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct A2AArtifactVerificationResult {
    pub valid: bool,
    pub signer_id: String,
    pub artifact_type: String,
    pub timestamp: String,
    pub original_artifact: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct A2ATrustAssessment {
    pub allowed: bool,
    pub trust_level: String,
    pub jacs_registered: bool,
    pub in_trust_store: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct A2AChainEntry {
    pub artifact_id: String,
    pub artifact_type: String,
    pub timestamp: String,
    pub agent_id: String,
    pub signature_present: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct A2AChainOfCustody {
    pub chain_of_custody: Vec<A2AChainEntry>,
    pub created: String,
    pub total_artifacts: usize,
}

#[derive(Debug, Clone)]
pub struct A2AMediatedJobOptions {
    pub transport: TransportType,
    pub notify_email: Option<String>,
    pub email_subject: Option<String>,
    pub verify_inbound_artifact: bool,
    pub enforce_trust_policy: bool,
    pub max_reconnect_attempts: usize,
}

impl Default for A2AMediatedJobOptions {
    fn default() -> Self {
        Self {
            transport: TransportType::Sse,
            notify_email: None,
            email_subject: None,
            verify_inbound_artifact: false,
            enforce_trust_policy: false,
            max_reconnect_attempts: 0,
        }
    }
}

pub struct A2AIntegration<'a, P: JacsProvider> {
    client: &'a HaiClient<P>,
    trust_policy: A2ATrustPolicy,
    trusted_cards: Arc<Mutex<HashMap<String, Value>>>,
}

impl<P: JacsProvider> HaiClient<P> {
    pub fn get_a2a(&self, trust_policy: Option<A2ATrustPolicy>) -> A2AIntegration<'_, P> {
        A2AIntegration {
            client: self,
            trust_policy: trust_policy.unwrap_or(A2ATrustPolicy::Verified),
            trusted_cards: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<'a, P: JacsProvider> A2AIntegration<'a, P> {
    pub fn export_agent_card(&self, agent_data: &Value) -> Result<A2AAgentCard> {
        let agent_id = value_string(agent_data, &["jacsId"]).if_empty_then(self.client.jacs_id());
        let name = value_string(agent_data, &["jacsName"]).if_empty_then(self.client.jacs_id());
        let default_description = format!("HAIAI agent {name}");
        let description =
            value_string(agent_data, &["jacsDescription"]).if_empty_then(&default_description);
        let version = value_string(agent_data, &["jacsVersion"]).if_empty_then("1.0.0");
        let profile =
            value_string(agent_data, &["a2aProfile"]).if_empty_then(A2A_PROTOCOL_VERSION_04);

        let mut base_url = format!("https://agent-{agent_id}.example.com");
        if let Some(domain) = value_string_opt(agent_data, &["jacsAgentDomain"]) {
            let cleaned = domain.trim_start_matches("https://");
            base_url = format!("https://{cleaned}/agent/{agent_id}");
        }

        let mut supported = vec![A2AAgentInterface {
            url: base_url,
            protocol_binding: "jsonrpc".to_string(),
            protocol_version: None,
            tenant: None,
        }];
        if profile == A2A_PROTOCOL_VERSION_10 {
            supported[0].protocol_version = Some(A2A_PROTOCOL_VERSION_10.to_string());
        }

        let mut metadata = Map::new();
        metadata.insert("jacsId".to_string(), Value::String(agent_id.to_string()));
        metadata.insert(
            "jacsVersion".to_string(),
            Value::String(version.to_string()),
        );
        metadata.insert("a2aProfile".to_string(), Value::String(profile.to_string()));

        Ok(A2AAgentCard {
            name: name.to_string(),
            description: description.to_string(),
            version: version.to_string(),
            protocol_versions: if profile == A2A_PROTOCOL_VERSION_04 {
                vec![A2A_PROTOCOL_VERSION_04.to_string()]
            } else {
                Vec::new()
            },
            supported_interfaces: supported,
            default_input_modes: vec!["text/plain".to_string(), "application/json".to_string()],
            default_output_modes: vec!["text/plain".to_string(), "application/json".to_string()],
            capabilities: A2AAgentCapabilities {
                extensions: vec![A2AAgentExtension {
                    uri: A2A_JACS_EXTENSION_URI.to_string(),
                    description: Some(
                        "JACS cryptographic document signing and verification".to_string(),
                    ),
                    required: false,
                }],
                ..A2AAgentCapabilities::default()
            },
            skills: convert_services_to_skills(agent_data.get("jacsServices")),
            metadata,
        })
    }

    pub fn sign_artifact(
        &self,
        artifact: Value,
        artifact_type: &str,
        parent_signatures: Option<Vec<Value>>,
    ) -> Result<A2AWrappedArtifact> {
        let now = format_rfc3339_now()?;
        let mut wrapped = A2AWrappedArtifact {
            jacs_id: Uuid::new_v4().to_string(),
            jacs_version: "1.0.0".to_string(),
            jacs_type: format!("a2a-{artifact_type}"),
            jacs_level: "artifact".to_string(),
            jacs_version_date: now.clone(),
            a2a_artifact: artifact,
            jacs_parent_signatures: parent_signatures.unwrap_or_default(),
            jacs_signature: None,
        };

        let canonical = canonical_artifact_json(self.client, &wrapped)?;
        let signature = self.client.sign_message(&canonical)?;
        wrapped.jacs_signature = Some(A2AArtifactSignature {
            agent_id: self.client.jacs_id().to_string(),
            date: now,
            signature,
        });
        Ok(wrapped)
    }

    pub fn verify_artifact(
        &self,
        wrapped: &A2AWrappedArtifact,
    ) -> Result<A2AArtifactVerificationResult> {
        let signer_id = wrapped
            .jacs_signature
            .as_ref()
            .map(|s| s.agent_id.clone())
            .unwrap_or_default();

        // Delegate to JACS provider for proper cryptographic verification.
        // This replaces the broken `signature == expected` comparison that
        // fails for non-deterministic algorithms (e.g., pq2025).
        let wrapped_json = serde_json::to_string(wrapped)?;
        match self.client.verify_a2a_artifact(&wrapped_json) {
            Ok(result_json) => {
                let jacs_result: Value = serde_json::from_str(&result_json)?;
                Ok(A2AArtifactVerificationResult {
                    valid: jacs_result
                        .get("valid")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    signer_id: jacs_result
                        .get("signerId")
                        .and_then(Value::as_str)
                        .unwrap_or(&signer_id)
                        .to_string(),
                    artifact_type: jacs_result
                        .get("artifactType")
                        .and_then(Value::as_str)
                        .unwrap_or(&wrapped.jacs_type)
                        .to_string(),
                    timestamp: jacs_result
                        .get("timestamp")
                        .and_then(Value::as_str)
                        .unwrap_or(&wrapped.jacs_version_date)
                        .to_string(),
                    original_artifact: jacs_result
                        .get("originalArtifact")
                        .cloned()
                        .unwrap_or_else(|| wrapped.a2a_artifact.clone()),
                    error: jacs_result
                        .get("error")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                })
            }
            Err(e) => Ok(A2AArtifactVerificationResult {
                valid: false,
                signer_id,
                artifact_type: wrapped.jacs_type.clone(),
                timestamp: wrapped.jacs_version_date.clone(),
                original_artifact: wrapped.a2a_artifact.clone(),
                error: Some(format!("verification failed: {e}")),
            }),
        }
    }

    pub fn create_chain_of_custody(&self, artifacts: &[A2AWrappedArtifact]) -> A2AChainOfCustody {
        let chain = artifacts
            .iter()
            .map(|artifact| {
                let signature = artifact.jacs_signature.as_ref();
                A2AChainEntry {
                    artifact_id: artifact.jacs_id.clone(),
                    artifact_type: artifact.jacs_type.clone(),
                    timestamp: artifact.jacs_version_date.clone(),
                    agent_id: signature.map(|s| s.agent_id.clone()).unwrap_or_default(),
                    signature_present: signature.map(|s| !s.signature.is_empty()).unwrap_or(false),
                }
            })
            .collect::<Vec<_>>();

        A2AChainOfCustody {
            total_artifacts: chain.len(),
            created: format_rfc3339_now().unwrap_or_default(),
            chain_of_custody: chain,
        }
    }

    pub fn generate_well_known_documents(
        &self,
        agent_card: &A2AAgentCard,
        jws_signature: &str,
        public_key_b64: &str,
        agent_data: &Value,
    ) -> Result<Value> {
        let mut card_value = serde_json::to_value(agent_card)?;
        if !jws_signature.is_empty() {
            if let Value::Object(ref mut card_obj) = card_value {
                card_obj.insert("signatures".to_string(), json!([{ "jws": jws_signature }]));
            }
        }

        let agent_id = value_string(agent_data, &["jacsId"]).if_empty_then(self.client.jacs_id());
        let agent_version = value_string(agent_data, &["jacsVersion"]).if_empty_then("1.0.0");
        let fallback_profile = resolve_card_profile(agent_card);
        let profile = value_string(agent_data, &["a2aProfile"]).if_empty_then(&fallback_profile);

        Ok(json!({
            "/.well-known/agent-card.json": card_value,
            "/.well-known/jwks.json": {
                "keys": [{
                    "kty": "OKP",
                    "crv": "Ed25519",
                    "kid": agent_id,
                    "use": "sig",
                    "alg": "EdDSA",
                    "x": public_key_b64
                }]
            },
            "/.well-known/jacs-agent.json": {
                "jacsVersion": "1.0",
                "agentId": agent_id,
                "agentVersion": agent_version,
                "capabilities": {
                    "signing": true,
                    "verification": true
                },
                "endpoints": {
                    "verify": "/jacs/verify",
                    "sign": "/jacs/sign",
                    "agent": "/jacs/agent"
                }
            },
            "/.well-known/jacs-extension.json": {
                "uri": A2A_JACS_EXTENSION_URI,
                "name": "JACS Document Provenance",
                "version": "1.0",
                "a2aProtocolVersion": profile
            }
        }))
    }

    pub fn assess_remote_agent(
        &self,
        agent_card_json: &str,
        policy: Option<A2ATrustPolicy>,
    ) -> Result<A2ATrustAssessment> {
        let card: Value = serde_json::from_str(agent_card_json)?;
        let resolved = policy.unwrap_or(self.trust_policy);
        let jacs_registered = has_jacs_extension(&card);
        let agent_id = extract_card_agent_id(&card).unwrap_or_default();
        let in_trust_store = self.is_trusted_a2a_agent(&agent_id);

        let trust_level = if in_trust_store {
            "explicitly_trusted"
        } else if jacs_registered {
            "jacs_verified"
        } else {
            "untrusted"
        };

        let mut result = A2ATrustAssessment {
            allowed: false,
            trust_level: trust_level.to_string(),
            jacs_registered,
            in_trust_store,
            reason: String::new(),
        };

        match resolved {
            A2ATrustPolicy::Open => {
                result.allowed = true;
                result.reason = "open policy: all agents accepted".to_string();
            }
            A2ATrustPolicy::Verified => {
                result.allowed = jacs_registered;
                result.reason = if result.allowed {
                    "verified policy: card declares JACS extension".to_string()
                } else {
                    "verified policy: card missing JACS extension".to_string()
                };
            }
            A2ATrustPolicy::Strict => {
                result.allowed = in_trust_store;
                result.reason = if result.allowed {
                    "strict policy: agent is in local trust store".to_string()
                } else {
                    "strict policy: agent is not in local trust store".to_string()
                };
            }
        }
        Ok(result)
    }

    pub fn trust_a2a_agent(&self, agent_card_json: &str) -> Result<String> {
        let card: Value = serde_json::from_str(agent_card_json)?;
        let Some(agent_id) = extract_card_agent_id(&card) else {
            return Err(HaiError::Message(
                "cannot trust card without metadata.jacsId".to_string(),
            ));
        };

        let mut trusted = self
            .trusted_cards
            .lock()
            .map_err(|e| HaiError::Message(format!("failed to lock trust store: {e}")))?;
        trusted.insert(agent_id.clone(), card);
        Ok(agent_id)
    }

    pub fn is_trusted_a2a_agent(&self, agent_id: &str) -> bool {
        let Ok(trusted) = self.trusted_cards.lock() else {
            return false;
        };
        trusted.contains_key(agent_id)
    }

    pub fn list_trusted_a2a_agents(&self) -> Vec<String> {
        let Ok(trusted) = self.trusted_cards.lock() else {
            return Vec::new();
        };
        let mut ids = trusted.keys().cloned().collect::<Vec<_>>();
        ids.sort();
        ids
    }

    pub fn register_options_with_agent_card(
        &self,
        mut options: RegisterAgentOptions,
        agent_card: &A2AAgentCard,
    ) -> Result<RegisterAgentOptions> {
        let merged = merge_agent_json_with_card(&options.agent_json, agent_card)?;
        options.agent_json = merged;
        Ok(options)
    }

    pub async fn register_with_agent_card(
        &self,
        options: RegisterAgentOptions,
        agent_card: &A2AAgentCard,
    ) -> Result<RegistrationResult> {
        let merged = self.register_options_with_agent_card(options, agent_card)?;
        self.client.register(&merged).await
    }

    pub async fn send_signed_artifact_email(
        &self,
        to: &str,
        subject: &str,
        artifact: &A2AWrappedArtifact,
    ) -> Result<SendEmailResult> {
        let pretty = serde_json::to_string_pretty(artifact)?;
        self.client
            .send_email(&SendEmailOptions {
                to: to.to_string(),
                subject: subject.to_string(),
                body: format!("Signed A2A artifact:\n\n{pretty}"),
                cc: Vec::new(),
                bcc: Vec::new(),
                in_reply_to: None,
                attachments: Vec::new(),
                labels: Vec::new(),
            })
            .await
    }

    pub async fn on_mediated_benchmark_job<F, Fut>(
        &self,
        options: A2AMediatedJobOptions,
        handler: F,
    ) -> Result<()>
    where
        F: FnMut(A2AWrappedArtifact) -> Fut + Send,
        Fut: Future<Output = Result<Value>> + Send,
    {
        let handler = Arc::new(tokio::sync::Mutex::new(handler));
        let notify_email = options.notify_email.clone();
        let email_subject = options.email_subject.clone();
        let verify_inbound_artifact = options.verify_inbound_artifact;
        let enforce_trust_policy = options.enforce_trust_policy;
        let max_reconnect_attempts = options.max_reconnect_attempts;
        let transport = options.transport;
        let mut reconnect_attempts = 0usize;

        loop {
            let handled_jobs = Arc::new(AtomicUsize::new(0));
            let handled_jobs_for_handler = Arc::clone(&handled_jobs);
            let handler = Arc::clone(&handler);
            let notify_email = notify_email.clone();
            let email_subject = email_subject.clone();

            let result = self
                .client
                .on_benchmark_job(transport, move |event_data| {
                    let handler = Arc::clone(&handler);
                    let handled_jobs_for_handler = Arc::clone(&handled_jobs_for_handler);
                    let notify_email = notify_email.clone();
                    let email_subject = email_subject.clone();
                    async move {
                        handled_jobs_for_handler.fetch_add(1, Ordering::SeqCst);

                        let job_id = value_string(&event_data, &["jobId", "job_id"]);
                        if job_id.is_empty() {
                            return Err(HaiError::Message(
                                "benchmark job event missing jobId".to_string(),
                            ));
                        }

                        if enforce_trust_policy {
                            let card_json = extract_remote_agent_card_json(&event_data)?.ok_or_else(|| {
                                HaiError::Message(
                                    "remote agent card required when trust enforcement is enabled".to_string(),
                                )
                            })?;
                            let trust = self.assess_remote_agent(&card_json, None)?;
                            if !trust.allowed {
                                return Err(HaiError::Message(format!(
                                    "trust policy rejected remote agent: {}",
                                    trust.reason
                                )));
                            }
                        }

                        if verify_inbound_artifact {
                            let inbound = extract_inbound_a2a_task(&event_data)?.ok_or_else(|| {
                                HaiError::Message(
                                    "inbound a2a task required when signature verification is enabled".to_string(),
                                )
                            })?;
                            let verify = self.verify_artifact(&inbound)?;
                            if !verify.valid {
                                return Err(HaiError::Message(format!(
                                    "inbound a2a task signature invalid: {}",
                                    verify.error.unwrap_or_else(|| "unknown verification failure".to_string())
                                )));
                            }
                        }

                        let task_payload = json!({
                            "type": event_data.get("type").cloned().unwrap_or(Value::String("benchmark_job".to_string())),
                            "jobId": job_id,
                            "scenarioId": event_data.get("scenarioId").cloned().unwrap_or(Value::Null),
                            "config": event_data.get("config").cloned().unwrap_or(Value::Null),
                        });
                        let task_artifact = self.sign_artifact(task_payload, "task", None)?;
                        let result_payload = {
                            let mut locked_handler = handler.lock().await;
                            (*locked_handler)(task_artifact.clone()).await?
                        };

                        let parent = serde_json::to_value(&task_artifact)?;
                        let result_artifact = self.sign_artifact(
                            result_payload.clone(),
                            "task-result",
                            Some(vec![parent]),
                        )?;

                        let message = result_payload
                            .get("message")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                            .unwrap_or_else(|| result_payload.to_string());

                        self.client
                            .submit_response(
                                job_id,
                                &message,
                                Some(json!({
                                    "a2aTask": task_artifact,
                                    "a2aResult": result_artifact.clone()
                                })),
                                0,
                            )
                            .await?;

                        if let Some(email) = notify_email {
                            let subject = email_subject
                                .unwrap_or_else(|| format!("A2A mediated result for job {job_id}"));
                            self.send_signed_artifact_email(&email, &subject, &result_artifact)
                                .await?;
                        }

                        Ok(())
                    }
                })
                .await;

            match result {
                Ok(()) => {
                    // If a stream ended without processing work, retry when configured.
                    if handled_jobs.load(Ordering::SeqCst) == 0
                        && reconnect_attempts < max_reconnect_attempts
                    {
                        reconnect_attempts += 1;
                        continue;
                    }
                    return Ok(());
                }
                Err(err) => {
                    if reconnect_attempts < max_reconnect_attempts {
                        reconnect_attempts += 1;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
    }
}

fn convert_services_to_skills(services: Option<&Value>) -> Vec<A2AAgentSkill> {
    let Some(Value::Array(entries)) = services else {
        return vec![A2AAgentSkill {
            id: "verify-signature".to_string(),
            name: "verify_signature".to_string(),
            description: "Verify JACS document signatures".to_string(),
            tags: vec![
                "jacs".to_string(),
                "verification".to_string(),
                "cryptography".to_string(),
            ],
            ..A2AAgentSkill::default()
        }];
    };

    let mut out = entries
        .iter()
        .filter_map(|entry| entry.as_object())
        .map(|entry| {
            let name = entry
                .get("name")
                .and_then(Value::as_str)
                .or_else(|| entry.get("serviceDescription").and_then(Value::as_str))
                .unwrap_or("service");
            let description = entry
                .get("serviceDescription")
                .and_then(Value::as_str)
                .unwrap_or("No description");

            A2AAgentSkill {
                id: slugify(name),
                name: name.to_string(),
                description: description.to_string(),
                tags: vec!["jacs".to_string(), slugify(name)],
                ..A2AAgentSkill::default()
            }
        })
        .collect::<Vec<_>>();

    if out.is_empty() {
        out.push(A2AAgentSkill {
            id: "verify-signature".to_string(),
            name: "verify_signature".to_string(),
            description: "Verify JACS document signatures".to_string(),
            tags: vec![
                "jacs".to_string(),
                "verification".to_string(),
                "cryptography".to_string(),
            ],
            ..A2AAgentSkill::default()
        });
    }
    out
}

fn canonical_artifact_json<P: JacsProvider>(
    client: &HaiClient<P>,
    wrapped: &A2AWrappedArtifact,
) -> Result<String> {
    let mut clone = wrapped.clone();
    clone.jacs_signature = None;
    let value = serde_json::to_value(clone)?;
    client.canonical_json(&value)
}

fn has_jacs_extension(card: &Value) -> bool {
    card.get("capabilities")
        .and_then(Value::as_object)
        .and_then(|caps| caps.get("extensions"))
        .and_then(Value::as_array)
        .map(|extensions| {
            extensions.iter().any(|ext| {
                ext.get("uri")
                    .and_then(Value::as_str)
                    .map(|uri| uri == A2A_JACS_EXTENSION_URI)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn extract_card_agent_id(card: &Value) -> Option<String> {
    card.get("metadata")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("jacsId"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn extract_remote_agent_card_json(event: &Value) -> Result<Option<String>> {
    for key in ["remoteAgentCard", "agentCard"] {
        let Some(card) = event.get(key) else {
            continue;
        };
        return match card {
            Value::String(raw) => Ok(Some(raw.clone())),
            Value::Object(_) => Ok(Some(serde_json::to_string(card)?)),
            _ => Err(HaiError::Message(format!(
                "{key} must be a JSON object or string"
            ))),
        };
    }
    Ok(None)
}

fn extract_inbound_a2a_task(event: &Value) -> Result<Option<A2AWrappedArtifact>> {
    for key in ["a2aTask", "a2a_task"] {
        let Some(task) = event.get(key) else {
            continue;
        };
        return match task {
            Value::String(raw) => Ok(Some(serde_json::from_str(raw)?)),
            Value::Object(_) => Ok(Some(serde_json::from_value(task.clone())?)),
            _ => Err(HaiError::Message(format!(
                "{key} must be a JSON object or string"
            ))),
        };
    }
    Ok(None)
}

fn merge_agent_json_with_card(agent_json: &str, card: &A2AAgentCard) -> Result<String> {
    if agent_json.trim().is_empty() {
        return Err(HaiError::Message("agent_json is required".to_string()));
    }

    let mut agent_doc: Value = serde_json::from_str(agent_json)?;
    let Some(agent_obj) = agent_doc.as_object_mut() else {
        return Err(HaiError::Message(
            "agent_json must be a JSON object".to_string(),
        ));
    };

    let card_value = serde_json::to_value(card)?;
    agent_obj.insert("a2aAgentCard".to_string(), card_value);

    if !agent_obj.contains_key("skills") && !card.skills.is_empty() {
        agent_obj.insert("skills".to_string(), serde_json::to_value(&card.skills)?);
    }
    if !agent_obj.contains_key("capabilities") {
        agent_obj.insert(
            "capabilities".to_string(),
            serde_json::to_value(&card.capabilities)?,
        );
    }

    let mut metadata = agent_obj
        .remove("metadata")
        .and_then(|m| m.as_object().cloned())
        .unwrap_or_default();
    metadata.insert(
        "a2aProfile".to_string(),
        Value::String(resolve_card_profile(card)),
    );
    metadata.insert(
        "a2aSkillsCount".to_string(),
        Value::Number(serde_json::Number::from(card.skills.len())),
    );
    agent_obj.insert("metadata".to_string(), Value::Object(metadata));

    Ok(serde_json::to_string(&agent_doc)?)
}

fn resolve_card_profile(card: &A2AAgentCard) -> String {
    if let Some(profile) = card
        .metadata
        .get("a2aProfile")
        .and_then(Value::as_str)
        .map(ToString::to_string)
    {
        return profile;
    }
    if let Some(profile) = card
        .protocol_versions
        .iter()
        .find(|v| !v.trim().is_empty())
        .cloned()
    {
        return profile;
    }
    if let Some(profile) = card
        .supported_interfaces
        .iter()
        .find_map(|iface| iface.protocol_version.clone())
        .filter(|v| !v.trim().is_empty())
    {
        return profile;
    }
    A2A_PROTOCOL_VERSION_04.to_string()
}

fn format_rfc3339_now() -> Result<String> {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|e| HaiError::Message(format!("failed to format timestamp: {e}")))
}

fn slugify(input: &str) -> String {
    let lowered = input.trim().to_lowercase();
    let replaced = lowered.replace([' ', '_'], "-");
    let mut out = String::new();
    for ch in replaced.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            out.push(ch);
        }
    }
    if out.is_empty() {
        "skill".to_string()
    } else {
        out
    }
}

fn value_string_opt<'a>(value: &'a Value, paths: &[&str]) -> Option<&'a str> {
    for path in paths {
        if let Some(found) = value.get(path).and_then(Value::as_str) {
            if !found.is_empty() {
                return Some(found);
            }
        }
    }
    None
}

fn value_string<'a>(value: &'a Value, paths: &[&str]) -> &'a str {
    value_string_opt(value, paths).unwrap_or("")
}

trait IfEmpty {
    fn if_empty_then<'a>(&'a self, fallback: &'a str) -> &'a str;
}

impl IfEmpty for str {
    fn if_empty_then<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.trim().is_empty() {
            fallback
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_keeps_ascii_and_dashes() {
        assert_eq!(slugify("Sign Response"), "sign-response");
        assert_eq!(slugify("a_b$c"), "a-bc");
    }

    #[test]
    fn resolve_profile_prefers_metadata() {
        let mut metadata = Map::new();
        metadata.insert("a2aProfile".to_string(), Value::String("1.0".to_string()));
        let card = A2AAgentCard {
            metadata,
            protocol_versions: vec!["0.4.0".to_string()],
            ..A2AAgentCard::default()
        };
        assert_eq!(resolve_card_profile(&card), "1.0");
    }

    #[test]
    fn merge_agent_json_with_card_adds_metadata() {
        let card = A2AAgentCard {
            skills: vec![A2AAgentSkill {
                id: "k".to_string(),
                name: "n".to_string(),
                description: "d".to_string(),
                tags: vec!["jacs".to_string()],
                ..A2AAgentSkill::default()
            }],
            supported_interfaces: vec![A2AAgentInterface {
                url: "https://agent.example.com".to_string(),
                protocol_binding: "jsonrpc".to_string(),
                protocol_version: Some("1.0".to_string()),
                tenant: None,
            }],
            ..A2AAgentCard::default()
        };

        let merged =
            merge_agent_json_with_card(r#"{"jacsId":"demo-agent"}"#, &card).expect("merged");
        let merged_json: Value = serde_json::from_str(&merged).expect("decode");
        assert!(merged_json.get("a2aAgentCard").is_some());
        assert_eq!(merged_json["metadata"]["a2aProfile"], "1.0");
        assert_eq!(merged_json["metadata"]["a2aSkillsCount"], 1);
    }

    #[test]
    fn extract_inbound_task_from_object() {
        let event = json!({
            "a2aTask": {
                "jacsId": "task-1",
                "jacsVersion": "1.0.0",
                "jacsType": "a2a-task",
                "jacsLevel": "artifact",
                "jacsVersionDate": "2026-02-24T00:00:00Z",
                "a2aArtifact": {"k":"v"},
                "jacsSignature": {
                    "agentID": "demo-agent",
                    "date": "2026-02-24T00:00:00Z",
                    "signature": "sig"
                }
            }
        });

        let task = extract_inbound_a2a_task(&event)
            .expect("extract")
            .expect("task");
        assert_eq!(task.jacs_type, "a2a-task");
    }

    #[test]
    fn extract_remote_card_from_object() {
        let event = json!({
            "remoteAgentCard": {
                "name": "remote",
                "metadata": {"jacsId":"remote-agent"}
            }
        });

        let raw = extract_remote_agent_card_json(&event)
            .expect("extract")
            .expect("card");
        let card: Value = serde_json::from_str(&raw).expect("decode");
        assert_eq!(card["metadata"]["jacsId"], "remote-agent");
    }
}
