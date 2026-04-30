use std::collections::HashMap;
use std::path::Path;

use base64::Engine;
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::{HaiError, Result};
use crate::types::{
    DocSearchResults, DocVerificationResult, MigrateAgentResult, RotationResult, SignedDocument,
    SignedPayload, StorageCapabilities, UpdateAgentResult,
};

// =============================================================================
// JACS 0.10.0 media types — re-exported for haiai library consumers.
// =============================================================================
//
// Image side: SignImageOptions / SignedMedia / MediaVerifyStatus /
// MediaVerificationResult / VerifyImageOptions / SignTextOptions / SignTextOutcome.
//
// Inline-text side: VerifyOptions (re-exported as VerifyTextOptions) /
// VerifyTextResult / SignatureStatus / SignatureEntry.
//
// Note: SignImageOptions / VerifyImageOptions / SignTextOptions / VerifyOptions
// derive only Debug+Clone(+Default) — they are NOT Serde-able. Binding-core
// constructs them field-by-field from JSON via local parse_* helpers.
// VerifyTextResult / SignatureEntry / SignatureStatus also lack Serde derives;
// binding-core converts them to a JSON envelope for FFI transport.
#[cfg(feature = "jacs-crate")]
pub use jacs::inline::{
    SignatureEntry as TextSignatureEntry, SignatureStatus as TextSignatureStatus,
    VerifyOptions as VerifyTextOptions, VerifyTextResult,
};
#[cfg(feature = "jacs-crate")]
pub use jacs::simple::types::{
    MediaVerificationResult, MediaVerifyStatus, SignImageOptions, SignTextOptions, SignTextOutcome,
    SignedMedia, VerifyImageOptions,
};

// =============================================================================
// Issue 011: shared snake_case status-string helpers.
//
// These translate the JACS-side `MediaVerifyStatus` and `TextSignatureStatus`
// enums to the canonical wire string used uniformly by binding-core (FFI
// JSON envelopes), hai-mcp (MCP `status` field), and haiai-cli (human label
// + JSON `status` field). Before, three identical match-arm copies lived in
// each crate; the next variant added in JACS would have required three
// edits in three files. Centralizing here makes the wire contract and CLI
// label drift-impossible by construction.
//
// Strings are stable wire identifiers (not human messages). Do not
// localize. New variants must be added by JACS first; the match here will
// then fail to compile, surfacing the breakage at the closest review point.
// =============================================================================

/// Map a JACS [`MediaVerifyStatus`] to its canonical snake_case wire string.
///
/// This is the single source of truth for the status label across the
/// binding-core JSON envelopes, the MCP `verify_image` envelope, and the
/// haiai-cli human label. Centralized per Issue 011 to prevent drift when
/// JACS adds a variant.
#[cfg(feature = "jacs-crate")]
pub fn media_verify_status_to_str(s: &MediaVerifyStatus) -> &'static str {
    match s {
        MediaVerifyStatus::Valid => "valid",
        MediaVerifyStatus::InvalidSignature => "invalid_signature",
        MediaVerifyStatus::HashMismatch => "hash_mismatch",
        MediaVerifyStatus::MissingSignature => "missing_signature",
        MediaVerifyStatus::KeyNotFound => "key_not_found",
        MediaVerifyStatus::UnsupportedFormat => "unsupported_format",
        MediaVerifyStatus::Malformed(_) => "malformed",
    }
}

/// Map a JACS [`TextSignatureStatus`] to its canonical snake_case wire
/// string. Single source of truth across binding-core JSON envelopes,
/// the MCP `verify_text` envelope, and the haiai-cli human label
/// (Issue 011).
#[cfg(feature = "jacs-crate")]
pub fn text_signature_status_to_str(s: &TextSignatureStatus) -> &'static str {
    match s {
        TextSignatureStatus::Valid => "valid",
        TextSignatureStatus::InvalidSignature => "invalid_signature",
        TextSignatureStatus::HashMismatch => "hash_mismatch",
        TextSignatureStatus::KeyNotFound => "key_not_found",
        TextSignatureStatus::UnsupportedAlgorithm => "unsupported_algorithm",
        TextSignatureStatus::Malformed(_) => "malformed",
    }
}

/// Translate the not-Serde-able [`VerifyTextResult`] into a JSON envelope
/// with a flat snake_case `status` string.
///
/// Single conversion site (Issue 013): binding-core FFI envelopes, the MCP
/// `verify_text` envelope, and the `haiai verify-text --json` CLI output all
/// route through this helper, so the wire shape is identical across surfaces:
///
/// ```text
/// { "status": "signed" | "missing_signature" | "malformed",
///   "signatures": [...],
///   "malformed_detail"?: string }
/// ```
///
/// The `signatures[]` entries each carry their own per-signature `status`
/// (produced via [`text_signature_status_to_str`]) — that is the right place
/// for the valid-vs-invalid signal. The file-level `status` is `"signed"`
/// whenever at least one signature was found, regardless of validity, to
/// match the JACS reference CLI and the documented SDK contract.
#[cfg(feature = "jacs-crate")]
pub fn verify_text_result_to_json(result: &VerifyTextResult) -> Value {
    match result {
        VerifyTextResult::Signed { signatures } => {
            let entries: Vec<Value> = signatures
                .iter()
                .map(|sig| {
                    let mut entry = serde_json::json!({
                        "signer_id": sig.signer_id,
                        "algorithm": sig.algorithm,
                        "timestamp": sig.timestamp,
                        "status": text_signature_status_to_str(&sig.status),
                    });
                    if let TextSignatureStatus::Malformed(detail) = &sig.status {
                        entry["malformed_detail"] = Value::String(detail.clone());
                    }
                    entry
                })
                .collect();
            serde_json::json!({
                "status": "signed",
                "signatures": entries,
            })
        }
        VerifyTextResult::MissingSignature => serde_json::json!({
            "status": "missing_signature",
            "signatures": [],
        }),
        VerifyTextResult::Malformed(detail) => serde_json::json!({
            "status": "malformed",
            "signatures": [],
            "malformed_detail": detail,
        }),
    }
}

/// Translate the partly-Serde-able [`MediaVerificationResult`] into a JSON
/// envelope with a flat snake_case `status` string.
///
/// JACS's [`MediaVerifyStatus`] uses `serde(rename_all = "snake_case")` which
/// serializes the `Malformed(String)` variant as `{"malformed": detail}` — a
/// tagged shape downstream language SDKs cannot consume uniformly (Issue 001).
/// This helper flattens that variant so callers always see
/// `status: "malformed"` plus a sibling `malformed_detail` field.
///
/// Single conversion site (Issue 013): binding-core FFI envelopes, the MCP
/// `verify_image` envelope, and the `haiai verify-image --json` CLI output
/// all route through this helper, so the wire shape is identical across
/// surfaces.
#[cfg(feature = "jacs-crate")]
pub fn media_verify_result_to_json(result: &MediaVerificationResult) -> Value {
    let mut envelope = serde_json::json!({
        "status": media_verify_status_to_str(&result.status),
        "signer_id": result.signer_id,
        "algorithm": result.algorithm,
        "format": result.format,
        "embedding_channels": result.embedding_channels,
    });
    if let MediaVerifyStatus::Malformed(detail) = &result.status {
        envelope["malformed_detail"] = Value::String(detail.clone());
    }
    envelope
}

// =============================================================================
// Layer 0: Core Signing (JacsProvider)
// =============================================================================

/// Bridge trait for JACS operations that HAI SDK depends on.
///
/// Implement this trait by adapting the canonical JACS Rust package (or a
/// local wrapper around it). HAIAI runtime code should not implement crypto
/// primitives directly.
///
/// # 0.2.0 Breaking Change
///
/// `rotate()`, `export_agent_json()`, `update_agent()`, and `sign_email_locally()`
/// are deprecated on this trait and will be removed in a future release.
/// Use [`JacsAgentLifecycle`] for lifecycle operations and [`JacsEmailProvider`]
/// for email operations instead.
pub trait JacsProvider: Send + Sync {
    fn jacs_id(&self) -> &str;
    fn sign_string(&self, message: &str) -> Result<String>;

    /// Sign raw bytes and return the signature bytes.
    /// Required for email signing where the payload is binary.
    fn sign_bytes(&self, data: &[u8]) -> Result<Vec<u8>>;

    /// Return the key identifier used for signing.
    fn key_id(&self) -> &str;

    /// Return the signing algorithm name (e.g., "ed25519", "rsa-pss-sha256").
    fn algorithm(&self) -> &str;

    /// Return canonical JSON text for `value` in the same way JACS signs.
    fn canonical_json(&self, value: &Value) -> Result<String>;

    /// Sign `value` as a full JACS document envelope.
    ///
    /// The returned JSON string MUST be byte-equivalent to what JACS's
    /// `signing_procedure` produces — i.e. it carries the `jacsId`,
    /// `jacsVersion`, `jacsVersionDate`, `jacsOriginalVersion`, `jacsLevel`,
    /// `jacsType`, `jacsSignature` (with `agentID`, `agentVersion`, `date`,
    /// `iat`, `jti`, `signature`, `signingAlgorithm`, `publicKeyHash`,
    /// `fields[]`), and `jacsHash` fields, and the signature MUST verify under
    /// JACS's [`SimpleAgent::verify_with_key`].
    ///
    /// The default implementation returns an error: providers without a real
    /// JACS agent (test stubs like [`StaticJacsProvider`]) cannot produce a
    /// JACS-verifiable envelope. [`LocalJacsProvider`] overrides this to
    /// delegate to JACS's `signing_procedure` so wrappers like
    /// [`crate::jacs_remote::RemoteJacsProvider`] get correct envelopes
    /// without re-implementing the signing scheme.
    ///
    /// Issue 021: previous `RemoteJacsProvider::sign_document` shimmed
    /// `canonical_json` + `sign_string`, which produces signatures that do
    /// NOT verify under JACS's per-field `build_signature_content`. This
    /// trait method is the single source of truth for JACS envelope signing.
    fn sign_envelope(&self, value: &Value) -> Result<String> {
        let _ = value;
        Err(HaiError::Provider(
            "sign_envelope not supported by this provider; use LocalJacsProvider \
             or any provider that wraps a real JACS SimpleAgent"
                .to_string(),
        ))
    }

    /// Sign a file as a JACS file envelope (JACS attachment pipeline).
    ///
    /// The returned [`SignedDocument`]'s `json` MUST carry the JACS file shape
    /// produced by `SimpleAgent::sign_file` — `jacsType="file"`, `jacsLevel`,
    /// `mimetype`, `filename`, and a `jacsFiles[]` block containing either the
    /// embedded payload or a hash-only reference, all driven by the JACS
    /// attachment pipeline. Wrappers such as
    /// [`crate::jacs_remote::RemoteJacsProvider`] delegate to this method so
    /// `(path, embed)` produces an identical envelope regardless of which
    /// provider the caller holds (Issue 006).
    ///
    /// The default implementation returns an error: providers without a real
    /// JACS agent (test stubs like [`StaticJacsProvider`]) cannot produce a
    /// JACS-verifiable file envelope. [`LocalJacsProvider`] overrides this to
    /// delegate to JACS's `SimpleAgent::sign_file`.
    fn sign_file_envelope(&self, path: &str, embed: bool) -> Result<SignedDocument> {
        let _ = (path, embed);
        Err(HaiError::Provider(
            "sign_file_envelope not supported by this provider; use LocalJacsProvider \
             or any provider that wraps a real JACS SimpleAgent"
                .to_string(),
        ))
    }

    /// Return a signed payload accepted by `/api/v1/agents/jobs/{job_id}/response`.
    fn sign_response(&self, payload: &Value) -> Result<SignedPayload>;

    /// Verify a wrapped A2A artifact using JACS cryptographic verification.
    ///
    /// Returns a JSON string containing the verification result with fields:
    /// `valid`, `status`, `signerId`, `artifactType`, `timestamp`, `originalArtifact`.
    ///
    /// Default implementation falls back to deterministic signature comparison
    /// (BROKEN for non-deterministic algorithms like pq2025). Providers that
    /// wrap a real JACS agent MUST override this.
    fn verify_a2a_artifact(&self, wrapped_json: &str) -> Result<String> {
        // Fallback: sign the same content and compare signatures.
        // This is WRONG for non-deterministic algorithms but preserved for
        // test providers (StaticJacsProvider) that use deterministic signing.
        let wrapped: Value = serde_json::from_str(wrapped_json)?;
        let signature = wrapped
            .get("jacsSignature")
            .and_then(|s| s.get("signature"))
            .and_then(|s| s.as_str())
            .unwrap_or("");
        let signer_id = wrapped
            .get("jacsSignature")
            .and_then(|s| s.get("agentID"))
            .and_then(|s| s.as_str())
            .unwrap_or("");

        // Strip signature for canonical form
        let mut clone = wrapped.clone();
        if let Some(obj) = clone.as_object_mut() {
            obj.remove("jacsSignature");
        }
        let canonical = self.canonical_json(&clone)?;
        let expected = self.sign_string(&canonical)?;
        let valid = signature == expected;

        let result = serde_json::json!({
            "valid": valid,
            "status": if valid { "verified" } else { "invalid" },
            "signerId": signer_id,
            "artifactType": wrapped.get("jacsType").and_then(|v| v.as_str()).unwrap_or(""),
            "timestamp": wrapped.get("jacsVersionDate").and_then(|v| v.as_str()).unwrap_or(""),
            "originalArtifact": wrapped.get("a2aArtifact").cloned().unwrap_or(Value::Null),
        });
        Ok(serde_json::to_string(&result)?)
    }

    /// Sign a raw RFC 5322 email locally using the agent's own JACS key.
    ///
    /// **Deprecated:** Use [`JacsEmailProvider::sign_email()`] instead.
    /// This method will be removed in a future release.
    fn sign_email_locally(&self, raw_email: &[u8]) -> Result<Vec<u8>> {
        let _ = raw_email;
        Err(HaiError::Provider(
            "local email signing not supported by this provider; use LocalJacsProvider".to_string(),
        ))
    }

    /// Rotate the agent's keys locally.
    ///
    /// **Deprecated:** Use [`JacsAgentLifecycle::rotate()`] instead.
    /// This method will be removed in a future release.
    fn rotate(&self) -> Result<RotationResult> {
        Err(HaiError::Provider(
            "key rotation not supported by this provider; use LocalJacsProvider".to_string(),
        ))
    }

    /// Export the current agent document as a JSON string.
    ///
    /// **Deprecated:** Use [`JacsAgentLifecycle::export_agent_json()`] instead.
    /// This method will be removed in a future release.
    fn export_agent_json(&self) -> Result<String> {
        Err(HaiError::Provider(
            "export_agent_json not supported by this provider; use LocalJacsProvider".to_string(),
        ))
    }

    /// Update agent metadata and re-sign with the existing key.
    ///
    /// **Deprecated:** Use [`JacsAgentLifecycle::update_agent()`] instead.
    /// This method will be removed in a future release.
    fn update_agent(&self, new_agent_data: &str) -> Result<UpdateAgentResult> {
        let _ = new_agent_data;
        Err(HaiError::Provider(
            "update_agent not supported by this provider; use LocalJacsProvider".to_string(),
        ))
    }
}

// =============================================================================
// Layer 1: Agent Lifecycle (JacsAgentLifecycle)
// =============================================================================

/// Extension trait for agent lifecycle operations.
///
/// Provides key rotation, agent migration, metadata update, diagnostics,
/// self-verification, quickstart creation, key re-encryption, and DNS
/// setup instructions.
pub trait JacsAgentLifecycle: JacsProvider {
    /// Rotate the agent's keys. Archives old keys, generates a new keypair,
    /// builds a new self-signed agent document, updates config on disk.
    fn lifecycle_rotate(&self) -> Result<RotationResult>;

    /// Migrate a legacy agent whose document predates a schema change.
    fn lifecycle_migrate(config_path: Option<&Path>) -> Result<MigrateAgentResult>
    where
        Self: Sized;

    /// Update agent metadata and re-sign with the existing key.
    fn lifecycle_update_agent(&self, new_data: &str) -> Result<UpdateAgentResult>;

    /// Export the current agent document as a JSON string.
    fn lifecycle_export_agent_json(&self) -> Result<String>;

    /// Return diagnostic information about the agent as a JSON value.
    fn diagnostics(&self) -> Result<Value>;

    /// Verify the agent's own signature integrity.
    fn verify_self(&self) -> Result<DocVerificationResult>;

    /// Create a new agent with zero-config onboarding.
    fn quickstart(
        name: &str,
        domain: &str,
        description: Option<&str>,
        algorithm: Option<&str>,
        config_path: Option<&str>,
    ) -> Result<Value>
    where
        Self: Sized;

    /// Re-encrypt the agent's private key with a new password.
    fn reencrypt_key(&self, old_password: &str, new_password: &str) -> Result<()>;

    /// Get DNS setup instructions for 5 cloud providers.
    fn get_setup_instructions(&self, domain: &str, ttl: u32) -> Result<Value>;
}

// =============================================================================
// Layer 2: Document Operations (JacsDocumentProvider)
// =============================================================================

/// Extension trait for document storage, retrieval, versioning, and search.
///
/// Wraps the JACS `DocumentService` into SDK-friendly signatures
/// (strings at the boundary, not internal JACS types).
pub trait JacsDocumentProvider: JacsProvider {
    /// Sign a document (returns signed JSON, does NOT store).
    fn sign_document(&self, data: &Value) -> Result<String>;

    /// Store a pre-signed document. Returns the document key (`id:version`).
    fn store_document(&self, signed_json: &str) -> Result<String>;

    /// Convenience: sign + store in one call.
    fn sign_and_store(&self, data: &Value) -> Result<SignedDocument>;

    /// Sign a file (with optional embedding). Returns the signed document.
    fn sign_file(&self, path: &str, embed: bool) -> Result<SignedDocument>;

    /// Get a document by key (`id:version`). Returns signed JSON.
    fn get_document(&self, key: &str) -> Result<String>;

    /// List document keys, optionally filtered by type.
    fn list_documents(&self, jacs_type: Option<&str>) -> Result<Vec<String>>;

    /// Get all versions of a document.
    fn get_document_versions(&self, doc_id: &str) -> Result<Vec<String>>;

    /// Get the latest version of a document.
    fn get_latest_document(&self, doc_id: &str) -> Result<String>;

    /// Remove (tombstone) a document.
    fn remove_document(&self, key: &str) -> Result<()>;

    /// Update a document, creating a new signed version.
    fn update_document(&self, doc_id: &str, data: &str) -> Result<SignedDocument>;

    /// Search documents (fulltext/hybrid depending on backend).
    fn search_documents(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<DocSearchResults>;

    /// Query documents by `jacsType`.
    fn query_by_type(&self, doc_type: &str, limit: usize, offset: usize) -> Result<Vec<String>>;

    /// Query documents by field value.
    ///
    /// **Backend-dependent.** `LocalJacsProvider` filters in-memory by the
    /// requested field; `RemoteJacsProvider` (Issue 052) does NOT support
    /// this method and returns
    /// [`HaiError::BackendUnsupported`](crate::error::HaiError::BackendUnsupported)
    /// — envelope JSON lives in S3 on the hai-api backend, not Postgres
    /// (PRD §10 Non-Goal #19), so a server-side JSONB filter would require
    /// fetching every candidate from S3. Cross-language consumers should
    /// match on the typed error and fall back to `search_documents` for
    /// full-text search, or call `query_by_type` / `query_by_agent` which
    /// are supported across backends.
    fn query_by_field(
        &self,
        field: &str,
        value: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<String>>;

    /// Query documents signed by a specific agent.
    fn query_by_agent(&self, agent_id: &str, limit: usize, offset: usize) -> Result<Vec<String>>;

    /// Report the capabilities of the configured storage backend.
    fn storage_capabilities(&self) -> Result<StorageCapabilities>;

    // =========================================================================
    // D5: MEMORY / SOUL convenience wrappers (Issue 003)
    //
    // These wrap the generic CRUD with a specific `jacsType` so the LLM caller
    // and the CLI/MCP surfaces see them by name. The default implementation is
    // the single D5 path for local and remote providers: sign typed JSON,
    // store through the configured backend, and query latest by type.
    // =========================================================================

    /// Sign and store a MEMORY record. If `content` is `None` the implementation
    /// reads `MEMORY.md` from CWD. Returns the record key (`id:version`).
    fn save_memory(&self, content: Option<&str>) -> Result<String> {
        save_typed_document(self, "memory", content, "MEMORY.md")
    }

    /// Sign and store a SOUL record. Mirror of `save_memory`.
    fn save_soul(&self, content: Option<&str>) -> Result<String> {
        save_typed_document(self, "soul", content, "SOUL.md")
    }

    /// Fetch the latest MEMORY record's signed envelope JSON. `Ok(None)` when
    /// no memory record exists for the caller.
    fn get_memory(&self) -> Result<Option<String>> {
        get_typed_latest_document(self, "memory")
    }

    /// Fetch the latest SOUL record's signed envelope JSON. Mirror of `get_memory`.
    fn get_soul(&self) -> Result<Option<String>> {
        get_typed_latest_document(self, "soul")
    }

    // =========================================================================
    // D9: typed-content helpers (Issue 003)
    //
    // Read a local file, set the right `Content-Type`, and POST it. Each method
    // delegates to the generic record CRUD with format-specific handling.
    // =========================================================================

    /// Read a signed-text file (markdown w/ JACS signature block) and POST it
    /// with `Content-Type: text/markdown; profile=jacs-text-v1`. Returns the key.
    fn store_text_file(&self, _path: &str) -> Result<String> {
        Err(HaiError::Provider(
            "store_text_file: not implemented for this provider".to_string(),
        ))
    }

    /// Detect a signed image's format from leading magic bytes and POST it with
    /// `Content-Type: image/png|jpeg|webp`. Returns the key.
    fn store_image_file(&self, _path: &str) -> Result<String> {
        Err(HaiError::Provider(
            "store_image_file: not implemented for this provider".to_string(),
        ))
    }

    /// Fetch the raw record bytes — no UTF-8 decode, no JSON parse. Used by D9
    /// callers reading binary content (signed images, signed-text envelopes).
    fn get_record_bytes(&self, _key: &str) -> Result<Vec<u8>> {
        Err(HaiError::Provider(
            "get_record_bytes: not implemented for this provider".to_string(),
        ))
    }
}

fn save_typed_document<P: JacsDocumentProvider + ?Sized>(
    provider: &P,
    jacs_type: &str,
    content: Option<&str>,
    default_filename: &str,
) -> Result<String> {
    let body = match content {
        Some(s) => s.to_string(),
        None => std::fs::read_to_string(default_filename)
            .map_err(|e| HaiError::Provider(format!("read {}: {}", default_filename, e)))?,
    };
    tracing::info!(
        operation = "save_typed_document",
        jacs_type,
        "saving typed JACS document"
    );
    let payload = json!({
        "jacsType": jacs_type,
        "body": body,
    });
    provider.sign_and_store(&payload).map(|doc| doc.key)
}

fn get_typed_latest_document<P: JacsDocumentProvider + ?Sized>(
    provider: &P,
    jacs_type: &str,
) -> Result<Option<String>> {
    tracing::info!(
        operation = "get_typed_latest_document",
        jacs_type,
        "fetching latest typed JACS document"
    );
    let keys = provider.query_by_type(jacs_type, 1, 0)?;
    if let Some(key) = keys.first() {
        return provider.get_document(key).map(Some);
    }

    for key in provider.list_documents(None)? {
        let doc = provider.get_document(&key)?;
        let value: Value = serde_json::from_str(&doc)?;
        if value.get("jacsType").and_then(Value::as_str) == Some(jacs_type) {
            return Ok(Some(doc));
        }
    }

    Ok(None)
}

// =============================================================================
// Layer 3: Batch Operations (JacsBatchProvider)
// =============================================================================

/// Extension trait for batch sign/verify operations.
pub trait JacsBatchProvider: JacsProvider {
    /// Sign multiple messages in a single batch operation.
    fn sign_messages(&self, messages: &[&Value]) -> Result<Vec<SignedDocument>>;

    /// Verify multiple documents in a single batch operation.
    fn verify_batch(&self, documents: &[&str]) -> Vec<DocVerificationResult>;
}

// =============================================================================
// Layer 4: Verification (JacsVerificationProvider)
// =============================================================================

/// Extension trait for document verification, DNS trust, and auth headers.
pub trait JacsVerificationProvider: JacsProvider {
    /// Verify a signed document.
    fn verify_document(&self, document: &str) -> Result<DocVerificationResult>;

    /// Verify a document with an explicit public key.
    fn verify_with_key(&self, document: &str, key: Vec<u8>) -> Result<DocVerificationResult>;

    /// Verify a document by storage lookup (requires document service).
    fn verify_by_id(&self, doc_id: &str) -> Result<DocVerificationResult>;

    /// Verify an agent's public key via DNS (L2 trust).
    fn verify_dns(&self, domain: &str) -> Result<()>;

    /// Build a JACS Authorization header for HTTP requests.
    fn build_auth_header_jacs(&self) -> Result<String>;

    /// Unwrap a signed event, verifying its signature against known server public keys.
    ///
    /// Returns a tuple of (unwrapped data, was_verified). If the signer's key is found
    /// in `server_public_keys`, the signature is verified and `was_verified` is `true`.
    /// If the signer's key is not found, the data is returned unverified.
    fn unwrap_signed_event(
        &self,
        event: &Value,
        server_public_keys: &HashMap<String, Vec<u8>>,
    ) -> Result<(Value, bool)>;
}

// =============================================================================
// Layer 5: Email (JacsEmailProvider)
// =============================================================================

/// Extension trait for email signing, verification, and attachment management.
pub trait JacsEmailProvider: JacsProvider {
    /// Sign a raw RFC 5322 email.
    fn sign_email(&self, raw: &[u8]) -> Result<Vec<u8>>;

    /// Verify a signed email with the given public key.
    fn verify_email(&self, raw: &[u8], key: Vec<u8>) -> Result<Value>;

    /// Add a JACS attachment to an email.
    fn add_jacs_attachment(&self, email: &[u8], doc: &[u8]) -> Result<Vec<u8>>;

    /// Extract a JACS attachment from an email.
    fn get_jacs_attachment(&self, email: &[u8]) -> Result<Vec<u8>>;

    /// Remove a JACS attachment from an email.
    fn remove_jacs_attachment(&self, email: &[u8]) -> Result<Vec<u8>>;

    /// Parse an email into structured parts.
    fn extract_email_parts(&self, raw: &[u8]) -> Result<Value>;
}

// =============================================================================
// Layer 8: Local Media (JacsMediaProvider) — JACS 0.10.0
// =============================================================================
//
// Local-only inline-text + image sign/verify/extract. Operations do not touch
// the HAI server: the trait is reachable from any provider that holds (or can
// reload) a `jacs::simple::SimpleAgent`. PRD: docs/MEDIA_SIGNING_PRD.md §4.2.

/// Extension trait for local image (PNG/JPEG/WebP) and inline-text sign/verify.
///
/// All operations are local — no HTTP, no server roundtrip. Identity follows
/// the loaded JACS agent (same as `JacsEmailProvider::sign_email`).
///
/// `extract_media_signature` does not consult the agent; the implementation
/// dispatches on `raw_payload` to either `extract_media_signature_raw` (raw
/// base64url payload) or `extract_media_signature` (decoded JSON string) in
/// JACS's `simple::advanced` module.
#[cfg(feature = "jacs-crate")]
pub trait JacsMediaProvider: JacsProvider {
    /// Sign a markdown / text file in place by appending a YAML signature
    /// block (`-----BEGIN JACS SIGNATURE-----`). Optional `.bak` backup.
    fn sign_text_file(&self, path: &str, opts: SignTextOptions) -> Result<SignTextOutcome>;

    /// Verify all signature blocks in a text file. File-level discriminators
    /// (`MissingSignature`, `Malformed`) only escalate to `Err` under
    /// `opts.strict`; per-block status entries always land inside the result.
    fn verify_text_file(&self, path: &str, opts: VerifyTextOptions) -> Result<VerifyTextResult>;

    /// Sign an image file by embedding a JACS signed-document JSON payload
    /// in the format-appropriate metadata chunk (PNG iTXt / JPEG APP11 /
    /// WebP XMP) and writing to `out_path`. With `opts.robust = true`, also
    /// embeds via LSB steganography (PNG/JPEG only — WebP returns Unsupported).
    fn sign_image(
        &self,
        in_path: &str,
        out_path: &str,
        opts: SignImageOptions,
    ) -> Result<SignedMedia>;

    /// Verify the JACS signature embedded in an image. Returns a status
    /// discriminator (`Valid`, `HashMismatch`, `MissingSignature`, etc.) and
    /// the signer info when available.
    fn verify_image(&self, path: &str, opts: VerifyImageOptions)
        -> Result<MediaVerificationResult>;

    /// Extract the JACS signature payload from a signed image without
    /// verifying it. `raw_payload = false` returns the decoded JSON string;
    /// `raw_payload = true` returns the base64url-no-pad wire payload as it
    /// was embedded in the metadata chunk.
    fn extract_media_signature(&self, path: &str, raw_payload: bool) -> Result<Option<String>>;
}

// =============================================================================
// Layer 6: Agreements (feature-gated)
// =============================================================================

/// Extension trait for multi-party agreements.
#[cfg(feature = "agreements")]
pub trait JacsAgreementProvider: JacsProvider {
    /// Create an agreement with specified agents and optional quorum.
    fn create_agreement(
        &self,
        doc: &str,
        agent_ids: &[String],
        quorum: Option<&str>,
    ) -> Result<SignedDocument>;

    /// Sign an existing agreement.
    fn sign_agreement(&self, document: &str) -> Result<SignedDocument>;

    /// Check the status of an agreement.
    fn check_agreement(&self, document: &str) -> Result<Value>;
}

// =============================================================================
// Layer 7: Attestation (feature-gated)
// =============================================================================

/// Extension trait for verifiable attestation claims.
#[cfg(feature = "attestation")]
pub trait JacsAttestationProvider: JacsProvider {
    /// Create an attestation with subject and claims.
    fn create_attestation(&self, subject: &Value, claims: &[Value]) -> Result<String>;

    /// Verify an attestation document.
    fn verify_attestation(&self, doc_key: &str) -> Result<Value>;
}

// =============================================================================
// Box<dyn JacsProvider> delegation
// =============================================================================

/// Blanket implementation so `HaiClient<Box<dyn JacsProvider>>` works.
///
/// This allows hai-binding-core to erase the concrete provider type behind
/// a trait object while still satisfying the `JacsProvider` bound.
impl JacsProvider for Box<dyn JacsProvider> {
    fn jacs_id(&self) -> &str {
        (**self).jacs_id()
    }

    fn sign_string(&self, message: &str) -> Result<String> {
        (**self).sign_string(message)
    }

    fn sign_bytes(&self, data: &[u8]) -> Result<Vec<u8>> {
        (**self).sign_bytes(data)
    }

    fn key_id(&self) -> &str {
        (**self).key_id()
    }

    fn algorithm(&self) -> &str {
        (**self).algorithm()
    }

    fn canonical_json(&self, value: &Value) -> Result<String> {
        (**self).canonical_json(value)
    }

    fn sign_response(&self, payload: &Value) -> Result<SignedPayload> {
        (**self).sign_response(payload)
    }

    fn verify_a2a_artifact(&self, wrapped_json: &str) -> Result<String> {
        (**self).verify_a2a_artifact(wrapped_json)
    }

    fn sign_email_locally(&self, raw_email: &[u8]) -> Result<Vec<u8>> {
        (**self).sign_email_locally(raw_email)
    }

    fn rotate(&self) -> Result<RotationResult> {
        (**self).rotate()
    }

    fn export_agent_json(&self) -> Result<String> {
        (**self).export_agent_json()
    }

    fn update_agent(&self, new_agent_data: &str) -> Result<UpdateAgentResult> {
        (**self).update_agent(new_agent_data)
    }
}

// Blanket impl so `HaiClient<Box<dyn JacsMediaProvider>>` works. The wrapper
// holds a single trait object that satisfies both the base `JacsProvider`
// supertrait and the media-layer methods. PRD §4.3.
//
// Two impls together: `JacsProvider for Box<dyn JacsMediaProvider>` (so the
// supertrait bound is satisfied for trait objects of the more-specific trait)
// plus `JacsMediaProvider for Box<dyn JacsMediaProvider>` (so the trait-method
// dispatch goes through). The base `Box<dyn JacsProvider>` blanket above is
// unaffected — it only matches trait objects of `JacsProvider` itself.
#[cfg(feature = "jacs-crate")]
impl JacsProvider for Box<dyn JacsMediaProvider> {
    fn jacs_id(&self) -> &str {
        (**self).jacs_id()
    }

    fn sign_string(&self, message: &str) -> Result<String> {
        (**self).sign_string(message)
    }

    fn sign_bytes(&self, data: &[u8]) -> Result<Vec<u8>> {
        (**self).sign_bytes(data)
    }

    fn key_id(&self) -> &str {
        (**self).key_id()
    }

    fn algorithm(&self) -> &str {
        (**self).algorithm()
    }

    fn canonical_json(&self, value: &Value) -> Result<String> {
        (**self).canonical_json(value)
    }

    fn sign_response(&self, payload: &Value) -> Result<SignedPayload> {
        (**self).sign_response(payload)
    }

    fn verify_a2a_artifact(&self, wrapped_json: &str) -> Result<String> {
        (**self).verify_a2a_artifact(wrapped_json)
    }

    fn sign_email_locally(&self, raw_email: &[u8]) -> Result<Vec<u8>> {
        (**self).sign_email_locally(raw_email)
    }

    fn rotate(&self) -> Result<RotationResult> {
        (**self).rotate()
    }

    fn export_agent_json(&self) -> Result<String> {
        (**self).export_agent_json()
    }

    fn update_agent(&self, new_agent_data: &str) -> Result<UpdateAgentResult> {
        (**self).update_agent(new_agent_data)
    }
}

#[cfg(feature = "jacs-crate")]
impl JacsMediaProvider for Box<dyn JacsMediaProvider> {
    fn sign_text_file(&self, path: &str, opts: SignTextOptions) -> Result<SignTextOutcome> {
        (**self).sign_text_file(path, opts)
    }

    fn verify_text_file(&self, path: &str, opts: VerifyTextOptions) -> Result<VerifyTextResult> {
        (**self).verify_text_file(path, opts)
    }

    fn sign_image(
        &self,
        in_path: &str,
        out_path: &str,
        opts: SignImageOptions,
    ) -> Result<SignedMedia> {
        (**self).sign_image(in_path, out_path, opts)
    }

    fn verify_image(
        &self,
        path: &str,
        opts: VerifyImageOptions,
    ) -> Result<MediaVerificationResult> {
        (**self).verify_image(path, opts)
    }

    fn extract_media_signature(&self, path: &str, raw_payload: bool) -> Result<Option<String>> {
        (**self).extract_media_signature(path, raw_payload)
    }
}

// =============================================================================
// Providers
// =============================================================================

/// Provider that permits unauthenticated methods only.
#[derive(Debug, Clone)]
pub struct NoopJacsProvider {
    jacs_id: String,
}

impl NoopJacsProvider {
    pub fn new(jacs_id: impl Into<String>) -> Self {
        Self {
            jacs_id: jacs_id.into(),
        }
    }
}

impl JacsProvider for NoopJacsProvider {
    fn jacs_id(&self) -> &str {
        &self.jacs_id
    }

    fn sign_string(&self, _message: &str) -> Result<String> {
        Err(HaiError::Provider(
            "no JACS signer configured; provide a real JacsProvider".to_string(),
        ))
    }

    fn sign_bytes(&self, _data: &[u8]) -> Result<Vec<u8>> {
        Err(HaiError::Provider(
            "no JACS signer configured; provide a real JacsProvider".to_string(),
        ))
    }

    fn key_id(&self) -> &str {
        &self.jacs_id
    }

    fn algorithm(&self) -> &str {
        "none"
    }

    fn canonical_json(&self, value: &Value) -> Result<String> {
        Ok(canonicalize_json_rfc8785(value))
    }

    fn sign_response(&self, _payload: &Value) -> Result<SignedPayload> {
        Err(HaiError::Provider(
            "no JACS response signer configured; provide a real JacsProvider".to_string(),
        ))
    }
}

/// Simple deterministic test provider.
///
/// This exists for contract tests only. Replace in production with a JACS
/// adapter implementation.
#[derive(Debug, Clone)]
pub struct StaticJacsProvider {
    jacs_id: String,
    algorithm: String,
}

impl StaticJacsProvider {
    pub fn new(jacs_id: impl Into<String>) -> Self {
        Self {
            jacs_id: jacs_id.into(),
            algorithm: "ed25519".to_string(),
        }
    }

    /// Create a StaticJacsProvider with a specific algorithm for testing
    /// multi-algorithm behavior.
    pub fn with_algorithm(jacs_id: impl Into<String>, algorithm: impl Into<String>) -> Self {
        Self {
            jacs_id: jacs_id.into(),
            algorithm: algorithm.into(),
        }
    }
}

impl JacsProvider for StaticJacsProvider {
    fn jacs_id(&self) -> &str {
        &self.jacs_id
    }

    fn sign_email_locally(&self, raw_email: &[u8]) -> Result<Vec<u8>> {
        // Test-only: return the raw email as-is (no actual JACS attachment).
        // This is sufficient for integration tests that only verify the HTTP
        // flow, not the cryptographic content of the signature.
        Ok(raw_email.to_vec())
    }

    fn sign_string(&self, message: &str) -> Result<String> {
        let raw = format!("sig:{}", message);
        Ok(base64::engine::general_purpose::STANDARD.encode(raw))
    }

    fn sign_bytes(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut result = b"sig:".to_vec();
        result.extend_from_slice(data);
        Ok(result)
    }

    fn key_id(&self) -> &str {
        &self.jacs_id
    }

    fn algorithm(&self) -> &str {
        &self.algorithm
    }

    fn canonical_json(&self, value: &Value) -> Result<String> {
        Ok(canonicalize_json_rfc8785(value))
    }

    /// Test-only synthetic envelope. Mirrors the JACS envelope shape closely
    /// enough that downstream HTTP tests can assert on the metadata fields
    /// (`jacsId`, `jacsVersion`, `jacsType`, `jacsVersionDate`,
    /// `jacsSignature.agentID`, etc.) without standing up a real JACS agent.
    /// The signature is NOT cryptographically valid under any real verifier —
    /// production code MUST use [`LocalJacsProvider`] (or another provider that
    /// wraps a real `SimpleAgent`).
    ///
    /// Issue 021: real verifiability is enforced by an integration round-trip
    /// (`tests/jacs_remote_signing_round_trip.rs`) using `LocalJacsProvider`,
    /// not by this stub.
    fn sign_envelope(&self, value: &Value) -> Result<String> {
        let now = OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|e| HaiError::Provider(format!("failed to format timestamp: {e}")))?;
        let mut envelope = value.clone();
        if let Value::Object(map) = &mut envelope {
            map.entry("jacsId".to_string())
                .or_insert_with(|| Value::String(Uuid::new_v4().to_string()));
            map.entry("jacsVersion".to_string())
                .or_insert_with(|| Value::String(Uuid::new_v4().to_string()));
            map.entry("jacsVersionDate".to_string())
                .or_insert_with(|| Value::String(now.clone()));
            map.entry("jacsType".to_string())
                .or_insert_with(|| Value::String("document".to_string()));
            let canonical = canonicalize_json_rfc8785(&Value::Object(map.clone()));
            let signature = self.sign_string(&canonical)?;
            map.insert(
                "jacsSignature".to_string(),
                serde_json::json!({
                    "agentID": self.jacs_id,
                    "agentVersion": "test-stub",
                    "date": now,
                    "signature": signature,
                    "signingAlgorithm": self.algorithm,
                    "publicKeyHash": "test-stub-hash",
                    "fields": [],
                }),
            );
        }
        serde_json::to_string(&envelope).map_err(|e| {
            HaiError::Provider(format!("StaticJacsProvider::sign_envelope serialise: {e}"))
        })
    }

    fn sign_response(&self, payload: &Value) -> Result<SignedPayload> {
        let canonical_payload = canonicalize_json_rfc8785(payload);
        let data = serde_json::from_str::<Value>(&canonical_payload)?;
        let now = OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|e| HaiError::Provider(format!("failed to format timestamp: {e}")))?;

        let doc = serde_json::json!({
            "version": "1.0.0",
            "document_type": "job_response",
            "data": data,
            "metadata": {
                "issuer": self.jacs_id,
                "document_id": Uuid::new_v4().to_string(),
                "created_at": now,
                "hash": "",
            },
            "jacsSignature": {
                "agentID": self.jacs_id,
                "date": now,
                "signature": self.sign_string(&canonical_payload)?,
            },
        });

        Ok(SignedPayload {
            signed_document: serde_json::to_string(&doc)?,
            agent_jacs_id: self.jacs_id.clone(),
        })
    }
}

// =============================================================================
// JacsMediaProvider fallback impls for the test-only providers.
//
// PRD §4.3 / TASK_003: hai-binding-core widens its trait object to
// `Box<dyn JacsMediaProvider>`. Static and Noop providers cannot perform real
// media signing (they have no SimpleAgent / no real key material), so each
// method returns a clear `HaiError::Provider` describing the limitation.
// Real signing requires `LocalJacsProvider` or `EmbeddedJacsProvider`.
// =============================================================================

#[cfg(feature = "jacs-crate")]
fn media_op_test_only_error(provider: &str, op: &str) -> HaiError {
    HaiError::Provider(format!(
        "media operation '{op}' requires a real LocalJacsProvider — current provider is {provider} (test-only)"
    ))
}

#[cfg(feature = "jacs-crate")]
impl JacsMediaProvider for NoopJacsProvider {
    fn sign_text_file(&self, _path: &str, _opts: SignTextOptions) -> Result<SignTextOutcome> {
        Err(media_op_test_only_error(
            "NoopJacsProvider",
            "sign_text_file",
        ))
    }

    fn verify_text_file(&self, _path: &str, _opts: VerifyTextOptions) -> Result<VerifyTextResult> {
        Err(media_op_test_only_error(
            "NoopJacsProvider",
            "verify_text_file",
        ))
    }

    fn sign_image(
        &self,
        _in_path: &str,
        _out_path: &str,
        _opts: SignImageOptions,
    ) -> Result<SignedMedia> {
        Err(media_op_test_only_error("NoopJacsProvider", "sign_image"))
    }

    fn verify_image(
        &self,
        _path: &str,
        _opts: VerifyImageOptions,
    ) -> Result<MediaVerificationResult> {
        Err(media_op_test_only_error("NoopJacsProvider", "verify_image"))
    }

    fn extract_media_signature(&self, _path: &str, _raw_payload: bool) -> Result<Option<String>> {
        Err(media_op_test_only_error(
            "NoopJacsProvider",
            "extract_media_signature",
        ))
    }
}

#[cfg(feature = "jacs-crate")]
impl JacsMediaProvider for StaticJacsProvider {
    fn sign_text_file(&self, _path: &str, _opts: SignTextOptions) -> Result<SignTextOutcome> {
        Err(media_op_test_only_error(
            "StaticJacsProvider",
            "sign_text_file",
        ))
    }

    fn verify_text_file(&self, _path: &str, _opts: VerifyTextOptions) -> Result<VerifyTextResult> {
        Err(media_op_test_only_error(
            "StaticJacsProvider",
            "verify_text_file",
        ))
    }

    fn sign_image(
        &self,
        _in_path: &str,
        _out_path: &str,
        _opts: SignImageOptions,
    ) -> Result<SignedMedia> {
        Err(media_op_test_only_error("StaticJacsProvider", "sign_image"))
    }

    fn verify_image(
        &self,
        _path: &str,
        _opts: VerifyImageOptions,
    ) -> Result<MediaVerificationResult> {
        Err(media_op_test_only_error(
            "StaticJacsProvider",
            "verify_image",
        ))
    }

    fn extract_media_signature(&self, _path: &str, _raw_payload: bool) -> Result<Option<String>> {
        Err(media_op_test_only_error(
            "StaticJacsProvider",
            "extract_media_signature",
        ))
    }
}

// =============================================================================
// Canonical JSON
// =============================================================================

/// Canonical JSON per RFC 8785 (JSON Canonicalization Scheme / JCS).
///
/// When the `jacs-crate` feature is enabled, delegates to `jacs::protocol::canonicalize_json`.
#[cfg(feature = "jacs-crate")]
pub fn canonicalize_json_rfc8785(value: &Value) -> String {
    jacs::protocol::canonicalize_json(value)
}

/// Canonical JSON per RFC 8785 (JSON Canonicalization Scheme / JCS).
///
/// Local fallback when `jacs-crate` feature is not enabled.
/// Requires the `serde_json_canonicalizer` feature.
#[cfg(all(not(feature = "jacs-crate"), feature = "serde_json_canonicalizer"))]
pub fn canonicalize_json_rfc8785(value: &Value) -> String {
    serde_json_canonicalizer::to_string(value).unwrap_or_else(|_| "null".to_string())
}

#[cfg(all(not(feature = "jacs-crate"), not(feature = "serde_json_canonicalizer")))]
compile_error!(
    "Either `jacs-crate` or `serde_json_canonicalizer` feature must be enabled for JSON canonicalization"
);
