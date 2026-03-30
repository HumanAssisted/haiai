//! Rust HAIAI.
//!
//! This crate is intentionally a thin HAI-platform wrapper around JACS.
//! Runtime signing/canonicalization should be delegated via [`JacsProvider`].
//!
//! # Feature flags
//!
//! * `jacs-crate` (default) -- Use the published jacs crate from crates.io.
//!
//! # Agent API
//!
//! The recommended entry point is [`agent::Agent`], which provides an
//! ergonomic `agent.email.*` namespace. All email operations sign with
//! the agent's JACS key -- there is no unsigned send path.
//!
//! ```rust,no_run
//! use haiai::agent::Agent;
//! use haiai::types::SendEmailOptions;
//!
//! # async fn example() -> haiai::Result<()> {
//! let agent = Agent::from_config(None).await?;
//! agent.email.send(SendEmailOptions {
//!     to: "other@hai.ai".into(),
//!     subject: "Hello".into(),
//!     body: "World".into(),
//!     cc: vec![],
//!     bcc: vec![],
//!     in_reply_to: None,
//!     attachments: vec![],
//!     labels: vec![],
//!     append_footer: None,
//! }).await?;
//! # Ok(())
//! # }
//! ```

pub mod a2a;
pub mod agent;
pub mod client;
pub mod config;
#[cfg(feature = "jacs-crate")]
pub mod email;
pub mod error;
pub mod jacs;
#[cfg(feature = "jacs-crate")]
pub mod jacs_local;
pub mod key_format;
pub mod mime;
pub mod self_knowledge;
pub mod types;
pub mod validation;
pub mod verify;

pub use a2a::{
    A2AAgentCapabilities, A2AAgentCard, A2AAgentExtension, A2AAgentInterface, A2AAgentSkill,
    A2AArtifactSignature, A2AArtifactVerificationResult, A2AChainEntry, A2AChainOfCustody,
    A2AIntegration, A2AMediatedJobOptions, A2ATrustAssessment, A2ATrustPolicy, A2AWrappedArtifact,
    A2A_JACS_EXTENSION_URI, A2A_PROTOCOL_VERSION_04, A2A_PROTOCOL_VERSION_10,
};
#[cfg(feature = "jacs-crate")]
pub use agent::{Agent, EmailNamespace};
pub use client::{
    HaiClient, HaiClientOptions, SseConnection, WsConnection, DEFAULT_BASE_URL,
    DEFAULT_DNS_RESOLVER, DEFAULT_MAX_RETRIES, DEFAULT_TIMEOUT_SECS,
};
pub use config::{
    load_config, redacted_display, resolve_private_key_candidates, resolve_storage_backend,
    resolve_storage_backend_label, AgentConfig, StorageConfigSummary,
};
#[cfg(feature = "jacs-crate")]
pub use email::{
    compute_content_hash,
    // JACS email types re-exported for consumer convenience
    sign_email,
    verify_email,
    AttachmentEntry,
    AttachmentInput,
    BodyPartEntry,
    ContentVerificationResult,
    EmailSignatureHeaders,
    EmailSignaturePayload,
    JacsEmailMetadata,
    JacsEmailSignature,
    JacsEmailSignatureDocument,
    ParsedAttachment,
    ParsedBodyPart,
    ParsedEmailParts,
    SignedHeaderEntry,
};
pub use error::{HaiError, Result};
pub use jacs::{
    JacsAgentLifecycle, JacsBatchProvider, JacsDocumentProvider, JacsEmailProvider, JacsProvider,
    JacsVerificationProvider, NoopJacsProvider, StaticJacsProvider,
};
#[cfg(feature = "agreements")]
pub use jacs::JacsAgreementProvider;
#[cfg(feature = "attestation")]
pub use jacs::JacsAttestationProvider;
#[cfg(feature = "jacs-crate")]
pub use jacs_local::LocalJacsProvider;
pub use types::*;
pub use verify::{
    generate_verify_link, generate_verify_link_hosted, MAX_VERIFY_DOCUMENT_BYTES,
    MAX_VERIFY_URL_LEN,
};
