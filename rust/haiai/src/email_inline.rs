//! Shared constants for HTML-inline signed email generation.

pub const HTML_INLINE_JACS_TRANSPORT: &str = "html_inline_jacs";
pub const ATTACHMENT_JACS_TRANSPORT: &str = "attachment_jacs";

pub const HAI_JACS_ENVELOPE_MARKER: &str = "data-hai-jacs-envelope";
pub const HAI_VERIFY_FOOTER_MARKER: &str = "data-hai-verify-footer";
pub const HAI_VERIFY_LINK_MARKER: &str = "data-hai-verify-link";
pub const HAI_LOGO_VERIFY_LINK_MARKER: &str = "data-hai-logo-verify-link";
pub const HAI_MARKER_VERSION: &str = "v1";

pub const HAI_JACS_ENVELOPE_SCRIPT_TYPE: &str = "application/jacs+json";
pub const HAI_JACS_ENVELOPE_SCRIPT_PREFIX: &str = "<script type=\"application/jacs+json\"";

pub const HAI_HTML_TEMPLATE_VERSION_MARKER: &str = "data-hai-template-version";
pub const HAI_HTML_TEMPLATE_VERSION: &str = "v1";

pub const HAI_JACS_LOGO_CID: &str = "hai-jacs-logo@hai.ai";
pub const HAI_JACS_LOGO_CONTENT_ID_HEADER: &str = "<hai-jacs-logo@hai.ai>";
pub const HAI_JACS_LOGO_CONTENT_DISPOSITION: &str = "inline";
pub const HAI_JACS_LOGO_CONTENT_TYPE: &str = "image/png";
pub const HAI_JACS_LOGO_FILENAME: &str = "hai-jacs-logo.png";
pub const HAI_JACS_LOGO_BYTES: &[u8] = include_bytes!("../assets/hai-jacs-logo.png");
/// Canonical SHA-256 of the bundled HAI verification logo, in lowercase hex.
/// HAI server-side code must verify equality against this constant in tests so
/// any unilateral asset change is caught in CI.
pub const HAI_JACS_LOGO_SHA256_HEX: &str =
    "4bbe834af55a42fcc600cefcf5cd50b1e56afec5b97785391ccabe30985e0dff";

pub const HAI_VERIFY_FOOTER_TEXT_TEMPLATE: &str =
    "This email is sent from an AI agent. Verify at [verify link]";

pub const HAI_HIDDEN_ENVELOPE_MAX_BYTES: usize = 8 * 1024;
pub const HAI_SIGNED_LOGO_SIZE_BYTE_CAP: Option<usize> = None;
pub const HAI_INLINE_SIZE_METRIC_HIDDEN_ENVELOPE: &str = "hidden_envelope";
pub const HAI_INLINE_SIZE_METRIC_SIGNED_LOGO: &str = "signed_logo";

pub const HAI_RESERVED_INLINE_EMAIL_MARKERS: &[&str] = &[
    HAI_JACS_ENVELOPE_MARKER,
    HAI_VERIFY_LINK_MARKER,
    HAI_LOGO_VERIFY_LINK_MARKER,
    HAI_VERIFY_FOOTER_MARKER,
    HAI_JACS_LOGO_CID,
    HAI_JACS_ENVELOPE_SCRIPT_PREFIX,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedInlineLogo {
    pub bytes: Vec<u8>,
    pub size_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlInlineJacsEnvelope {
    pub hidden_envelope: String,
    pub compact_header: String,
    pub hidden_envelope_size_bytes: usize,
}

pub fn build_hidden_jacs_envelope(
    signed_jacs_envelope: &str,
) -> crate::error::Result<HtmlInlineJacsEnvelope> {
    let compact_header = format!("sha256:{}", sha256_hex(signed_jacs_envelope.as_bytes()));
    let jacs_envelope = serde_json::from_str::<serde_json::Value>(signed_jacs_envelope)?;
    let hidden_value = serde_json::json!({
        "compactHeader": compact_header,
        "jacsEnvelope": jacs_envelope,
    });
    let hidden_envelope = serde_json::to_string(&hidden_value)?;
    let hidden_envelope_size_bytes = validate_hidden_jacs_envelope_size(&hidden_envelope)?;

    Ok(HtmlInlineJacsEnvelope {
        hidden_envelope,
        compact_header,
        hidden_envelope_size_bytes,
    })
}

pub fn validate_hidden_jacs_envelope_size(
    hidden_jacs_envelope: &str,
) -> crate::error::Result<usize> {
    let size_bytes = hidden_jacs_envelope.len();
    if size_bytes >= HAI_HIDDEN_ENVELOPE_MAX_BYTES {
        return Err(crate::error::HaiError::Validation {
            field: "hidden_jacs_envelope".to_string(),
            message: "hidden_jacs_envelope_too_large".to_string(),
        });
    }
    Ok(size_bytes)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};

    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(feature = "jacs-crate")]
pub fn embed_jacs_header_in_inline_logo(
    compact_jacs_header: &str,
) -> crate::error::Result<SignedInlineLogo> {
    let signed =
        jacs::email::embed_jacs_header_in_logo_png(HAI_JACS_LOGO_BYTES, compact_jacs_header)
            .map_err(|e| {
                crate::error::HaiError::Provider(format!("signed logo embedding failed: {e}"))
            })?;

    Ok(SignedInlineLogo {
        bytes: signed.bytes,
        size_bytes: signed.size_bytes,
    })
}

#[cfg(not(feature = "jacs-crate"))]
pub fn embed_jacs_header_in_inline_logo(
    compact_jacs_header: &str,
) -> crate::error::Result<SignedInlineLogo> {
    // Wasm builds intentionally do not pull `jacs-media` / `image`, but the
    // inline-email transport only needs to add a small uncompressed PNG iTXt
    // metadata chunk. The signed hidden envelope is still produced by the
    // configured JACS provider; this helper only carries its compact header.
    let bytes = embed_png_itxt_jacs_header(HAI_JACS_LOGO_BYTES, compact_jacs_header)?;
    Ok(SignedInlineLogo {
        size_bytes: bytes.len(),
        bytes,
    })
}

#[cfg(not(feature = "jacs-crate"))]
const PNG_SIGNATURE_BYTES: &[u8] = b"\x89PNG\r\n\x1a\n";
#[cfg(not(feature = "jacs-crate"))]
const PNG_IEND_TYPE: &[u8; 4] = b"IEND";
#[cfg(not(feature = "jacs-crate"))]
const PNG_ITXT_TYPE: &[u8; 4] = b"iTXt";
#[cfg(not(feature = "jacs-crate"))]
const PNG_JACS_KEYWORD: &[u8] = b"JACS-Signature";

#[cfg(not(feature = "jacs-crate"))]
fn embed_png_itxt_jacs_header(
    base_logo_png: &[u8],
    compact_jacs_header: &str,
) -> crate::error::Result<Vec<u8>> {
    if !base_logo_png.starts_with(PNG_SIGNATURE_BYTES) {
        return Err(crate::error::HaiError::Provider(
            "bundled inline logo is not a PNG".to_string(),
        ));
    }

    let new_chunk = build_png_chunk(PNG_ITXT_TYPE, &build_png_itxt_body(compact_jacs_header));
    let mut out = Vec::with_capacity(base_logo_png.len() + new_chunk.len());
    out.extend_from_slice(PNG_SIGNATURE_BYTES);

    let mut pos = PNG_SIGNATURE_BYTES.len();
    let mut inserted = false;
    while pos + 12 <= base_logo_png.len() {
        let body_len = u32::from_be_bytes([
            base_logo_png[pos],
            base_logo_png[pos + 1],
            base_logo_png[pos + 2],
            base_logo_png[pos + 3],
        ]) as usize;
        let type_start = pos + 4;
        let type_end = type_start + 4;
        let body_start = type_end;
        let Some(body_end) = body_start.checked_add(body_len) else {
            return Err(crate::error::HaiError::Provider(
                "invalid PNG inline logo chunk length".to_string(),
            ));
        };
        let Some(crc_end) = body_end.checked_add(4) else {
            return Err(crate::error::HaiError::Provider(
                "invalid PNG inline logo CRC length".to_string(),
            ));
        };
        if crc_end > base_logo_png.len() {
            return Err(crate::error::HaiError::Provider(
                "truncated PNG inline logo chunk".to_string(),
            ));
        }

        let chunk_type = &base_logo_png[type_start..type_end];
        let body = &base_logo_png[body_start..body_end];
        let full_chunk = &base_logo_png[pos..crc_end];

        if chunk_type == PNG_ITXT_TYPE && png_itxt_is_jacs_header(body) {
            pos = crc_end;
            continue;
        }

        if chunk_type == PNG_IEND_TYPE {
            out.extend_from_slice(&new_chunk);
            out.extend_from_slice(full_chunk);
            inserted = true;
            break;
        }

        out.extend_from_slice(full_chunk);
        pos = crc_end;
    }

    if !inserted {
        return Err(crate::error::HaiError::Provider(
            "PNG inline logo is missing IEND chunk".to_string(),
        ));
    }

    Ok(out)
}

#[cfg(not(feature = "jacs-crate"))]
fn build_png_itxt_body(payload: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(PNG_JACS_KEYWORD.len() + payload.len() + 6);
    out.extend_from_slice(PNG_JACS_KEYWORD);
    out.push(0);
    out.push(0);
    out.push(0);
    out.extend_from_slice(b"en");
    out.push(0);
    out.push(0);
    out.extend_from_slice(payload.as_bytes());
    out
}

#[cfg(not(feature = "jacs-crate"))]
fn png_itxt_is_jacs_header(body: &[u8]) -> bool {
    let Some(keyword_end) = body.iter().position(|&b| b == 0) else {
        return false;
    };
    if &body[..keyword_end] != PNG_JACS_KEYWORD {
        return false;
    }
    let compression_flag = keyword_end + 1;
    body.get(compression_flag) == Some(&0)
}

#[cfg(not(feature = "jacs-crate"))]
fn build_png_chunk(type_bytes: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(type_bytes);
    out.extend_from_slice(body);
    out.extend_from_slice(&png_crc32(type_bytes, body).to_be_bytes());
    out
}

#[cfg(not(feature = "jacs-crate"))]
fn png_crc32(type_bytes: &[u8], body: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for (n, entry) in table.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb88320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *entry = c;
        }
        table
    });

    let mut crc = 0xffff_ffff;
    for &byte in type_bytes.iter().chain(body.iter()) {
        crc = table[((crc ^ u32::from(byte)) & 0xff) as usize] ^ (crc >> 8);
    }
    crc ^ 0xffff_ffff
}

pub fn render_html_inline_email_body(
    plain_text: &str,
    verify_url: &str,
    hidden_jacs_envelope: &str,
) -> String {
    render_html_inline_email_body_with_logo_link(plain_text, verify_url, hidden_jacs_envelope, true)
}

pub fn render_html_inline_email_body_with_logo_link(
    plain_text: &str,
    verify_url: &str,
    hidden_jacs_envelope: &str,
    wrap_logo_link: bool,
) -> String {
    let escaped_body = escape_html_text(plain_text).replace('\n', "<br>");
    let escaped_verify_url = escape_html_attr(verify_url);
    let logo_html = if wrap_logo_link {
        format!(
            r#"<a {logo_link_marker}="{marker_version}" href="{verify_url}"><img src="cid:{logo_cid}" alt="HAI verification logo"></a>"#,
            logo_link_marker = HAI_LOGO_VERIFY_LINK_MARKER,
            marker_version = HAI_MARKER_VERSION,
            verify_url = escaped_verify_url,
            logo_cid = HAI_JACS_LOGO_CID,
        )
    } else {
        format!(
            r#"<img src="cid:{logo_cid}" alt="HAI verification logo">"#,
            logo_cid = HAI_JACS_LOGO_CID,
        )
    };

    format!(
        concat!(
            r#"<html data-hai-template-version="{template_version}"><body>"#,
            r#"<main data-hai-message-body="v1">{body}</main>"#,
            r#"{logo_html}"#,
            r#"<script type="{script_type}" {envelope_marker}="{marker_version}">{envelope}</script>"#,
            r#"<footer {footer_marker}="{marker_version}">This email is sent from an AI agent. Verify at "#,
            r#"<a {verify_link_marker}="{marker_version}" href="{verify_url}">{verify_url}</a></footer>"#,
            r#"</body></html>"#
        ),
        template_version = HAI_HTML_TEMPLATE_VERSION,
        marker_version = HAI_MARKER_VERSION,
        body = escaped_body,
        logo_html = logo_html,
        script_type = HAI_JACS_ENVELOPE_SCRIPT_TYPE,
        envelope_marker = HAI_JACS_ENVELOPE_MARKER,
        envelope = hidden_jacs_envelope,
        footer_marker = HAI_VERIFY_FOOTER_MARKER,
        verify_link_marker = HAI_VERIFY_LINK_MARKER,
        verify_url = escaped_verify_url,
    )
}

pub fn render_text_inline_email_body(plain_text: &str, verify_url: &str) -> String {
    format!(
        "{}\n\n{}",
        plain_text,
        HAI_VERIFY_FOOTER_TEXT_TEMPLATE.replace("[verify link]", verify_url)
    )
}

fn escape_html_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_html_attr(value: &str) -> String {
    escape_html_text(value)
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_inline_constants_match_prd_contract() {
        assert_eq!(HAI_JACS_ENVELOPE_MARKER, "data-hai-jacs-envelope");
        assert_eq!(HAI_VERIFY_LINK_MARKER, "data-hai-verify-link");
        assert_eq!(HAI_LOGO_VERIFY_LINK_MARKER, "data-hai-logo-verify-link");
        assert_eq!(HAI_VERIFY_FOOTER_MARKER, "data-hai-verify-footer");
        assert_eq!(HAI_JACS_LOGO_CID, "hai-jacs-logo@hai.ai");
        assert_eq!(HAI_JACS_LOGO_CONTENT_ID_HEADER, "<hai-jacs-logo@hai.ai>");
        assert_eq!(HAI_HIDDEN_ENVELOPE_MAX_BYTES, 8192);
        assert_eq!(
            HAI_VERIFY_FOOTER_TEXT_TEMPLATE,
            "This email is sent from an AI agent. Verify at [verify link]"
        );
        assert_eq!(HAI_SIGNED_LOGO_SIZE_BYTE_CAP, None);
    }

    #[test]
    fn bundled_logo_bytes_are_png() {
        assert!(HAI_JACS_LOGO_BYTES.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn bundled_logo_matches_canonical_hash() {
        assert_eq!(sha256_hex(HAI_JACS_LOGO_BYTES), HAI_JACS_LOGO_SHA256_HEX);
    }

    #[test]
    fn renders_deterministic_v1_html_from_plain_text() {
        let html = render_html_inline_email_body(
            "Hello <agent> & team",
            "https://hai.ai/verify/email?id=abc&mode=strict",
            r#"{"compactHeader":"abc"}"#,
        );

        assert_eq!(
            html,
            concat!(
                r#"<html data-hai-template-version="v1"><body>"#,
                r#"<main data-hai-message-body="v1">Hello &lt;agent&gt; &amp; team</main>"#,
                r#"<a data-hai-logo-verify-link="v1" href="https://hai.ai/verify/email?id=abc&amp;mode=strict">"#,
                r#"<img src="cid:hai-jacs-logo@hai.ai" alt="HAI verification logo"></a>"#,
                r#"<script type="application/jacs+json" data-hai-jacs-envelope="v1">{"compactHeader":"abc"}</script>"#,
                r#"<footer data-hai-verify-footer="v1">This email is sent from an AI agent. Verify at "#,
                r#"<a data-hai-verify-link="v1" href="https://hai.ai/verify/email?id=abc&amp;mode=strict">"#,
                r#"https://hai.ai/verify/email?id=abc&amp;mode=strict</a></footer>"#,
                r#"</body></html>"#
            )
        );
    }

    #[test]
    fn renders_exact_footer_in_html_and_text_fallback() {
        let verify_url = "https://hai.ai/verify/email?id=abc";
        let html = render_html_inline_email_body("Hello", verify_url, r#"{"compactHeader":"abc"}"#);
        let text = render_text_inline_email_body("Hello", verify_url);

        assert!(html.contains("This email is sent from an AI agent. Verify at "));
        assert!(html.contains(&format!(
            r#"<a data-hai-verify-link="v1" href="{verify_url}">{verify_url}</a>"#
        )));
        assert_eq!(
            text,
            "Hello\n\nThis email is sent from an AI agent. Verify at https://hai.ai/verify/email?id=abc"
        );
    }

    #[test]
    fn logo_verify_link_wrapper_is_optional_and_matches_footer_url() {
        let verify_url = "https://hai.ai/verify/email?id=abc";
        let wrapped = render_html_inline_email_body_with_logo_link(
            "Hello",
            verify_url,
            r#"{"compactHeader":"abc"}"#,
            true,
        );
        let unwrapped = render_html_inline_email_body_with_logo_link(
            "Hello",
            verify_url,
            r#"{"compactHeader":"abc"}"#,
            false,
        );

        assert!(wrapped.contains(&format!(
            r#"<a data-hai-logo-verify-link="v1" href="{verify_url}"><img src="cid:hai-jacs-logo@hai.ai""#
        )));
        assert!(wrapped.contains(&format!(
            r#"<a data-hai-verify-link="v1" href="{verify_url}">{verify_url}</a>"#
        )));
        assert!(!unwrapped.contains("data-hai-logo-verify-link"));
        assert!(unwrapped.contains(r#"<img src="cid:hai-jacs-logo@hai.ai""#));
    }

    #[cfg(feature = "jacs-crate")]
    #[test]
    fn embeds_compact_jacs_header_in_inlined_logo_bytes() {
        let compact_header = r#"{"compactHeader":"abc"}"#;
        let signed_logo = embed_jacs_header_in_inline_logo(compact_header).unwrap();
        let extracted = jacs::email::extract_jacs_header_from_logo_png(&signed_logo.bytes).unwrap();

        assert_eq!(extracted.as_deref(), Some(compact_header));
        assert_eq!(signed_logo.size_bytes, signed_logo.bytes.len());
    }

    #[test]
    fn validates_hidden_envelope_size_and_reports_signed_logo_size() {
        let max_valid = "x".repeat(HAI_HIDDEN_ENVELOPE_MAX_BYTES - 1);
        assert_eq!(
            validate_hidden_jacs_envelope_size(&max_valid).unwrap(),
            HAI_HIDDEN_ENVELOPE_MAX_BYTES - 1
        );

        let oversized = "x".repeat(HAI_HIDDEN_ENVELOPE_MAX_BYTES);
        let err = validate_hidden_jacs_envelope_size(&oversized).unwrap_err();
        assert!(err.to_string().contains("hidden_jacs_envelope_too_large"));

        let signed_logo = SignedInlineLogo {
            bytes: vec![1, 2, 3],
            size_bytes: 3,
        };
        assert_eq!(signed_logo.size_bytes, signed_logo.bytes.len());
    }
}
