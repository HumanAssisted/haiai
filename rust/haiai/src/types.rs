use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloResult {
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub client_ip: String,
    #[serde(default)]
    pub hai_public_key_fingerprint: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub hai_signed_ack: String,
    #[serde(default)]
    pub hello_id: String,
    #[serde(default)]
    pub test_scenario: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CreateAgentOptions {
    pub name: String,
    pub password: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub algorithm: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_directory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_directory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_storage: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CreateAgentResult {
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub public_key_path: String,
    #[serde(default)]
    pub config_path: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub algorithm: String,
    #[serde(default)]
    pub private_key_path: String,
    #[serde(default)]
    pub data_directory: String,
    #[serde(default)]
    pub key_directory: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub dns_record: String,
}

/// Options for key rotation.
#[derive(Debug, Clone, Default)]
pub struct RotateKeysOptions {
    /// Whether to re-register with HAI after local rotation. Default: true.
    pub register_with_hai: Option<bool>,
}

/// Result of a key rotation operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationResult {
    /// Agent's stable JACS ID (unchanged).
    pub jacs_id: String,
    /// Version before rotation.
    pub old_version: String,
    /// New version assigned during rotation.
    pub new_version: String,
    /// SHA-256 hash of the new public key (hex).
    pub new_public_key_hash: String,
    /// Whether re-registration with HAI succeeded.
    pub registered_with_hai: bool,
    /// Complete self-signed agent JSON string.
    pub signed_agent_json: String,
}

/// Result of an agent document update (metadata re-sign with existing key).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateAgentResult {
    /// Agent's stable JACS ID (unchanged).
    pub jacs_id: String,
    /// Version before the update.
    pub old_version: String,
    /// New version assigned during the update.
    pub new_version: String,
    /// Complete self-signed agent JSON string.
    pub signed_agent_json: String,
    /// Whether re-registration with HAI succeeded.
    #[serde(default)]
    pub registered_with_hai: bool,
}

/// Result of a legacy agent migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrateAgentResult {
    /// Agent's stable JACS ID (unchanged).
    pub jacs_id: String,
    /// Version before migration.
    pub old_version: String,
    /// New version assigned during migration.
    pub new_version: String,
    /// Fields that were patched in the raw JSON before loading (e.g. `["iat", "jti"]`).
    pub patched_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckUsernameResult {
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RegisterAgentOptions {
    pub agent_json: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_key_pem: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RegistrationResult {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub jacs_id: String,
    #[serde(default)]
    pub dns_verified: bool,
    #[serde(default)]
    pub registrations: Vec<RegistrationEntry>,
    #[serde(default)]
    pub registered_at: String,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationEntry {
    #[serde(default)]
    pub key_id: String,
    #[serde(default)]
    pub algorithm: String,
    #[serde(default)]
    pub signature_json: String,
    #[serde(default)]
    pub signed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyAgentResult {
    #[serde(default)]
    pub jacs_id: String,
    #[serde(default)]
    pub registered: bool,
    #[serde(default)]
    pub registrations: Vec<RegistrationEntry>,
    #[serde(default)]
    pub dns_verified: bool,
    #[serde(default)]
    pub registered_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResponseResult {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub job_id: String,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimUsernameResult {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub agent_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateUsernameResult {
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub previous_username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeleteUsernameResult {
    #[serde(default)]
    pub released_username: String,
    #[serde(default)]
    pub cooldown_until: String,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FieldStatus {
    Pass,
    Modified,
    Fail,
    Unverifiable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldResult {
    #[serde(default)]
    pub field: String,
    pub status: FieldStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChainEntry {
    #[serde(default)]
    pub signer: String,
    #[serde(default)]
    pub jacs_id: String,
    #[serde(default)]
    pub valid: bool,
    #[serde(default)]
    pub forwarded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailVerificationResultV2 {
    pub valid: bool,
    #[serde(default)]
    pub jacs_id: String,
    #[serde(default)]
    pub algorithm: String,
    #[serde(default)]
    pub reputation_tier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_verified: Option<bool>,
    #[serde(default)]
    pub field_results: Vec<FieldResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chain: Vec<ChainEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Agent status from registry: "active", "suspended", or "revoked".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_status: Option<String>,
    /// Benchmark tiers the agent has completed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub benchmarks_completed: Vec<String>,
}

impl EmailVerificationResultV2 {
    pub fn err(jacs_id: &str, reputation_tier: &str, error: &str) -> Self {
        Self {
            valid: false,
            jacs_id: jacs_id.to_string(),
            algorithm: String::new(),
            reputation_tier: reputation_tier.to_string(),
            dns_verified: None,
            field_results: Vec::new(),
            chain: Vec::new(),
            error: Some(error.to_string()),
            agent_status: None,
            benchmarks_completed: Vec::new(),
        }
    }
}

/// An email attachment with raw data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailAttachment {
    pub filename: String,
    pub content_type: String,
    /// Raw attachment bytes. Serialized as base64 `data_base64` for the API.
    #[serde(skip)]
    pub data: Vec<u8>,
    /// Base64-encoded data sent to the API.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_base64: Option<String>,
}

impl EmailAttachment {
    /// Create a new attachment from raw bytes.
    ///
    /// `data_base64` is left as `None` and will be computed automatically
    /// during `send_email`.
    pub fn new(filename: String, content_type: String, data: Vec<u8>) -> Self {
        Self {
            filename,
            content_type,
            data,
            data_base64: None,
        }
    }

    /// Return the raw attachment bytes, falling back to decoding `data_base64`
    /// when `data` is empty.
    pub fn effective_data(&self) -> Vec<u8> {
        if !self.data.is_empty() {
            return self.data.clone();
        }
        if let Some(ref b64) = self.data_base64 {
            if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(b64) {
                return decoded;
            }
        }
        Vec::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendEmailOptions {
    pub to: String,
    pub subject: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_reply_to: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<EmailAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendEmailResult {
    #[serde(default)]
    pub message_id: String,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListMessagesOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailMessage {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub from_address: String,
    #[serde(default)]
    pub to_address: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body_text: String,
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    pub is_read: bool,
    #[serde(default)]
    pub delivery_status: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub read_at: Option<String>,
    #[serde(default)]
    pub jacs_verified: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailStatus {
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub tier: String,
    #[serde(default)]
    pub billing_tier: String,
    #[serde(default)]
    pub messages_sent_24h: i32,
    #[serde(default)]
    pub daily_limit: i32,
    #[serde(default)]
    pub daily_used: i32,
    #[serde(default)]
    pub resets_at: String,
    #[serde(default)]
    pub messages_sent_total: i32,
    #[serde(default)]
    pub external_enabled: bool,
    #[serde(default)]
    pub external_sends_today: i32,
    #[serde(default)]
    pub last_tier_change: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRegistryResponse {
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub jacs_id: String,
    #[serde(default)]
    pub public_key: String,
    #[serde(default)]
    pub algorithm: String,
    #[serde(default)]
    pub reputation_tier: String,
    #[serde(default)]
    pub registered_at: String,
    /// Agent status: "active", "suspended", or "revoked".
    #[serde(default)]
    pub agent_status: Option<String>,
    /// Benchmark tiers the agent has completed (e.g., ["free", "pro"]).
    #[serde(default)]
    pub benchmarks_completed: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicKeyInfo {
    #[serde(default)]
    pub jacs_id: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub public_key: String,
    #[serde(default)]
    pub public_key_raw_b64: String,
    #[serde(default)]
    pub algorithm: String,
    #[serde(default)]
    pub public_key_hash: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub dns_verified: bool,
    #[serde(default)]
    pub created_at: String,
}

/// Response from `GET /jacs/v1/agents/{jacs_id}/keys` — all key versions for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentKeyHistory {
    #[serde(default)]
    pub jacs_id: String,
    #[serde(default)]
    pub keys: Vec<PublicKeyInfo>,
    #[serde(default)]
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DocumentVerificationResult {
    #[serde(default)]
    pub valid: bool,
    #[serde(default)]
    pub verified_at: String,
    #[serde(default)]
    pub document_type: String,
    #[serde(default)]
    pub issuer_verified: bool,
    #[serde(default)]
    pub signature_verified: bool,
    #[serde(default)]
    pub signer_id: String,
    #[serde(default)]
    pub signed_at: String,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedPayload {
    pub signed_document: String,
    pub agent_jacs_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TransportType {
    #[default]
    Sse,
    Ws,
}

impl TransportType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sse => "sse",
            Self::Ws => "ws",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProRunOptions {
    pub transport: TransportType,
    pub poll_interval: Duration,
    pub poll_timeout: Duration,
}

impl Default for ProRunOptions {
    fn default() -> Self {
        Self {
            transport: TransportType::Sse,
            poll_interval: Duration::from_secs(2),
            poll_timeout: Duration::from_secs(300),
        }
    }
}

/// Deprecated: Use `ProRunOptions` instead.
pub type DnsCertifiedRunOptions = ProRunOptions;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TranscriptMessage {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub annotations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FreeChaoticResult {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub transcript: Vec<TranscriptMessage>,
    #[serde(default)]
    pub upsell_message: String,
    #[serde(default)]
    pub raw_response: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProRunResult {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub score: f64,
    #[serde(default)]
    pub transcript: Vec<TranscriptMessage>,
    #[serde(default)]
    pub payment_id: String,
    #[serde(default)]
    pub raw_response: Value,
}

/// Deprecated: Use `ProRunResult` instead.
pub type DnsCertifiedResult = ProRunResult;

// =============================================================================
// Document & Search Types (SDK boundary)
// =============================================================================

/// A signed document returned by document operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedDocument {
    /// The document key (`id:version`).
    pub key: String,
    /// The signed document JSON.
    pub json: String,
}

/// Results from a search operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocSearchResults {
    /// The matched documents, ordered by relevance.
    pub results: Vec<DocSearchHit>,
    /// Total number of matching documents (for pagination).
    pub total_count: usize,
    /// Which search method the backend used.
    pub method: String,
}

/// A single search result with relevance metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocSearchHit {
    /// The document key (`id:version`).
    pub key: String,
    /// The signed document JSON.
    pub json: String,
    /// Relevance score (0.0 - 1.0).
    pub score: f64,
    /// Which field(s) matched, if applicable.
    pub matched_fields: Vec<String>,
}

/// Capabilities of the configured storage backend.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StorageCapabilities {
    /// Whether fulltext search is supported.
    pub fulltext: bool,
    /// Whether vector similarity search is supported.
    pub vector: bool,
    /// Whether field-level queries are supported.
    pub query_by_field: bool,
    /// Whether type-based queries are supported.
    pub query_by_type: bool,
    /// Whether pagination is supported.
    pub pagination: bool,
    /// Whether soft-delete (tombstone) is used instead of hard delete.
    pub tombstone: bool,
}

/// Result of a document verification operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocVerificationResult {
    /// The document key (if available).
    pub key: String,
    /// Whether the document is valid.
    pub valid: bool,
    /// Error message if verification failed.
    pub error: Option<String>,
    /// ID of the agent that signed the document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_id: Option<String>,
    /// ISO 8601 timestamp of when the document was signed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    /// Name of the signer (if available).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signer_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VerificationStatus {
    #[serde(default)]
    pub jacs_valid: bool,
    #[serde(default)]
    pub dns_valid: bool,
    #[serde(default)]
    pub hai_registered: bool,
    #[serde(default)]
    pub badge: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentVerificationResult {
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub verification: VerificationStatus,
    #[serde(default)]
    pub hai_signatures: Vec<String>,
    #[serde(default)]
    pub verified_at: String,
    #[serde(default)]
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VerifyAgentDocumentRequest {
    pub agent_json: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HaiEvent {
    #[serde(default)]
    pub event_type: String,
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub raw: String,
}
