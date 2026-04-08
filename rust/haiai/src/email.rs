//! HAI email verification wrapper.
//!
//! This module adds HAI-specific trust chain verification on top of JACS's
//! neutral email signing functions. It handles registry lookup, DNS
//! verification, and identity binding -- without duplicating any MIME parsing
//! or cryptography (those live in JACS).

use base64::Engine;
use sha2::{Digest, Sha256};

use crate::error::{HaiError, Result};
use crate::types::KeyRegistryResponse;
use crate::types::{ChainEntry, EmailVerificationResultV2, FieldResult, FieldStatus};

use jacs::simple::{CreateAgentParams, SimpleAgent};

// Re-export JACS email types for consumer convenience.
pub use jacs::email::{
    sign_email, AttachmentEntry, BodyPartEntry, ContentVerificationResult, EmailSignatureHeaders,
    EmailSignaturePayload, JacsEmailMetadata, JacsEmailSignature, JacsEmailSignatureDocument,
    ParsedAttachment, ParsedBodyPart, ParsedEmailParts, SignedHeaderEntry,
};

use jacs::email::{get_jacs_attachment, verify_email_content, verify_email_document};

/// Input for a single attachment in [`compute_content_hash`].
pub struct AttachmentInput {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

/// Compute a deterministic content hash for email content.
///
/// This produces a hash that all SDKs must agree on for the same inputs.
/// The algorithm uses JACS's `compute_attachment_hash` convention internally:
///
/// 1. Compute per-attachment hash: `sha256(filename_utf8 + ":" + content_type_lower + ":" + raw_bytes)`
/// 2. Sort attachment hashes lexicographically
/// 3. Compute overall hash:
///    - No attachments: `sha256(subject + "\n" + body)`
///    - With attachments: `sha256(subject + "\n" + body + "\n" + sorted_hashes.join("\n"))`
///
/// Returns `"sha256:<hex>"` format.
pub fn compute_content_hash(subject: &str, body: &str, attachments: &[AttachmentInput]) -> String {
    // Compute per-attachment hashes using JACS convention
    let mut att_hashes: Vec<String> = attachments
        .iter()
        .map(|att| {
            let content_type_lower = att.content_type.to_lowercase();
            let mut h = Sha256::new();
            h.update(att.filename.as_bytes());
            h.update(b":");
            h.update(content_type_lower.as_bytes());
            h.update(b":");
            h.update(&att.data);
            format!("sha256:{:x}", h.finalize())
        })
        .collect();
    att_hashes.sort();

    // Compute overall content hash
    let mut h = Sha256::new();
    h.update(subject.as_bytes());
    h.update(b"\n");
    h.update(body.as_bytes());
    for ah in &att_hashes {
        h.update(b"\n");
        h.update(ah.as_bytes());
    }
    format!("sha256:{:x}", h.finalize())
}

fn convert_field_result(value: jacs::email::FieldResult) -> FieldResult {
    let json = serde_json::to_value(value).expect("FieldResult should serialize");
    serde_json::from_value(json).expect("FieldResult should match SDK schema")
}

fn convert_chain_entry(value: jacs::email::ChainEntry) -> ChainEntry {
    let json = serde_json::to_value(value).expect("ChainEntry should serialize");
    serde_json::from_value(json).expect("ChainEntry should match SDK schema")
}

/// Verify a raw RFC 5322 email with JACS attachment signature.
///
/// This is the primary HAI verification API. It performs:
/// 1. JACS signature extraction and document validation
/// 2. HAI registry lookup for the signer's public key
/// 3. Identity binding checks (PRD lines 681-694)
/// 4. Cryptographic signature verification (delegated to JACS)
/// 5. DNS verification (for pro and enterprise tiers)
/// 6. Content hash comparison
/// 7. Forwarding chain verification
///
/// # Arguments
/// * `raw_email` - Raw RFC 5322 email bytes (with JACS attachment)
/// * `hai_url` - HAI server URL for registry lookup (e.g., "https://hai.ai")
pub async fn verify_email(raw_email: &[u8], hai_url: &str) -> EmailVerificationResultV2 {
    let hai_url = hai_url.trim_end_matches('/');

    // Step 1: Extract the JACS signature attachment to get metadata
    let jacs_bytes = match get_jacs_attachment(raw_email) {
        Ok(b) => b,
        Err(e) => {
            return EmailVerificationResultV2::err(
                "",
                "",
                &format!("No JACS signature found: {e}"),
            );
        }
    };

    // Step 2: Parse the JACS envelope to get signer info.
    // The JACS attachment is a JACS envelope (jacsType, content, jacsSignature, ...)
    // not a JacsEmailSignatureDocument. We extract the fields we need directly.
    let jacs_value: serde_json::Value = match serde_json::from_slice(&jacs_bytes) {
        Ok(v) => v,
        Err(e) => {
            return EmailVerificationResultV2::err(
                "",
                "",
                &format!("Invalid JACS signature document: {e}"),
            );
        }
    };

    let jacs_id = jacs_value
        .get("jacsSignature")
        .and_then(|sig| sig.get("agentID"))
        .and_then(|id| id.as_str())
        .unwrap_or("")
        .to_string();

    let content = match jacs_value.get("content") {
        Some(c) => c,
        None => {
            return EmailVerificationResultV2::err(
                &jacs_id,
                "",
                "JACS document missing 'content' field",
            );
        }
    };

    let pre_payload: EmailSignaturePayload = match serde_json::from_value(content.clone()) {
        Ok(p) => p,
        Err(e) => {
            return EmailVerificationResultV2::err(
                &jacs_id,
                "",
                &format!("Invalid email payload in JACS document: {e}"),
            );
        }
    };

    let from_email = pre_payload.headers.from.value.clone();
    let sig_algorithm = jacs_value
        .get("jacsSignature")
        .and_then(|sig| sig.get("signingAlgorithm"))
        .and_then(|a| a.as_str())
        .unwrap_or("")
        .to_string();

    // Step 3: Fetch public key from HAI registry
    let registry = match fetch_public_key_from_registry(hai_url, &from_email).await {
        Ok(r) => r,
        Err(e) => {
            return EmailVerificationResultV2::err(
                &jacs_id,
                "",
                &format!("Registry lookup failed: {e}"),
            );
        }
    };

    let reputation_tier = &registry.reputation_tier;

    // Step 4: Identity binding checks (PRD lines 681-694)
    // 4a: metadata.issuer must match registry.jacs_id
    if jacs_id != registry.jacs_id {
        return EmailVerificationResultV2::err(
            &jacs_id,
            reputation_tier,
            &format!(
                "Identity mismatch: document issuer '{}' does not match registry jacs_id '{}'",
                jacs_id, registry.jacs_id
            ),
        );
    }

    // 4b: payload.headers.from.value must match registry email
    if from_email != registry.email {
        return EmailVerificationResultV2::err(
            &jacs_id,
            reputation_tier,
            &format!(
                "Identity mismatch: From '{}' does not match registry email '{}'",
                from_email, registry.email
            ),
        );
    }

    // 4c: signature.algorithm must match registry algorithm
    if !algorithms_match(&sig_algorithm, &registry.algorithm) {
        return EmailVerificationResultV2::err(
            &jacs_id,
            reputation_tier,
            &format!(
                "Algorithm mismatch: signature uses '{}' but registry has '{}'",
                sig_algorithm, registry.algorithm
            ),
        );
    }

    // 4d: Check agent status from registry (reject suspended/revoked agents)
    let agent_status = registry.agent_status.clone();
    let benchmarks_completed = registry.benchmarks_completed.clone().unwrap_or_default();
    if let Some(ref status) = agent_status {
        if status != "active" {
            return EmailVerificationResultV2 {
                agent_status: agent_status.clone(),
                benchmarks_completed: benchmarks_completed.clone(),
                ..EmailVerificationResultV2::err(
                    &jacs_id,
                    reputation_tier,
                    &format!(
                        "Agent status is '{}' -- only active agents can send verified email",
                        status
                    ),
                )
            };
        }
    }

    // Step 5: Parse PEM to get raw public key bytes
    let raw_pub_key = match extract_public_key_bytes(&registry.public_key) {
        Ok(k) => k,
        Err(e) => {
            return EmailVerificationResultV2::err(
                &jacs_id,
                reputation_tier,
                &format!("Failed to parse public key: {e}"),
            );
        }
    };

    // Step 6: Verify the JACS document (crypto verification + hash check)
    // This internally removes the JACS attachment before parsing (PRD line 473)
    let agent = match create_verification_agent() {
        Ok(a) => a,
        Err(e) => {
            return EmailVerificationResultV2::err(
                &jacs_id,
                reputation_tier,
                &format!("Failed to create verification agent: {e}"),
            );
        }
    };
    let (trusted_doc, parts) = match verify_email_document(raw_email, &agent, &raw_pub_key) {
        Ok(result) => result,
        Err(e) => {
            return EmailVerificationResultV2::err(
                &jacs_id,
                reputation_tier,
                &format!("JACS signature verification failed: {e}"),
            );
        }
    };

    // Step 7: DNS verification (for pro and enterprise tiers)
    let dns_verified = if reputation_tier == "pro" || reputation_tier == "enterprise"
    {
        let domain = extract_domain(&from_email);
        match verify_dns_public_key(&domain, &registry.public_key).await {
            Ok(verified) => {
                if !verified {
                    return EmailVerificationResultV2::err(
                        &jacs_id,
                        reputation_tier,
                        "DNS public key hash does not match registry key",
                    );
                }
                Some(true)
            }
            Err(e) => {
                return EmailVerificationResultV2::err(
                    &jacs_id,
                    reputation_tier,
                    &format!("DNS verification failed: {e}"),
                );
            }
        }
    } else {
        None // DNS check skipped for free tier
    };

    // Step 8: Content hash comparison
    let content_result = verify_email_content(&trusted_doc, &parts);
    let field_results = content_result
        .field_results
        .into_iter()
        .map(convert_field_result)
        .collect::<Vec<_>>();

    // Step 9: Verify parent chain entries cryptographically.
    // JACS returns parent chain entries with valid=false because it lacks
    // the parent signers' public keys. We upgrade them here by looking up
    // each parent signer from the registry and verifying their signature.
    let mut chain = content_result
        .chain
        .into_iter()
        .map(convert_chain_entry)
        .collect::<Vec<_>>();
    verify_parent_chain_entries(&mut chain, &parts, hai_url, &agent).await;

    // Recompute overall validity: fields must pass AND all chain entries valid
    let fields_valid = !field_results.iter().any(|r| r.status == FieldStatus::Fail);
    let chain_valid = chain.iter().all(|entry| entry.valid);
    let valid = fields_valid && chain_valid;

    EmailVerificationResultV2 {
        valid,
        jacs_id,
        algorithm: sig_algorithm,
        reputation_tier: reputation_tier.clone(),
        dns_verified,
        field_results,
        chain,
        error: None,
        agent_status,
        benchmarks_completed,
    }
}

/// Verify parent chain entries by looking up each signer's public key
/// and verifying their JACS document signature.
async fn verify_parent_chain_entries(
    chain: &mut [ChainEntry],
    parts: &ParsedEmailParts,
    hai_url: &str,
    agent: &SimpleAgent,
) {
    // Skip the first chain entry (the current signer, already verified)
    for entry in chain.iter_mut().skip(1) {
        if entry.valid {
            continue; // Already verified
        }

        // Find the parent JACS attachment raw bytes that match this signer.
        // Parent attachments are JACS envelopes, not JacsEmailSignatureDocuments.
        let parent_att = parts.jacs_attachments.iter().find(|att| {
            serde_json::from_slice::<serde_json::Value>(&att.content)
                .ok()
                .and_then(|v| {
                    v.get("jacsSignature")
                        .and_then(|sig| sig.get("agentID"))
                        .and_then(|id| id.as_str())
                        .map(|id| id == entry.jacs_id)
                })
                .unwrap_or(false)
        });

        let Some(parent_att) = parent_att else {
            continue; // Can't find the parent document
        };

        // Parse the parent JACS envelope for identity/algorithm checks
        let parent_value: serde_json::Value = match serde_json::from_slice(&parent_att.content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let parent_issuer = parent_value
            .get("jacsSignature")
            .and_then(|sig| sig.get("agentID"))
            .and_then(|id| id.as_str())
            .unwrap_or("");

        let parent_content = match parent_value.get("content") {
            Some(c) => c,
            None => continue,
        };

        let parent_payload: EmailSignaturePayload =
            match serde_json::from_value(parent_content.clone()) {
                Ok(p) => p,
                Err(_) => continue,
            };

        let parent_algorithm = parent_value
            .get("jacsSignature")
            .and_then(|sig| sig.get("signingAlgorithm"))
            .and_then(|a| a.as_str())
            .unwrap_or("");

        // Look up the parent signer's public key from the registry
        let parent_email = &parent_payload.headers.from.value;
        let registry = match fetch_public_key_from_registry(hai_url, parent_email).await {
            Ok(r) => r,
            Err(_) => continue, // Registry lookup failed, leave as invalid
        };

        // Check identity binding
        if parent_issuer != registry.jacs_id {
            continue; // issuer mismatch -- leave as invalid
        }

        // Check algorithm binding (matches top-level check)
        if !algorithms_match(parent_algorithm, &registry.algorithm) {
            continue; // algorithm mismatch -- leave as invalid
        }

        // Extract public key bytes
        let raw_pub_key = match extract_public_key_bytes(&registry.public_key) {
            Ok(k) => k,
            Err(_) => continue,
        };

        // Verify the parent document using SimpleAgent::verify_with_key.
        // This handles hash verification AND cryptographic signature check.
        let parent_json = match std::str::from_utf8(&parent_att.content) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if agent
            .verify_with_key(parent_json, raw_pub_key)
            .is_ok_and(|r| r.valid)
        {
            entry.valid = true;
        }
    }
}

/// Create an ephemeral `SimpleAgent` for verification infrastructure.
///
/// The agent's own key material is not used during `verify_with_key` or
/// `verify_email_document` -- only the caller-supplied public key matters.
/// We create a lightweight Ed25519 agent in a temp directory to satisfy
/// the JACS infrastructure requirements (schema loading, document parsing).
fn create_verification_agent() -> Result<SimpleAgent> {
    let tmp = tempfile::tempdir().map_err(|e| {
        HaiError::Provider(format!(
            "Failed to create temp directory for verification agent: {e}"
        ))
    })?;
    let tmp_path = tmp.path().to_string_lossy().to_string();

    let params = CreateAgentParams::builder()
        .name("hai-verifier")
        .password("hai-verify-ephemeral")
        .algorithm("ring-Ed25519")
        .data_directory(&format!("{}/jacs_data", tmp_path))
        .key_directory(&format!("{}/jacs_keys", tmp_path))
        .config_path(&format!("{}/jacs.config.json", tmp_path))
        .build();

    let (agent, _info) = SimpleAgent::create_with_params(params)
        .map_err(|e| HaiError::Provider(format!("Failed to create verification agent: {e}")))?;

    // Keep the temp directory alive for the agent's lifetime by leaking it.
    // The OS will reclaim it when the process exits. This avoids the temp dir
    // being deleted while the agent still references files in it.
    std::mem::forget(tmp);

    Ok(agent)
}

/// Fetch the public key and metadata from the HAI registry for a given email.
///
/// Calls `GET /api/agents/keys/{email}` on the HAI server.
pub async fn fetch_public_key_from_registry(
    hai_url: &str,
    email: &str,
) -> Result<KeyRegistryResponse> {
    let encoded_email =
        percent_encoding::utf8_percent_encode(email, percent_encoding::NON_ALPHANUMERIC);
    let url = format!("{}/api/agents/keys/{}", hai_url, encoded_email);
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| HaiError::Provider(format!("Failed to fetch public key: {e}")))?;

    if !resp.status().is_success() {
        return Err(HaiError::Provider(format!(
            "Registry returned HTTP {}",
            resp.status().as_u16()
        )));
    }

    let registry: KeyRegistryResponse = resp
        .json()
        .await
        .map_err(|e| HaiError::Provider(format!("Failed to parse registry response: {e}")))?;

    if registry.public_key.is_empty() {
        return Err(HaiError::Provider("No public key found in registry".into()));
    }

    Ok(registry)
}

/// Verify that a DNS TXT record at `_v1.agent.jacs.{domain}` contains
/// a `jacs_public_key_hash=` value matching the SHA-256 hash of the
/// public key PEM bytes.
///
/// Returns `Ok(true)` if verified, `Ok(false)` if the hash doesn't match,
/// or `Err` if the DNS lookup fails.
pub async fn verify_dns_public_key(domain: &str, public_key_pem: &str) -> Result<bool> {
    // Compute expected hash: sha256(public_key_pem_bytes), base64 encoded
    let expected_hash = {
        let mut hasher = Sha256::new();
        hasher.update(public_key_pem.as_bytes());
        base64::engine::general_purpose::STANDARD.encode(hasher.finalize())
    };

    // Query DNS TXT record at _v1.agent.jacs.{domain}
    // Use DNS-over-HTTPS (Google's public resolver) since we don't have a
    // native DNS TXT record library as a dependency.
    let txt_name = format!("_v1.agent.jacs.{domain}");
    let txt_records = fetch_dns_txt_records(&txt_name).await?;

    for record in &txt_records {
        // Look for jacs_public_key_hash= in the TXT record
        for part in record.split(';') {
            let part = part.trim();
            if let Some(hash_value) = part.strip_prefix("jacs_public_key_hash=") {
                let hash_value = hash_value.trim();
                if hash_value == expected_hash {
                    return Ok(true);
                }
                // Found the field but hash doesn't match
                return Ok(false);
            }
        }
    }

    // No jacs_public_key_hash field found in any TXT record
    Ok(false)
}

/// Fetch DNS TXT records using DNS-over-HTTPS.
async fn fetch_dns_txt_records(name: &str) -> Result<Vec<String>> {
    use crate::client::DEFAULT_DNS_RESOLVER;
    let url = format!(
        "{}?name={}&type=TXT",
        DEFAULT_DNS_RESOLVER,
        percent_encoding::utf8_percent_encode(name, percent_encoding::NON_ALPHANUMERIC)
    );

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("Accept", "application/dns-json")
        .send()
        .await
        .map_err(|e| HaiError::Provider(format!("DNS lookup failed: {e}")))?;

    if !resp.status().is_success() {
        return Err(HaiError::Provider(format!(
            "DNS lookup returned HTTP {}",
            resp.status().as_u16()
        )));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| HaiError::Provider(format!("Failed to parse DNS response: {e}")))?;

    let mut records = Vec::new();
    if let Some(answers) = body.get("Answer").and_then(|a| a.as_array()) {
        for answer in answers {
            if let Some(data) = answer.get("data").and_then(|d| d.as_str()) {
                // DNS TXT data is often quoted; strip outer quotes
                let clean = data.trim_matches('"');
                records.push(clean.to_string());
            }
        }
    }

    Ok(records)
}

/// Extract the domain part from an email address.
fn extract_domain(email: &str) -> String {
    // Handle angle bracket formats like "Name <user@domain.com>"
    let clean = email
        .rfind('<')
        .and_then(|start| {
            email[start + 1..]
                .find('>')
                .map(|end| &email[start + 1..start + 1 + end])
        })
        .unwrap_or(email);

    clean
        .rfind('@')
        .map(|pos| &clean[pos + 1..])
        .unwrap_or(clean)
        .to_string()
}

/// Extract public key bytes from a PEM-encoded public key.
///
/// Detects the algorithm from the SPKI DER structure:
/// - Ed25519 (OID 1.3.101.112): extracts the 32-byte raw public key
/// - Everything else (RSA, PQ2025): returns the full DER-encoded SubjectPublicKeyInfo
///
/// The JACS `SimpleAgent::verify_with_key()` handles per-algorithm format
/// conversion internally, so callers can pass these bytes directly.
fn extract_public_key_bytes(pem: &str) -> Result<Vec<u8>> {
    let pem_lines: Vec<&str> = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    let der_bytes = base64::engine::general_purpose::STANDARD
        .decode(pem_lines.join(""))
        .map_err(|e| HaiError::Provider(format!("Invalid PEM encoding: {e}")))?;

    if der_bytes.len() < 12 {
        return Err(HaiError::Provider("Public key DER too short".into()));
    }

    // Detect Ed25519 by looking for OID 1.3.101.112 (hex: 06 03 2b 65 70)
    // in the SPKI AlgorithmIdentifier.
    let ed25519_oid: &[u8] = &[0x06, 0x03, 0x2b, 0x65, 0x70];
    let is_ed25519 = der_bytes
        .windows(ed25519_oid.len())
        .any(|w| w == ed25519_oid);

    if is_ed25519 {
        // Ed25519 SPKI: the last 32 bytes are the raw public key
        if der_bytes.len() < 32 {
            return Err(HaiError::Provider(
                "Ed25519 DER too short for 32-byte key".into(),
            ));
        }
        Ok(der_bytes[der_bytes.len() - 32..].to_vec())
    } else {
        // RSA (or other algorithms): return full DER for the JACS verifier
        Ok(der_bytes)
    }
}

/// Check if two algorithm names refer to the same algorithm.
///
/// Uses JACS's `normalize_algorithm` for consistent normalization across the stack.
fn algorithms_match(a: &str, b: &str) -> bool {
    jacs::email::normalize_algorithm(a) == jacs::email::normalize_algorithm(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_domain_simple() {
        assert_eq!(extract_domain("agent@example.com"), "example.com");
    }

    #[test]
    fn extract_domain_with_angle_brackets() {
        assert_eq!(extract_domain("Agent <agent@example.com>"), "example.com");
    }

    #[test]
    fn extract_domain_no_at_sign() {
        assert_eq!(extract_domain("nodomain"), "nodomain");
    }

    #[test]
    fn algorithms_match_ed25519_variants() {
        assert!(algorithms_match("ed25519", "ed25519"));
        assert!(algorithms_match("ed25519", "ring-ed25519"));
        assert!(algorithms_match("ring-ed25519", "ed25519"));
    }

    #[test]
    fn algorithms_match_rsa_variants() {
        assert!(algorithms_match("rsa-pss", "rsa-pss"));
        assert!(algorithms_match("rsa-pss", "rsa-pss-sha256"));
    }

    #[test]
    fn algorithms_mismatch() {
        assert!(!algorithms_match("ed25519", "rsa-pss"));
    }

    #[test]
    fn err_result_sets_fields() {
        let r = EmailVerificationResultV2::err("agent:v1", "free_chaotic", "test error");
        assert!(!r.valid);
        assert_eq!(r.jacs_id, "agent:v1");
        assert_eq!(r.reputation_tier, "free_chaotic");
        assert_eq!(r.error.as_deref(), Some("test error"));
        assert!(r.field_results.is_empty());
        assert!(r.chain.is_empty());
        assert!(r.dns_verified.is_none());
    }

    // -- Tests that use JACS email functions with SimpleAgent --

    use super::{CreateAgentParams, SimpleAgent};

    /// Create a test SimpleAgent for email signing/verification tests.
    ///
    /// Returns the agent and a TempDir that must be kept alive for the
    /// agent's lifetime (dropping it deletes the key files).
    fn create_test_agent(name: &str) -> (SimpleAgent, tempfile::TempDir) {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let tmp_path = tmp.path().to_string_lossy().to_string();

        let params = CreateAgentParams::builder()
            .name(name)
            .password("TestHaiSdk!2026")
            .algorithm("ring-Ed25519")
            .data_directory(&format!("{}/jacs_data", tmp_path))
            .key_directory(&format!("{}/jacs_keys", tmp_path))
            .config_path(&format!("{}/jacs.config.json", tmp_path))
            .build();

        let (agent, _info) = SimpleAgent::create_with_params(params).expect("create test agent");

        // Set env vars needed by the keystore at signing time.
        // SAFETY: tests that sign emails must not run in parallel
        // or they will stomp on each other's env vars.
        unsafe {
            std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", "TestHaiSdk!2026");
            std::env::set_var("JACS_KEY_DIRECTORY", format!("{}/jacs_keys", tmp_path));
            std::env::set_var("JACS_AGENT_PRIVATE_KEY_FILENAME", "jacs.private.pem.enc");
        }

        (agent, tmp)
    }

    #[test]
    fn sign_email_and_extract_doc() {
        let email = b"From: sender@example.com\r\nTo: recipient@example.com\r\nSubject: Test\r\nDate: Fri, 28 Feb 2026 12:00:00 +0000\r\nMessage-ID: <test@example.com>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nHello World\r\n";
        let (agent, _tmp) = create_test_agent("test-agent");
        let signed = sign_email(email, &agent).unwrap();

        // The JACS attachment is a JACS envelope, not a JacsEmailSignatureDocument.
        let doc_bytes = get_jacs_attachment(&signed).unwrap();
        let jacs_doc: serde_json::Value = serde_json::from_slice(&doc_bytes).unwrap();

        // Verify JACS envelope structure
        assert_eq!(jacs_doc["jacsType"].as_str(), Some("message"));
        assert!(jacs_doc.get("jacsId").is_some(), "should have jacsId");
        assert!(
            jacs_doc.get("jacsSignature").is_some(),
            "should have jacsSignature"
        );
        assert!(
            jacs_doc.get("content").is_some(),
            "should have content field"
        );

        // Verify the email payload is in the content field
        let payload: EmailSignaturePayload =
            serde_json::from_value(jacs_doc["content"].clone()).unwrap();
        assert!(!payload.headers.from.value.is_empty());

        // The JACS agent ID is assigned by SimpleAgent, not a fixed string.
        let agent_id = jacs_doc["jacsSignature"]["agentID"].as_str().unwrap_or("");
        assert!(!agent_id.is_empty());
    }

    #[tokio::test]
    async fn verify_email_missing_jacs_attachment() {
        let email = b"From: sender@example.com\r\nTo: recipient@example.com\r\nSubject: Test\r\nDate: Fri, 28 Feb 2026 12:00:00 +0000\r\nMessage-ID: <test@example.com>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nHello World\r\n";

        let result = verify_email(email, "http://127.0.0.1:1").await;
        assert!(!result.valid);
        assert!(result
            .error
            .as_deref()
            .unwrap()
            .contains("No JACS signature found"));
    }

    #[tokio::test]
    async fn verify_email_registry_unreachable() {
        let email = b"From: sender@example.com\r\nTo: recipient@example.com\r\nSubject: Test\r\nDate: Fri, 28 Feb 2026 12:00:00 +0000\r\nMessage-ID: <test@example.com>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nHello World\r\n";
        let (agent, _tmp) = create_test_agent("test-agent");
        let signed = sign_email(email, &agent).unwrap();

        let result = verify_email(&signed, "http://127.0.0.1:1").await;
        assert!(!result.valid);
        assert!(result
            .error
            .as_deref()
            .unwrap()
            .contains("Registry lookup failed"));
    }

    #[tokio::test]
    async fn verify_email_with_mock_registry_identity_mismatch() {
        let (agent, _tmp) = create_test_agent("test-agent");

        // Get the agent's JACS ID so we can set up a mismatching registry
        let email = b"From: sender@example.com\r\nTo: recipient@example.com\r\nSubject: Test\r\nDate: Fri, 28 Feb 2026 12:00:00 +0000\r\nMessage-ID: <test@example.com>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nHello World\r\n";
        let signed = sign_email(email, &agent).unwrap();

        // This test uses httpmock to simulate a registry that returns a different jacs_id
        let server = httpmock::MockServer::start();
        server.mock(|when, then| {
            when.method("GET")
                .path_includes("/api/agents/keys/");
            then.status(200)
                .json_body(serde_json::json!({
                    "email": "sender@example.com",
                    "jacs_id": "wrong-agent:v1",  // Doesn't match signer
                    "public_key": "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEAMjAxMjAxMjAxMjAxMjAxMjAxMjAxMjAxMjAxMjAxMjA=\n-----END PUBLIC KEY-----",
                    "algorithm": "ed25519",
                    "reputation_tier": "free_chaotic",
                    "registered_at": "2026-01-01T00:00:00Z"
                }));
        });

        let result = verify_email(&signed, &server.base_url()).await;
        assert!(!result.valid);
        assert!(result
            .error
            .as_deref()
            .unwrap()
            .contains("Identity mismatch"));
        assert!(result.error.as_deref().unwrap().contains("issuer"));
    }

    #[tokio::test]
    async fn verify_email_with_mock_registry_email_mismatch() {
        let (agent, _tmp) = create_test_agent("test-agent");

        let email = b"From: sender@example.com\r\nTo: recipient@example.com\r\nSubject: Test\r\nDate: Fri, 28 Feb 2026 12:00:00 +0000\r\nMessage-ID: <test@example.com>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nHello World\r\n";
        let signed = sign_email(email, &agent).unwrap();

        // Extract the real JACS agent ID from the signed document so the
        // issuer check passes and only the email mismatch check triggers.
        let doc_bytes = get_jacs_attachment(&signed).unwrap();
        let jacs_doc: serde_json::Value = serde_json::from_slice(&doc_bytes).unwrap();
        let real_agent_id = jacs_doc["jacsSignature"]["agentID"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        let server = httpmock::MockServer::start();
        server.mock(|when, then| {
            when.method("GET")
                .path_includes("/api/agents/keys/");
            then.status(200)
                .json_body(serde_json::json!({
                    "email": "different@example.com",  // Doesn't match From
                    "jacs_id": real_agent_id,
                    "public_key": "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEAMjAxMjAxMjAxMjAxMjAxMjAxMjAxMjAxMjAxMjAxMjA=\n-----END PUBLIC KEY-----",
                    "algorithm": "ed25519",
                    "reputation_tier": "free_chaotic",
                    "registered_at": "2026-01-01T00:00:00Z"
                }));
        });

        let result = verify_email(&signed, &server.base_url()).await;
        assert!(!result.valid);
        assert!(result
            .error
            .as_deref()
            .unwrap()
            .contains("Identity mismatch"));
        assert!(result.error.as_deref().unwrap().contains("From"));
    }

    #[test]
    fn registry_url_encodes_special_email_chars() {
        // Verify that percent_encoding properly encodes special characters
        // in email addresses used for registry lookups (Issue 038).
        let encoded: String = percent_encoding::utf8_percent_encode(
            "agent+tag@example.com",
            percent_encoding::NON_ALPHANUMERIC,
        )
        .to_string();
        assert!(
            !encoded.contains('+'),
            "'+' should be percent-encoded in URL path, got: {encoded}"
        );
        assert!(
            encoded.contains("%2B") || encoded.contains("%2b"),
            "'+' should become %2B, got: {encoded}"
        );
        // '@' should also be encoded
        assert!(
            encoded.contains("%40"),
            "'@' should be percent-encoded, got: {encoded}"
        );
    }
}
