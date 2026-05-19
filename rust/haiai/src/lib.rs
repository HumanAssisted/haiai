// Copyright (c) 2026 Human Assisted Intelligence, Inc.
//
// Use of this software is governed by the Business Source License 1.1
// included in the LICENSE file.
//
// SPDX-License-Identifier: BUSL-1.1

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

// HAIAI_WASM_PRD.md §4.2 mutual-exclusivity gate. The `wasm` feature swaps
// jacs (full native crate) for jacs-core / jacs-wasm and target-conditional
// HTTP / WebSocket transports. Mixing the two features cannot produce a
// coherent build, so we fail fast at compile time with a clear message.
#[cfg(all(feature = "wasm", feature = "jacs-crate"))]
compile_error!(
    "the `wasm` and `jacs-crate` features are mutually exclusive: use `--no-default-features --features wasm`"
);

pub mod a2a;
// `agent::Agent` is built on `LocalJacsProvider` (which is `jacs-crate`-gated
// and conflicts with the `wasm` feature) and on `crate::validation` (gated
// out of wasm by Task 009). Browser callers use `BrowserAgentHandle` from
// the `haiai-wasm` crate instead (HAIAI_WASM_PRD §4.3).
#[cfg(not(target_arch = "wasm32"))]
pub mod agent;
pub mod client;
pub mod config;
#[cfg(feature = "jacs-crate")]
pub mod document_store;
#[cfg(feature = "jacs-crate")]
pub mod email;
pub mod email_inline;
pub mod error;
pub mod jacs;
#[cfg(feature = "jacs-crate")]
pub mod jacs_local;
pub mod jacs_remote;
pub mod key_format;
pub mod mime;
// `self_knowledge` pulls `bm25` (a search runtime) and is only ever used
// from the CLI / MCP tool surface. Browsers neither expose a knowledge-query
// API nor have a search runtime; gated out of the wasm build per
// HAIAI_WASM_PRD §4.2.1 + Task 009 audit.
#[cfg(not(target_arch = "wasm32"))]
pub mod self_knowledge;
pub mod types;
// `validation` pulls `html5ever` for HTML body validation in the email send
// path. The wasm build's send path canonicalizes / signs in pure JSON; we
// gate the html5ever-using module out of the wasm tree per HAIAI_WASM_PRD
// §4.2.1 + Task 009 audit. Tasks downstream that need a wasm `validate_html`
// can add a no-op stub at that time.
#[cfg(not(target_arch = "wasm32"))]
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
    HaiClient, HaiClientOptions, DEFAULT_BASE_URL, DEFAULT_DNS_RESOLVER, DEFAULT_MAX_RETRIES,
    DEFAULT_TIMEOUT_SECS,
};
// SseConnection / WsConnection are native-only (tokio task handles); the
// wasm build exposes streaming via `EventStreamHandle` in `haiai-wasm`
// (Task 029). See HAIAI_WASM_PRD §4.6.
#[cfg(not(target_arch = "wasm32"))]
pub use client::{SseConnection, WsConnection};
#[cfg(feature = "jacs-crate")]
pub use config::resolve_log_filter;
pub use config::{
    load_config, redacted_display, resolve_private_key_candidates, resolve_remote,
    resolve_storage_backend, resolve_storage_backend_label, AgentConfig, StorageConfigSummary,
    DEFAULT_LOG_FILTER,
};
#[cfg(feature = "jacs-crate")]
pub use document_store::{build_document_provider, build_document_provider_for_backend};
#[cfg(feature = "jacs-crate")]
pub use email::{
    // JACS email types re-exported for consumer convenience
    sign_email,
    verify_email,
    AttachmentEntry,
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
pub use email_inline::*;
pub use error::{HaiError, Result};
#[cfg(feature = "agreements")]
pub use jacs::JacsAgreementProvider;
#[cfg(feature = "attestation")]
pub use jacs::JacsAttestationProvider;
#[cfg(feature = "jacs-crate")]
pub use jacs::{
    media_verify_result_to_json, media_verify_status_to_str, text_signature_status_to_str,
    verify_text_result_to_json, JacsMediaProvider, MediaVerificationResult, MediaVerifyStatus,
    SignImageOptions, SignTextOptions, SignTextOutcome, SignedMedia, TextSignatureEntry,
    TextSignatureStatus, VerifyImageOptions, VerifyTextOptions, VerifyTextResult,
};
pub use jacs::{
    DocSummary, JacsAgentLifecycle, JacsBatchProvider, JacsDocumentProvider, JacsEmailProvider,
    JacsProvider, JacsVerificationProvider, NoopJacsProvider, SaveDocumentRequest, SaveIntent,
    StaticJacsProvider,
};
#[cfg(feature = "jacs-crate")]
pub use jacs_local::LocalJacsProvider;
pub use jacs_remote::{RemoteJacsProvider, RemoteJacsProviderOptions};
pub use types::*;
pub use verify::{
    generate_verify_link, generate_verify_link_hosted, MAX_VERIFY_DOCUMENT_BYTES,
    MAX_VERIFY_URL_LEN,
};

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    pub(crate) fn env_lock() -> MutexGuard<'static, ()> {
        match ENV_LOCK.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}
