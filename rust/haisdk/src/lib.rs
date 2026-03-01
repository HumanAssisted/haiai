//! Rust HAISDK.
//!
//! This crate is intentionally a thin HAI-platform wrapper around JACS.
//! Runtime signing/canonicalization should be delegated via [`JacsProvider`].
//!
//! # Feature flags
//!
//! * `jacs-crate` (default) -- Use the published jacs crate from crates.io.
//! * `jacs-local` -- Use the local path dependency to the JACS repo (for dev).
//!
//! These are mutually exclusive. To use `jacs-local`, run:
//! ```sh
//! cargo test --no-default-features --features jacs-local,rustls-tls
//! ```

#[cfg(all(feature = "jacs-local", feature = "jacs-crate"))]
compile_error!(
    "Features `jacs-local` and `jacs-crate` are mutually exclusive. \
     Use `--no-default-features --features jacs-local,rustls-tls` to select jacs-local."
);

pub mod a2a;
pub mod client;
pub mod config;
pub mod error;
pub mod jacs;
#[cfg(feature = "jacs-local")]
pub mod email;
#[cfg(any(feature = "jacs-crate", feature = "jacs-local"))]
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
#[cfg(feature = "jacs-local")]
pub use email::{
    verify_email, EmailVerificationResultV2,
    // JACS email types re-exported for consumer convenience
    sign_email, AttachmentEntry, BodyPartEntry, ChainEntry, ContentVerificationResult,
    EmailSignatureHeaders, EmailSignaturePayload, EmailSigner, EmailVerifier, FieldResult,
    FieldStatus, JacsEmailMetadata, JacsEmailSignature, JacsEmailSignatureDocument,
    ParsedAttachment, ParsedBodyPart, ParsedEmailParts, SignedHeaderEntry,
};
pub use error::{HaiError, Result};
pub use jacs::{JacsProvider, NoopJacsProvider, StaticJacsProvider};
#[cfg(any(feature = "jacs-crate", feature = "jacs-local"))]
pub use jacs_local::LocalJacsProvider;
pub use types::*;
pub use verify::{
    generate_verify_link, generate_verify_link_hosted,
    MAX_VERIFY_DOCUMENT_BYTES, MAX_VERIFY_URL_LEN,
};
