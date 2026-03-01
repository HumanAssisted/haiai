use base64::Engine;
use serde_json::Value;

use crate::error::{HaiError, Result};

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
    fn hosted_uses_document_id() {
        let url =
            generate_verify_link_hosted(r#"{"document_id":"abc"}"#, Some("https://example.com/"))
                .expect("hosted");
        assert_eq!(url, "https://example.com/verify/abc");
    }
}
