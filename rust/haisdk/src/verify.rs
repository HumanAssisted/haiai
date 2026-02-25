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

/// Verify an email's JACS signature.
///
/// This is a standalone async function -- no agent authentication required.
///
/// # Arguments
/// * `headers` - Email headers. Must contain `X-JACS-Signature`,
///   `X-JACS-Content-Hash`, and `From`.
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
    if content_hash_header.is_empty() {
        return err_result("", "", "Missing X-JACS-Content-Hash header");
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

    // Step 3: Recompute content hash
    let mut hasher = Sha256::new();
    hasher.update(subject.as_bytes());
    hasher.update(b"\n");
    hasher.update(body.as_bytes());
    let computed_hash = format!("sha256:{:x}", hasher.finalize());

    // Step 4: Compare content hashes
    if computed_hash != content_hash_header {
        return err_result(jacs_id, "", "Content hash mismatch");
    }

    // Step 5: Fetch public key from registry
    let registry_url = format!("{hai_url}/api/agents/keys/{from_address}");
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

    let sign_input = format!("{computed_hash}:{timestamp}");
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
    fn verify_missing_content_hash_header_returns_error() {
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
}
