//! RFC 5322 MIME email construction.
//!
//! Builds standards-compliant email messages from [`SendEmailOptions`] that can
//! be parsed by `mail_parser` and signed by `jacs::email::sign_email()`.
//! Mirrors the server-side `build_raw_email` in `hai/api/src/routes/agent_email.rs`.

use base64::Engine;

use crate::email_inline::{
    HAI_JACS_LOGO_CONTENT_ID_HEADER, HAI_JACS_LOGO_CONTENT_TYPE, HAI_JACS_LOGO_FILENAME,
};
use crate::error::{HaiError, Result};
use crate::types::{EmailAttachment, SendEmailOptions};

/// Strip `\r`, `\n`, and `"` from a header value to prevent CRLF and parameter injection.
pub(crate) fn sanitize_header(value: &str) -> String {
    value
        .chars()
        .filter(|c| *c != '\r' && *c != '\n' && *c != '"')
        .collect()
}

/// Build an RFC 5322 email from structured fields.
///
/// Produces raw bytes with CRLF line endings suitable for
/// `jacs::email::sign_email()` and parseable by `mail_parser`.
///
/// # Arguments
/// * `opts` - The email options (to, subject, body, attachments, etc.)
/// * `from_email` - The sender's email address (e.g., "agent@hai.ai")
pub fn build_rfc5322_email(opts: &SendEmailOptions, from_email: &str) -> Result<Vec<u8>> {
    let date = time::OffsetDateTime::now_utc();
    let date_str = date
        .format(&time::format_description::well_known::Rfc2822)
        .map_err(|e| HaiError::Message(format!("failed to format date: {e}")))?;

    let message_id = format!("<{}@hai.ai>", uuid::Uuid::new_v4());

    let safe_to = sanitize_header(&opts.to);
    let safe_from = sanitize_header(from_email);
    let safe_subject = sanitize_header(&opts.subject);

    // Build CC header if any CC recipients
    let cc_header = if !opts.cc.is_empty() {
        let safe_cc: Vec<String> = opts.cc.iter().map(|a| sanitize_header(a)).collect();
        format!("Cc: {}\r\n", safe_cc.join(", "))
    } else {
        String::new()
    };
    // BCC header is included during submission so the API can read envelope
    // recipients from the MIME. JACS does not sign it (not in
    // EmailSignatureHeaders), and the MTA strips it before delivery.
    let bcc_header = if !opts.bcc.is_empty() {
        let safe_bcc: Vec<String> = opts.bcc.iter().map(|a| sanitize_header(a)).collect();
        format!("Bcc: {}\r\n", safe_bcc.join(", "))
    } else {
        String::new()
    };

    if opts.attachments.is_empty() {
        // Simple text/plain email (no MIME multipart needed)
        let mut email = String::new();
        email.push_str(&format!("From: <{}>\r\n", safe_from));
        email.push_str(&format!("To: {}\r\n", safe_to));
        email.push_str(&cc_header);
        email.push_str(&bcc_header);
        email.push_str(&format!("Subject: {}\r\n", safe_subject));
        email.push_str(&format!("Date: {}\r\n", date_str));
        email.push_str(&format!("Message-ID: {}\r\n", message_id));
        if let Some(ref reply_to) = opts.in_reply_to {
            let safe_reply = sanitize_header(reply_to);
            email.push_str(&format!("In-Reply-To: {}\r\n", safe_reply));
            email.push_str(&format!("References: {}\r\n", safe_reply));
        }
        email.push_str("MIME-Version: 1.0\r\n");
        email.push_str("Content-Type: text/plain; charset=utf-8\r\n");
        email.push_str("Content-Transfer-Encoding: 8bit\r\n");
        email.push_str("\r\n"); // end of headers
        email.push_str(&opts.body);
        email.push_str("\r\n");

        Ok(email.into_bytes())
    } else {
        // multipart/mixed with text body + attachments
        let boundary = format!("hai-boundary-{}", uuid::Uuid::new_v4().simple());

        let mut email = String::new();
        email.push_str(&format!("From: <{}>\r\n", safe_from));
        email.push_str(&format!("To: {}\r\n", safe_to));
        email.push_str(&cc_header);
        email.push_str(&bcc_header);
        email.push_str(&format!("Subject: {}\r\n", safe_subject));
        email.push_str(&format!("Date: {}\r\n", date_str));
        email.push_str(&format!("Message-ID: {}\r\n", message_id));
        if let Some(ref reply_to) = opts.in_reply_to {
            let safe_reply = sanitize_header(reply_to);
            email.push_str(&format!("In-Reply-To: {}\r\n", safe_reply));
            email.push_str(&format!("References: {}\r\n", safe_reply));
        }
        email.push_str("MIME-Version: 1.0\r\n");
        email.push_str(&format!(
            "Content-Type: multipart/mixed; boundary=\"{}\"\r\n",
            boundary
        ));
        email.push_str("\r\n"); // end of headers

        // Body part
        email.push_str(&format!("--{}\r\n", boundary));
        email.push_str("Content-Type: text/plain; charset=utf-8\r\n");
        email.push_str("Content-Transfer-Encoding: 8bit\r\n");
        email.push_str("\r\n");
        email.push_str(&opts.body);
        email.push_str("\r\n");

        // Attachment parts
        for att in &opts.attachments {
            push_attachment_part(&mut email, &boundary, att);
        }

        // Closing boundary
        email.push_str(&format!("--{}--\r\n", boundary));

        Ok(email.into_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rfc5322HeaderValues {
    pub date: String,
    pub message_id: String,
}

pub fn generate_rfc5322_header_values() -> Result<Rfc5322HeaderValues> {
    let date = time::OffsetDateTime::now_utc();
    let date_str = date
        .format(&time::format_description::well_known::Rfc2822)
        .map_err(|e| HaiError::Message(format!("failed to format date: {e}")))?;

    Ok(Rfc5322HeaderValues {
        date: date_str,
        message_id: format!("<{}@hai.ai>", uuid::Uuid::new_v4()),
    })
}

pub fn build_html_inline_rfc5322_email(
    opts: &SendEmailOptions,
    from_email: &str,
    html_body: &str,
    signed_logo_png: Option<&[u8]>,
) -> Result<Vec<u8>> {
    let headers = generate_rfc5322_header_values()?;
    build_html_inline_rfc5322_email_with_headers(
        opts,
        from_email,
        html_body,
        signed_logo_png,
        &headers,
    )
}

pub fn build_html_inline_rfc5322_email_with_headers(
    opts: &SendEmailOptions,
    from_email: &str,
    html_body: &str,
    signed_logo_png: Option<&[u8]>,
    headers: &Rfc5322HeaderValues,
) -> Result<Vec<u8>> {
    let mut email = String::new();
    let alternative_boundary = format!("hai-alt-{}", uuid::Uuid::new_v4().simple());

    if opts.attachments.is_empty() {
        push_standard_headers(
            &mut email,
            opts,
            from_email,
            &format!(
                "Content-Type: multipart/alternative; boundary=\"{}\"\r\n",
                alternative_boundary
            ),
            headers,
        )?;
        push_html_inline_alternative_body(
            &mut email,
            opts,
            html_body,
            signed_logo_png,
            &alternative_boundary,
        );
        return Ok(email.into_bytes());
    }

    let mixed_boundary = format!("hai-mixed-{}", uuid::Uuid::new_v4().simple());
    push_standard_headers(
        &mut email,
        opts,
        from_email,
        &format!(
            "Content-Type: multipart/mixed; boundary=\"{}\"\r\n",
            mixed_boundary
        ),
        headers,
    )?;

    email.push_str(&format!("--{}\r\n", mixed_boundary));
    email.push_str(&format!(
        "Content-Type: multipart/alternative; boundary=\"{}\"\r\n",
        alternative_boundary
    ));
    email.push_str("\r\n");
    push_html_inline_alternative_body(
        &mut email,
        opts,
        html_body,
        signed_logo_png,
        &alternative_boundary,
    );

    for att in &opts.attachments {
        push_attachment_part(&mut email, &mixed_boundary, att);
    }

    email.push_str(&format!("--{}--\r\n", mixed_boundary));

    Ok(email.into_bytes())
}

fn push_html_inline_alternative_body(
    email: &mut String,
    opts: &SendEmailOptions,
    html_body: &str,
    signed_logo_png: Option<&[u8]>,
    boundary: &str,
) {
    email.push_str(&format!("--{}\r\n", boundary));
    email.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    email.push_str("Content-Transfer-Encoding: 8bit\r\n");
    email.push_str("\r\n");
    email.push_str(&opts.body);
    email.push_str("\r\n");

    match signed_logo_png {
        Some(logo_png) => {
            let related_boundary = format!("hai-related-{}", uuid::Uuid::new_v4().simple());
            email.push_str(&format!("--{}\r\n", boundary));
            email.push_str(&format!(
                "Content-Type: multipart/related; boundary=\"{}\"; type=\"text/html\"\r\n",
                related_boundary
            ));
            email.push_str("\r\n");

            email.push_str(&format!("--{}\r\n", related_boundary));
            email.push_str("Content-Type: text/html; charset=utf-8\r\n");
            email.push_str("Content-Transfer-Encoding: 8bit\r\n");
            email.push_str("\r\n");
            email.push_str(html_body);
            email.push_str("\r\n");

            email.push_str(&format!("--{}\r\n", related_boundary));
            email.push_str(&format!(
                "Content-Type: {}; name=\"{}\"\r\n",
                HAI_JACS_LOGO_CONTENT_TYPE, HAI_JACS_LOGO_FILENAME
            ));
            email.push_str(&format!(
                "Content-ID: {}\r\n",
                HAI_JACS_LOGO_CONTENT_ID_HEADER
            ));
            email.push_str(&format!(
                "Content-Disposition: inline; filename=\"{}\"\r\n",
                HAI_JACS_LOGO_FILENAME
            ));
            email.push_str("Content-Transfer-Encoding: base64\r\n");
            email.push_str("\r\n");
            push_base64_body(email, logo_png);
            email.push_str(&format!("--{}--\r\n", related_boundary));
        }
        None => {
            email.push_str(&format!("--{}\r\n", boundary));
            email.push_str("Content-Type: text/html; charset=utf-8\r\n");
            email.push_str("Content-Transfer-Encoding: 8bit\r\n");
            email.push_str("\r\n");
            email.push_str(html_body);
            email.push_str("\r\n");
        }
    }

    email.push_str(&format!("--{}--\r\n", boundary));
}

fn push_base64_body(email: &mut String, bytes: &[u8]) {
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    for chunk in b64.as_bytes().chunks(76) {
        email.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        email.push_str("\r\n");
    }
}

fn push_attachment_part(email: &mut String, boundary: &str, att: &EmailAttachment) {
    let raw_data = att.effective_data();
    let safe_filename = sanitize_header(&att.filename);
    let safe_content_type = sanitize_header(&att.content_type);

    email.push_str(&format!("--{}\r\n", boundary));
    email.push_str(&format!(
        "Content-Type: {}; name=\"{}\"\r\n",
        safe_content_type, safe_filename
    ));
    email.push_str(&format!(
        "Content-Disposition: attachment; filename=\"{}\"\r\n",
        safe_filename
    ));
    email.push_str("Content-Transfer-Encoding: base64\r\n");
    email.push_str("\r\n");
    push_base64_body(email, &raw_data);
}

fn push_standard_headers(
    email: &mut String,
    opts: &SendEmailOptions,
    from_email: &str,
    content_type_header: &str,
    headers: &Rfc5322HeaderValues,
) -> Result<()> {
    let safe_to = sanitize_header(&opts.to);
    let safe_from = sanitize_header(from_email);
    let safe_subject = sanitize_header(&opts.subject);

    email.push_str(&format!("From: <{}>\r\n", safe_from));
    email.push_str(&format!("To: {}\r\n", safe_to));
    if !opts.cc.is_empty() {
        let safe_cc: Vec<String> = opts.cc.iter().map(|a| sanitize_header(a)).collect();
        email.push_str(&format!("Cc: {}\r\n", safe_cc.join(", ")));
    }
    if !opts.bcc.is_empty() {
        let safe_bcc: Vec<String> = opts.bcc.iter().map(|a| sanitize_header(a)).collect();
        email.push_str(&format!("Bcc: {}\r\n", safe_bcc.join(", ")));
    }
    email.push_str(&format!("Subject: {}\r\n", safe_subject));
    email.push_str(&format!("Date: {}\r\n", headers.date));
    email.push_str(&format!("Message-ID: {}\r\n", headers.message_id));
    if let Some(ref reply_to) = opts.in_reply_to {
        let safe_reply = sanitize_header(reply_to);
        email.push_str(&format!("In-Reply-To: {}\r\n", safe_reply));
        email.push_str(&format!("References: {}\r\n", safe_reply));
    }
    email.push_str("MIME-Version: 1.0\r\n");
    email.push_str(content_type_header);
    email.push_str("\r\n");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::email_inline::{
        render_html_inline_email_body, HAI_JACS_LOGO_BYTES, HAI_JACS_LOGO_CID,
        HAI_JACS_LOGO_CONTENT_ID_HEADER, HAI_JACS_LOGO_FILENAME,
    };
    use crate::types::{EmailAttachment, SendEmailOptions};
    use mail_parser::{MessageParser, MimeHeaders as _};

    fn simple_opts() -> SendEmailOptions {
        SendEmailOptions {
            to: "recipient@hai.ai".to_string(),
            subject: "Test Subject".to_string(),
            body: "Hello, world!".to_string(),
            cc: vec![],
            bcc: vec![],
            in_reply_to: None,
            attachments: vec![],
            labels: vec![],
            append_footer: None,
            idempotency_key: None,
        }
    }

    #[test]
    fn build_simple_text_email() {
        let raw = build_rfc5322_email(&simple_opts(), "sender@hai.ai").unwrap();
        let text = String::from_utf8_lossy(&raw);

        assert!(text.contains("From: <sender@hai.ai>\r\n"));
        assert!(text.contains("To: recipient@hai.ai\r\n"));
        assert!(text.contains("Subject: Test Subject\r\n"));
        assert!(text.contains("Date: "));
        assert!(text.contains("Message-ID: <"));
        assert!(text.contains("Content-Type: text/plain; charset=utf-8\r\n"));
        assert!(text.contains("Hello, world!"));
    }

    #[test]
    fn build_html_inline_email_uses_multipart_alternative_for_text_and_html() {
        let opts = simple_opts();
        let html = render_html_inline_email_body(
            &opts.body,
            "https://hai.ai/verify/email?id=abc",
            r#"{"compactHeader":"abc"}"#,
        );

        let raw = build_html_inline_rfc5322_email(&opts, "sender@hai.ai", &html, None).unwrap();
        let text = String::from_utf8_lossy(&raw);

        assert!(text.contains("Content-Type: multipart/alternative; boundary="));
        assert!(text.contains("Content-Type: text/plain; charset=utf-8"));
        assert!(text.contains("Content-Type: text/html; charset=utf-8"));

        let message = MessageParser::default().parse(&raw).unwrap();
        let has_plain = message.parts.iter().any(|part| {
            part.content_type()
                .map(|ct| format!("{}/{}", ct.ctype(), ct.subtype().unwrap_or("")))
                .as_deref()
                == Some("text/plain")
        });
        let has_html = message.parts.iter().any(|part| {
            part.content_type()
                .map(|ct| format!("{}/{}", ct.ctype(), ct.subtype().unwrap_or("")))
                .as_deref()
                == Some("text/html")
        });

        assert!(has_plain, "mail parser should see text/plain body");
        assert!(has_html, "mail parser should see text/html body");
        assert!(text.contains(&opts.body));
        assert!(text.contains(r#"data-hai-template-version="v1""#));
    }

    #[test]
    fn build_html_inline_email_uses_related_logo_part() {
        let opts = simple_opts();
        let html = render_html_inline_email_body(
            &opts.body,
            "https://hai.ai/verify/email?id=abc",
            r#"{"compactHeader":"abc"}"#,
        );

        let raw = build_html_inline_rfc5322_email(
            &opts,
            "sender@hai.ai",
            &html,
            Some(HAI_JACS_LOGO_BYTES),
        )
        .unwrap();
        let text = String::from_utf8_lossy(&raw);

        assert!(text.contains("Content-Type: multipart/related; boundary="));
        assert!(text.contains(&format!("Content-ID: {}", HAI_JACS_LOGO_CONTENT_ID_HEADER)));
        assert!(text.contains(&format!(
            "Content-Disposition: inline; filename=\"{}\"",
            HAI_JACS_LOGO_FILENAME
        )));
        assert!(text.contains(&format!(r#"src="cid:{}""#, HAI_JACS_LOGO_CID)));
        assert!(!text.contains(r#"src="https://"#));
        assert!(!text.contains(r#"src="data:"#));

        let message = MessageParser::default().parse(&raw).unwrap();
        let has_logo = message.parts.iter().any(|part| {
            let content_id = part
                .content_id()
                .unwrap_or_default()
                .trim_matches(|c| c == '<' || c == '>');
            let content_type = part
                .content_type()
                .map(|ct| format!("{}/{}", ct.ctype(), ct.subtype().unwrap_or("")))
                .unwrap_or_default();
            content_id == HAI_JACS_LOGO_CID
                && content_type == "image/png"
                && part.content_disposition().map(|d| d.ctype()) == Some("inline")
        });

        assert!(has_logo, "mail parser should see the inline logo part");
    }

    #[test]
    fn build_html_inline_email_wraps_user_attachments_in_mixed() {
        let mut opts = simple_opts();
        opts.attachments = vec![EmailAttachment::new(
            "report.txt".to_string(),
            "text/plain".to_string(),
            b"report".to_vec(),
        )];
        let html = render_html_inline_email_body(
            &opts.body,
            "https://hai.ai/verify/email?id=abc",
            r#"{"compactHeader":"abc"}"#,
        );

        let raw = build_html_inline_rfc5322_email(
            &opts,
            "sender@hai.ai",
            &html,
            Some(HAI_JACS_LOGO_BYTES),
        )
        .unwrap();
        let text = String::from_utf8_lossy(&raw);

        assert!(text.contains("Content-Type: multipart/mixed; boundary="));
        assert!(text.contains("Content-Type: multipart/alternative; boundary="));
        assert!(text.contains("Content-Type: multipart/related; boundary="));
        assert!(!text.contains("jacs-signature.json"));
        assert!(!text.contains("hai.ai.signature.jacs.yaml"));

        let message = MessageParser::default().parse(&raw).unwrap();
        let attachment_names: Vec<_> = message
            .parts
            .iter()
            .filter(|part| part.content_disposition().map(|d| d.ctype()) == Some("attachment"))
            .filter_map(|part| part.attachment_name())
            .collect();

        assert_eq!(attachment_names, vec!["report.txt"]);
    }

    #[test]
    fn build_email_with_attachments() {
        let opts = SendEmailOptions {
            to: "recipient@hai.ai".to_string(),
            subject: "With Attachments".to_string(),
            body: "See attached.".to_string(),
            cc: vec![],
            bcc: vec![],
            in_reply_to: None,
            attachments: vec![
                EmailAttachment::new(
                    "file1.txt".to_string(),
                    "text/plain".to_string(),
                    b"content of file 1".to_vec(),
                ),
                EmailAttachment::new(
                    "file2.pdf".to_string(),
                    "application/pdf".to_string(),
                    b"fake pdf content".to_vec(),
                ),
            ],
            labels: vec![],
            append_footer: None,
            idempotency_key: None,
        };

        let raw = build_rfc5322_email(&opts, "sender@hai.ai").unwrap();
        let text = String::from_utf8_lossy(&raw);

        assert!(text.contains("Content-Type: multipart/mixed; boundary="));
        assert!(text.contains("Content-Disposition: attachment; filename=\"file1.txt\""));
        assert!(text.contains("Content-Disposition: attachment; filename=\"file2.pdf\""));
        assert!(text.contains("Content-Transfer-Encoding: base64"));
        assert!(text.contains("See attached."));
    }

    #[test]
    fn build_reply_email() {
        let opts = SendEmailOptions {
            to: "recipient@hai.ai".to_string(),
            subject: "Re: Original".to_string(),
            body: "Reply body".to_string(),
            cc: vec![],
            bcc: vec![],
            in_reply_to: Some("<original-id@hai.ai>".to_string()),
            attachments: vec![],
            labels: vec![],
            append_footer: None,
            idempotency_key: None,
        };

        let raw = build_rfc5322_email(&opts, "sender@hai.ai").unwrap();
        let text = String::from_utf8_lossy(&raw);

        assert!(text.contains("In-Reply-To: <original-id@hai.ai>\r\n"));
        assert!(text.contains("References: <original-id@hai.ai>\r\n"));
    }

    #[test]
    fn crlf_injection_sanitized() {
        let opts = SendEmailOptions {
            to: "recipient@hai.ai".to_string(),
            subject: "Bad\r\nBcc: attacker@evil.com".to_string(),
            body: "Body".to_string(),
            cc: vec![],
            bcc: vec![],
            in_reply_to: None,
            attachments: vec![],
            labels: vec![],
            append_footer: None,
            idempotency_key: None,
        };

        let raw = build_rfc5322_email(&opts, "sender@hai.ai").unwrap();
        let text = String::from_utf8_lossy(&raw);

        // The \r\n must be stripped, so "Bcc:" does NOT start a new header line.
        // Verify that no line starts with "Bcc:" (the injection attempt).
        for line in text.split("\r\n") {
            assert!(
                !line.starts_with("Bcc:"),
                "CRLF injection succeeded: found header line starting with Bcc:"
            );
        }
        // Subject should be sanitized (CRLF removed, text concatenated)
        assert!(text.contains("Subject: BadBcc: attacker@evil.com\r\n"));
    }

    #[test]
    fn output_is_valid_rfc5322() {
        let raw = build_rfc5322_email(&simple_opts(), "sender@hai.ai").unwrap();
        // Basic structural validation: must contain header/body separator
        let text = String::from_utf8_lossy(&raw);
        assert!(text.contains("\r\n\r\n"), "must have header/body separator");
        // Must have required headers
        assert!(text.contains("From:"));
        assert!(text.contains("To:"));
        assert!(text.contains("Subject:"));
        assert!(text.contains("Date:"));
        assert!(text.contains("Message-ID:"));
        assert!(text.contains("MIME-Version: 1.0"));
    }

    #[test]
    fn filename_quote_injection_sanitized() {
        let opts = SendEmailOptions {
            to: "recipient@hai.ai".to_string(),
            subject: "Test".to_string(),
            body: "Body".to_string(),
            cc: vec![],
            bcc: vec![],
            in_reply_to: None,
            attachments: vec![EmailAttachment::new(
                "file\"; name=\"evil".to_string(),
                "text/plain".to_string(),
                b"content".to_vec(),
            )],
            labels: vec![],
            append_footer: None,
            idempotency_key: None,
        };

        let raw = build_rfc5322_email(&opts, "sender@hai.ai").unwrap();
        let text = String::from_utf8_lossy(&raw);

        // The quote must be stripped so it can't break out of the filename parameter
        assert!(
            !text.contains("filename=\"file\""),
            "Quote injection: filename quote broke out of parameter"
        );
        for line in text.split("\r\n") {
            assert!(
                !line.contains("name=\"evil\""),
                "Parameter injection succeeded: found injected name parameter"
            );
        }
    }

    #[test]
    fn output_has_crlf_line_endings() {
        let raw = build_rfc5322_email(&simple_opts(), "sender@hai.ai").unwrap();
        let text = String::from_utf8(raw).unwrap();
        // Every line must end with \r\n (no bare \n)
        for line in text.split("\r\n") {
            assert!(!line.contains('\n'), "found bare \\n in line: {:?}", line);
        }
    }
}
