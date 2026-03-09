//! RFC 5322 MIME email construction.
//!
//! Builds standards-compliant email messages from [`SendEmailOptions`] that can
//! be parsed by `mail_parser` and signed by `jacs::email::sign_email()`.
//! Mirrors the server-side `build_raw_email` in `hai/api/src/routes/agent_email.rs`.

use base64::Engine;

use crate::error::{HaiError, Result};
use crate::types::SendEmailOptions;

/// Strip `\r`, `\n`, and `"` from a header value to prevent CRLF and parameter injection.
fn sanitize_header(value: &str) -> String {
    value.chars().filter(|c| *c != '\r' && *c != '\n' && *c != '"').collect()
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

    if opts.attachments.is_empty() {
        // Simple text/plain email (no MIME multipart needed)
        let mut email = String::new();
        email.push_str(&format!("From: <{}>\r\n", safe_from));
        email.push_str(&format!("To: {}\r\n", safe_to));
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
            let raw_data = att.effective_data();
            let b64 =
                base64::engine::general_purpose::STANDARD.encode(&raw_data);
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
            // Write base64 data in 76-char lines (RFC 2045)
            for chunk in b64.as_bytes().chunks(76) {
                email.push_str(std::str::from_utf8(chunk).unwrap_or(""));
                email.push_str("\r\n");
            }
        }

        // Closing boundary
        email.push_str(&format!("--{}--\r\n", boundary));

        Ok(email.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{EmailAttachment, SendEmailOptions};

    fn simple_opts() -> SendEmailOptions {
        SendEmailOptions {
            to: "recipient@hai.ai".to_string(),
            subject: "Test Subject".to_string(),
            body: "Hello, world!".to_string(),
            in_reply_to: None,
            attachments: vec![],
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
    fn build_email_with_attachments() {
        let opts = SendEmailOptions {
            to: "recipient@hai.ai".to_string(),
            subject: "With Attachments".to_string(),
            body: "See attached.".to_string(),
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
            in_reply_to: Some("<original-id@hai.ai>".to_string()),
            attachments: vec![],
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
            in_reply_to: None,
            attachments: vec![],
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
            in_reply_to: None,
            attachments: vec![EmailAttachment::new(
                "file\"; name=\"evil".to_string(),
                "text/plain".to_string(),
                b"content".to_vec(),
            )],
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
            assert!(
                !line.contains('\n'),
                "found bare \\n in line: {:?}",
                line
            );
        }
    }
}
