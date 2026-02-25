use std::collections::HashMap;

use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::{HaiError, Result};
use crate::types::{EmailVerificationResult, KeyRegistryResponse};

pub const MAX_VERIFY_URL_LEN: usize = 2048;
pub const MAX_VERIFY_DOCUMENT_BYTES: usize = 1515;

pub fn generate_verify_link(document: &str, base_url: Option<&str>) -> Result<String> {
    let base = base_url.unwrap_or("https://hai.ai").trim_end_matches('/');
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(document.as_bytes());
    let full_url = format!("{base}/jacs/verify?s={encoded}");

    if full_url.len() > MAX_VERIFY_URL_LEN {
        return Err(HaiError::VerifyUrlTooLong {
            max_len: MAX_VERIFY_URL_LEN,
        });
    }

    Ok(full_url)
}

pub fn generate_verify_link_hosted(document: &str, base_url: Option<&str>) -> Result<String> {
    let base = base_url.unwrap_or("https://hai.ai").trim_end_matches('/');
    let value: Value = serde_json::from_str(document).unwrap_or(Value::Null);
    let doc_id = value
        .get("jacsDocumentId")
        .and_then(Value::as_str)
        .or_else(|| value.get("document_id").and_then(Value::as_str))
        .or_else(|| value.get("id").and_then(Value::as_str));

    let doc_id = doc_id.ok_or(HaiError::MissingHostedDocumentId)?;
    Ok(format!("{base}/verify/{doc_id}"))
}

const MAX_TIMESTAMP_AGE: i64 = 86400; // 24 hours
const MAX_TIMESTAMP_FUTURE: i64 = 300; // 5 minutes

/// Parse the `X-JACS-Signature` header into a map of key=value pairs.
///
/// Format: `v=1; a=ed25519; id=agent-id; t=1740000000; s=base64sig`
pub fn parse_jacs_signature_header(header: &str) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    for part in header.split(';') {
        let part = part.trim();
        if let Some(eq_idx) = part.find('=') {
            let key = part[..eq_idx].trim().to_string();
            let value = part[eq_idx + 1..].trim().to_string();
            fields.insert(key, value);
        }
    }
    fields
}

fn err_result(jacs_id: &str, reputation_tier: &str, error: &str) -> EmailVerificationResult {
    EmailVerificationResult {
        valid: false,
        jacs_id: jacs_id.to_string(),
        reputation_tier: reputation_tier.to_string(),
        error: Some(error.to_string()),
    }
}

/// Determine the signature version and extract the content hash from the
/// parsed `X-JACS-Signature` fields.
///
/// - v2: `h=` field is present in the signature header and carries the content hash.
/// - v1: content hash comes from the separate `X-JACS-Content-Hash` header.
///
/// Returns `(version, content_hash)` or an error string.
pub fn resolve_sig_version(
    fields: &HashMap<String, String>,
    content_hash_header: &str,
) -> std::result::Result<(u8, String), String> {
    let version_str = fields.get("v").map(|s| s.as_str()).unwrap_or("1");
    let version: u8 = version_str.parse().unwrap_or(1);

    match version {
        2 => {
            let h = fields.get("h").map(|s| s.as_str()).unwrap_or("");
            if h.is_empty() {
                return Err("v2 signature missing h= field".to_string());
            }
            Ok((2, h.to_string()))
        }
        1 => {
            if content_hash_header.is_empty() {
                return Err("Missing X-JACS-Content-Hash header".to_string());
            }
            Ok((1, content_hash_header.to_string()))
        }
        _ => Err(format!("Unsupported signature version: {version}")),
    }
}

/// Build the signing payload for a given version.
///
/// - v1: `{content_hash}:{timestamp}`
/// - v2: `{content_hash}:{from}:{timestamp}`
pub fn build_signing_payload(version: u8, content_hash: &str, from: &str, timestamp: i64) -> String {
    match version {
        2 => format!("{content_hash}:{from}:{timestamp}"),
        _ => format!("{content_hash}:{timestamp}"),
    }
}

/// Verify an email's JACS signature.
///
/// This is a standalone async function -- no agent authentication required.
///
/// Supports both v1 and v2 signatures:
/// - **v1**: content hash from `X-JACS-Content-Hash` header; signing payload `{hash}:{timestamp}`
/// - **v2**: content hash from `h=` field in `X-JACS-Signature`; signing payload `{hash}:{from}:{timestamp}`
///
/// For v2 with attachments the `h=` value already covers attachment data,
/// so the verifier does not need raw attachment bytes.
///
/// # Arguments
/// * `headers` - Email headers. Must contain `X-JACS-Signature` and `From`.
///   v1 also requires `X-JACS-Content-Hash`.
/// * `subject` - Email subject line.
/// * `body` - Email body text.
/// * `hai_url` - HAI server URL for public key lookup.
pub async fn verify_email_signature(
    headers: &HashMap<String, String>,
    subject: &str,
    body: &str,
    hai_url: &str,
) -> EmailVerificationResult {
    let hai_url = hai_url.trim_end_matches('/');

    // Step 1: Extract required headers
    let sig_header = headers.get("X-JACS-Signature").map(|s| s.as_str()).unwrap_or("");
    let content_hash_header = headers.get("X-JACS-Content-Hash").map(|s| s.as_str()).unwrap_or("");
    let from_address = headers.get("From").map(|s| s.as_str()).unwrap_or("");

    if sig_header.is_empty() {
        return err_result("", "", "Missing X-JACS-Signature header");
    }
    if from_address.is_empty() {
        return err_result("", "", "Missing From header");
    }

    // Step 2: Parse signature header fields
    let fields = parse_jacs_signature_header(sig_header);
    let jacs_id = fields.get("id").map(|s| s.as_str()).unwrap_or("");
    let timestamp_str = fields.get("t").map(|s| s.as_str()).unwrap_or("");
    let signature_b64 = fields.get("s").map(|s| s.as_str()).unwrap_or("");
    let algorithm = fields.get("a").map(|s| s.as_str()).unwrap_or("ed25519");
    let from_field = fields.get("from").map(|s| s.as_str()).unwrap_or("");

    if jacs_id.is_empty() || timestamp_str.is_empty() || signature_b64.is_empty() {
        return err_result(jacs_id, "", "Incomplete X-JACS-Signature header (missing id, t, or s)");
    }

    if algorithm != "ed25519" {
        return err_result(jacs_id, "", &format!("Unsupported algorithm: {algorithm}"));
    }

    let timestamp: i64 = match timestamp_str.parse() {
        Ok(t) => t,
        Err(_) => return err_result(jacs_id, "", &format!("Invalid timestamp: {timestamp_str}")),
    };

    // Step 3: Determine version and resolve content hash
    let (version, sig_content_hash) = match resolve_sig_version(&fields, content_hash_header) {
        Ok(pair) => pair,
        Err(msg) => return err_result(jacs_id, "", &msg),
    };

    // Step 4: For v1, recompute and compare content hash.
    // For v2, the h= value is trusted once the signature is verified — the
    // signature itself proves the sender committed to that hash.
    if version == 1 {
        let mut hasher = Sha256::new();
        hasher.update(subject.as_bytes());
        hasher.update(b"\n");
        hasher.update(body.as_bytes());
        let computed_hash = format!("sha256:{:x}", hasher.finalize());

        if computed_hash != sig_content_hash {
            return err_result(jacs_id, "", "Content hash mismatch");
        }
    }

    // Step 5: Fetch public key from registry
    // Use `from` field from sig header if present (v2), otherwise fall back to From header
    let lookup_address = if !from_field.is_empty() { from_field } else { from_address };
    let registry_url = format!("{hai_url}/api/agents/keys/{lookup_address}");
    let client = reqwest::Client::new();
    let registry_resp = match client.get(&registry_url).send().await {
        Ok(r) => r,
        Err(e) => return err_result(jacs_id, "", &format!("Failed to fetch public key: {e}")),
    };

    if !registry_resp.status().is_success() {
        return err_result(
            jacs_id,
            "",
            &format!("Registry returned HTTP {}", registry_resp.status().as_u16()),
        );
    }

    let registry_data: KeyRegistryResponse = match registry_resp.json().await {
        Ok(d) => d,
        Err(e) => return err_result(jacs_id, "", &format!("Failed to parse registry response: {e}")),
    };

    let reputation_tier = registry_data.reputation_tier;
    let public_key_pem = registry_data.public_key;
    let registry_jacs_id = registry_data.jacs_id;

    if public_key_pem.is_empty() {
        return err_result(jacs_id, &reputation_tier, "No public key found in registry");
    }
    if registry_jacs_id.trim().is_empty() {
        return err_result(jacs_id, &reputation_tier, "No jacs_id found in registry");
    }
    if registry_jacs_id != jacs_id {
        return err_result(
            &registry_jacs_id,
            &reputation_tier,
            "Signature id does not match registry jacs_id",
        );
    }

    // Parse PEM to get raw Ed25519 public key bytes
    let pem_lines: Vec<&str> = public_key_pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect();
    let der_bytes = match base64::engine::general_purpose::STANDARD.decode(pem_lines.join("")) {
        Ok(b) => b,
        Err(e) => return err_result(&registry_jacs_id, &reputation_tier, &format!("Invalid PEM encoding: {e}")),
    };

    // Ed25519 SPKI: last 32 bytes are the raw public key
    if der_bytes.len() < 32 {
        return err_result(&registry_jacs_id, &reputation_tier, "Public key DER too short");
    }
    let raw_pub_key = &der_bytes[der_bytes.len() - 32..];

    let sig_bytes = match base64::engine::general_purpose::STANDARD.decode(signature_b64) {
        Ok(b) => b,
        Err(_) => {
            match base64::engine::general_purpose::URL_SAFE.decode(signature_b64) {
                Ok(b) => b,
                Err(_) => return err_result(&registry_jacs_id, &reputation_tier, "Invalid signature encoding"),
            }
        }
    };

    // Build version-specific signing payload
    let sign_from = if !from_field.is_empty() { from_field } else { from_address };
    let sign_input = build_signing_payload(version, &sig_content_hash, sign_from, timestamp);
    let verify_key = ring::signature::UnparsedPublicKey::new(
        &ring::signature::ED25519,
        raw_pub_key,
    );
    if verify_key.verify(sign_input.as_bytes(), &sig_bytes).is_err() {
        return err_result(&registry_jacs_id, &reputation_tier, "Signature verification failed");
    }

    // Step 7: Check timestamp freshness
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let age = now - timestamp;
    if age > MAX_TIMESTAMP_AGE {
        return err_result(&registry_jacs_id, &reputation_tier, "Signature timestamp is too old (>24h)");
    }
    if age < -MAX_TIMESTAMP_FUTURE {
        return err_result(
            &registry_jacs_id,
            &reputation_tier,
            "Signature timestamp is too far in the future (>5min)",
        );
    }

    EmailVerificationResult {
        valid: true,
        jacs_id: registry_jacs_id,
        reputation_tier,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine;

    use super::*;

    #[test]
    fn generates_url_safe_link() {
        let url = generate_verify_link(r#"{"k":">>>>"}"#, None).expect("link");
        assert!(url.starts_with("https://hai.ai/jacs/verify?s="));
        let encoded = url.split("?s=").nth(1).expect("encoded");
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
        assert!(!encoded.contains('='));

        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .expect("decode");
        assert_eq!(String::from_utf8(decoded).expect("utf8"), r#"{"k":">>>>"}"#);
    }

    #[test]
    fn parse_jacs_signature_header_fields() {
        let fields = parse_jacs_signature_header("v=1; a=ed25519; id=test-agent; t=1740000000; s=base64sig");
        assert_eq!(fields.get("v").unwrap(), "1");
        assert_eq!(fields.get("a").unwrap(), "ed25519");
        assert_eq!(fields.get("id").unwrap(), "test-agent");
        assert_eq!(fields.get("t").unwrap(), "1740000000");
        assert_eq!(fields.get("s").unwrap(), "base64sig");
    }

    #[test]
    fn verify_missing_sig_header_returns_error() {
        let mut headers = HashMap::new();
        headers.insert("X-JACS-Content-Hash".to_string(), "sha256:abc".to_string());
        headers.insert("From".to_string(), "test@hai.ai".to_string());

        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let result = rt.block_on(verify_email_signature(&headers, "Test", "Body", "https://hai.ai"));
        assert!(!result.valid);
        assert_eq!(result.error.as_deref(), Some("Missing X-JACS-Signature header"));
    }

    #[test]
    fn verify_missing_content_hash_header_returns_error_v1() {
        let mut headers = HashMap::new();
        headers.insert("X-JACS-Signature".to_string(), "v=1; a=ed25519; id=x; t=1; s=abc".to_string());
        headers.insert("From".to_string(), "test@hai.ai".to_string());

        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let result = rt.block_on(verify_email_signature(&headers, "Test", "Body", "https://hai.ai"));
        assert!(!result.valid);
        assert_eq!(result.error.as_deref(), Some("Missing X-JACS-Content-Hash header"));
    }

    #[test]
    fn verify_content_hash_mismatch() {
        let mut headers = HashMap::new();
        headers.insert("X-JACS-Signature".to_string(), "v=1; a=ed25519; id=test; t=1740393600; s=abc".to_string());
        headers.insert("X-JACS-Content-Hash".to_string(), "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_string());
        headers.insert("From".to_string(), "test@hai.ai".to_string());

        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let result = rt.block_on(verify_email_signature(&headers, "Test Subject", "Hello, this is a test email body.", "https://hai.ai"));
        assert!(!result.valid);
        assert_eq!(result.error.as_deref(), Some("Content hash mismatch"));
    }

    #[test]
    fn hosted_uses_document_id() {
        let url =
            generate_verify_link_hosted(r#"{"document_id":"abc"}"#, Some("https://example.com/"))
                .expect("hosted");
        assert_eq!(url, "https://example.com/verify/abc");
    }

    // ── v2 verify support tests ──────────────────────────────────────────

    #[test]
    fn resolve_sig_version_v1_requires_content_hash_header() {
        let fields: HashMap<String, String> = [
            ("v".into(), "1".into()),
        ].into_iter().collect();

        let result = resolve_sig_version(&fields, "");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Missing X-JACS-Content-Hash header");
    }

    #[test]
    fn resolve_sig_version_v1_uses_header() {
        let fields: HashMap<String, String> = [
            ("v".into(), "1".into()),
        ].into_iter().collect();

        let (ver, hash) = resolve_sig_version(&fields, "sha256:abc123").unwrap();
        assert_eq!(ver, 1);
        assert_eq!(hash, "sha256:abc123");
    }

    #[test]
    fn resolve_sig_version_v2_uses_h_field() {
        let fields: HashMap<String, String> = [
            ("v".into(), "2".into()),
            ("h".into(), "sha256:def456".into()),
        ].into_iter().collect();

        let (ver, hash) = resolve_sig_version(&fields, "").unwrap();
        assert_eq!(ver, 2);
        assert_eq!(hash, "sha256:def456");
    }

    #[test]
    fn resolve_sig_version_v2_requires_h_field() {
        let fields: HashMap<String, String> = [
            ("v".into(), "2".into()),
        ].into_iter().collect();

        let result = resolve_sig_version(&fields, "sha256:fallback");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "v2 signature missing h= field");
    }

    #[test]
    fn resolve_sig_version_defaults_to_v1() {
        let fields: HashMap<String, String> = HashMap::new();
        let (ver, hash) = resolve_sig_version(&fields, "sha256:default").unwrap();
        assert_eq!(ver, 1);
        assert_eq!(hash, "sha256:default");
    }

    #[test]
    fn resolve_sig_version_unsupported_version() {
        let fields: HashMap<String, String> = [
            ("v".into(), "99".into()),
        ].into_iter().collect();

        let result = resolve_sig_version(&fields, "sha256:x");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unsupported signature version"));
    }

    #[test]
    fn build_signing_payload_v1_format() {
        let payload = build_signing_payload(1, "sha256:abc", "sender@hai.ai", 1740000000);
        assert_eq!(payload, "sha256:abc:1740000000");
    }

    #[test]
    fn build_signing_payload_v2_format() {
        let payload = build_signing_payload(2, "sha256:abc", "sender@hai.ai", 1740000000);
        assert_eq!(payload, "sha256:abc:sender@hai.ai:1740000000");
    }

    #[test]
    fn parse_v2_header_extracts_all_fields() {
        let header = "v=2; a=ed25519; id=agent-123; from=test@hai.ai; h=sha256:abcdef; t=1740000000; s=base64sig; jv=1.0";
        let fields = parse_jacs_signature_header(header);
        assert_eq!(fields.get("v").unwrap(), "2");
        assert_eq!(fields.get("from").unwrap(), "test@hai.ai");
        assert_eq!(fields.get("h").unwrap(), "sha256:abcdef");
        assert_eq!(fields.get("jv").unwrap(), "1.0");
        assert_eq!(fields.get("id").unwrap(), "agent-123");
        assert_eq!(fields.get("t").unwrap(), "1740000000");
        assert_eq!(fields.get("s").unwrap(), "base64sig");
    }

    #[test]
    fn v2_does_not_require_content_hash_header() {
        // v2 headers: X-JACS-Signature has h= field, no X-JACS-Content-Hash needed
        let mut headers = HashMap::new();
        headers.insert(
            "X-JACS-Signature".to_string(),
            "v=2; a=ed25519; id=test; from=test@hai.ai; h=sha256:abc; t=1740000000; s=abc".to_string(),
        );
        // No X-JACS-Content-Hash
        headers.insert("From".to_string(), "test@hai.ai".to_string());

        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let result = rt.block_on(verify_email_signature(&headers, "Test", "Body", "https://hai.ai"));
        // Should NOT fail with "Missing X-JACS-Content-Hash header"
        // It will fail later (registry fetch), but the point is it gets past header validation
        assert!(result.error.as_deref() != Some("Missing X-JACS-Content-Hash header"));
    }

    #[test]
    fn v1_still_requires_content_hash_header() {
        let mut headers = HashMap::new();
        headers.insert(
            "X-JACS-Signature".to_string(),
            "v=1; a=ed25519; id=test; t=1740000000; s=abc".to_string(),
        );
        headers.insert("From".to_string(), "test@hai.ai".to_string());

        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let result = rt.block_on(verify_email_signature(&headers, "Test", "Body", "https://hai.ai"));
        assert!(!result.valid);
        assert_eq!(result.error.as_deref(), Some("Missing X-JACS-Content-Hash header"));
    }
}
