//! Rust HAISDK.
//!
//! This crate is intentionally a thin HAI-platform wrapper around JACS.
//! Runtime signing/canonicalization should be delegated via [`JacsProvider`].

pub mod a2a;
pub mod client;
pub mod config;
pub mod error;
pub mod jacs;
#[cfg(feature = "jacs-local")]
pub mod jacs_local;
pub mod types;
pub mod verify;

pub use a2a::{
    A2AAgentCapabilities, A2AAgentCard, A2AAgentExtension, A2AAgentInterface, A2AAgentSkill,
    A2AArtifactSignature, A2AArtifactVerificationResult, A2AChainEntry, A2AChainOfCustody,
    A2AIntegration, A2AMediatedJobOptions, A2ATrustAssessment, A2ATrustPolicy, A2AWrappedArtifact,
    A2A_JACS_EXTENSION_URI, A2A_PROTOCOL_VERSION_04, A2A_PROTOCOL_VERSION_10,
};
pub use client::{HaiClient, HaiClientOptions, SseConnection, WsConnection};
pub use config::{load_config, resolve_private_key_candidates, AgentConfig};
pub use error::{HaiError, Result};
pub use jacs::{JacsProvider, NoopJacsProvider, StaticJacsProvider};
#[cfg(feature = "jacs-local")]
pub use jacs_local::LocalJacsProvider;
pub use types::*;
pub use verify::{
    generate_verify_link, generate_verify_link_hosted, parse_jacs_signature_header,
    verify_email_signature, MAX_VERIFY_DOCUMENT_BYTES, MAX_VERIFY_URL_LEN,
};
