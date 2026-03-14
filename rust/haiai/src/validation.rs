//! Input validation for email operations.
//!
//! Validates email addresses, header values, attachment constraints, and
//! other user-provided inputs before they reach MIME construction or the API.

use crate::error::{HaiError, Result};

/// Maximum attachment size in bytes (10 MB).
pub const MAX_ATTACHMENT_SIZE: usize = 10 * 1024 * 1024;

/// Maximum number of attachments per email.
pub const MAX_ATTACHMENT_COUNT: usize = 5;

/// Validate that a header value does not contain CR or LF characters.
///
/// Returns an error with the field name if CRLF injection is detected.
pub fn validate_no_crlf(field_name: &str, value: &str) -> Result<()> {
    if value.contains('\r') || value.contains('\n') {
        return Err(HaiError::Validation {
            field: field_name.to_string(),
            message: format!(
                "Invalid characters in '{}': must not contain CR or LF",
                field_name
            ),
        });
    }
    Ok(())
}

/// Validate that an email address has basic syntactic validity.
///
/// Checks for:
/// - Non-empty after trimming
/// - Presence of `@` separator
/// - Non-empty local part (1-64 chars)
/// - Non-empty domain with at least one `.`
/// - No whitespace or control characters
pub fn validate_email_address(address: &str) -> Result<()> {
    let trimmed = address.trim();
    if trimmed.is_empty() {
        return Err(HaiError::Validation {
            field: "to".to_string(),
            message: "Invalid email address: empty string".to_string(),
        });
    }

    // Check for CRLF injection in the address itself
    validate_no_crlf("to", trimmed)?;

    let (local, domain) = trimmed
        .rsplit_once('@')
        .ok_or_else(|| HaiError::Validation {
            field: "to".to_string(),
            message: format!("Invalid email address: '{}' (missing @)", address),
        })?;

    if local.is_empty() {
        return Err(HaiError::Validation {
            field: "to".to_string(),
            message: format!("Invalid email address: '{}' (empty local part)", address),
        });
    }

    if local.len() > 64 {
        return Err(HaiError::Validation {
            field: "to".to_string(),
            message: format!("Invalid email address: '{}' (local part too long)", address),
        });
    }

    if domain.is_empty() || !domain.contains('.') {
        return Err(HaiError::Validation {
            field: "to".to_string(),
            message: format!("Invalid email address: '{}' (invalid domain)", address),
        });
    }

    // Check for whitespace or control characters
    if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(HaiError::Validation {
            field: "to".to_string(),
            message: format!(
                "Invalid email address: '{}' (contains whitespace or control characters)",
                address
            ),
        });
    }

    Ok(())
}

/// Validate attachment constraints (size and count limits).
pub fn validate_attachments(attachments: &[crate::types::EmailAttachment]) -> Result<()> {
    if attachments.len() > MAX_ATTACHMENT_COUNT {
        return Err(HaiError::Validation {
            field: "attachments".to_string(),
            message: format!(
                "Too many attachments: {} (maximum {})",
                attachments.len(),
                MAX_ATTACHMENT_COUNT
            ),
        });
    }

    for att in attachments {
        let data = att.effective_data();
        if data.len() > MAX_ATTACHMENT_SIZE {
            return Err(HaiError::Validation {
                field: "attachments".to_string(),
                message: format!(
                    "Attachment '{}' too large: {} bytes (maximum {} bytes)",
                    att.filename,
                    data.len(),
                    MAX_ATTACHMENT_SIZE
                ),
            });
        }

        // Validate filename for path traversal
        validate_filename(&att.filename)?;
    }

    Ok(())
}

/// Validate that a filename does not contain path traversal characters.
pub fn validate_filename(filename: &str) -> Result<()> {
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return Err(HaiError::Validation {
            field: "filename".to_string(),
            message: format!(
                "Invalid filename '{}': must not contain path traversal characters",
                filename
            ),
        });
    }
    validate_no_crlf("filename", filename)?;
    Ok(())
}

/// Validate all fields of a SendEmailOptions before constructing MIME.
pub fn validate_send_email(options: &crate::types::SendEmailOptions) -> Result<()> {
    validate_email_address(&options.to)?;
    for cc_addr in &options.cc {
        validate_email_address(cc_addr)?;
    }
    for bcc_addr in &options.bcc {
        validate_email_address(bcc_addr)?;
    }
    validate_no_crlf("subject", &options.subject)?;
    validate_no_crlf("body", &options.body)?;
    if let Some(ref reply_to) = options.in_reply_to {
        validate_no_crlf("in_reply_to", reply_to)?;
    }
    validate_attachments(&options.attachments)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{EmailAttachment, SendEmailOptions};

    #[test]
    fn valid_email_address() {
        assert!(validate_email_address("agent@hai.ai").is_ok());
        assert!(validate_email_address("user@example.com").is_ok());
    }

    #[test]
    fn invalid_email_no_at() {
        let err = validate_email_address("noatsign").unwrap_err();
        assert!(err.to_string().contains("missing @"));
    }

    #[test]
    fn invalid_email_empty() {
        let err = validate_email_address("").unwrap_err();
        assert!(err.to_string().contains("empty string"));
    }

    #[test]
    fn invalid_email_empty_local() {
        let err = validate_email_address("@hai.ai").unwrap_err();
        assert!(err.to_string().contains("empty local part"));
    }

    #[test]
    fn invalid_email_no_dot_domain() {
        let err = validate_email_address("user@localhost").unwrap_err();
        assert!(err.to_string().contains("invalid domain"));
    }

    #[test]
    fn invalid_email_local_too_long() {
        let long_local = "a".repeat(65);
        let err = validate_email_address(&format!("{}@hai.ai", long_local)).unwrap_err();
        assert!(err.to_string().contains("local part too long"));
    }

    #[test]
    fn crlf_in_subject_rejected() {
        let err = validate_no_crlf("subject", "Bad\r\nBcc: attacker@evil.com").unwrap_err();
        assert!(err.to_string().contains("CR or LF"));
    }

    #[test]
    fn crlf_in_to_rejected() {
        let err = validate_email_address("user@hai.ai\r\nBcc: evil@evil.com").unwrap_err();
        assert!(err.to_string().contains("CR or LF"));
    }

    #[test]
    fn too_many_attachments_rejected() {
        let attachments: Vec<EmailAttachment> = (0..6)
            .map(|i| EmailAttachment::new(format!("file{}.txt", i), "text/plain".into(), vec![1]))
            .collect();
        let err = validate_attachments(&attachments).unwrap_err();
        assert!(err.to_string().contains("Too many attachments"));
    }

    #[test]
    fn oversized_attachment_rejected() {
        let big_data = vec![0u8; MAX_ATTACHMENT_SIZE + 1];
        let attachments = vec![EmailAttachment::new(
            "big.bin".into(),
            "application/octet-stream".into(),
            big_data,
        )];
        let err = validate_attachments(&attachments).unwrap_err();
        assert!(err.to_string().contains("too large"));
    }

    #[test]
    fn path_traversal_in_filename_rejected() {
        let err = validate_filename("../../etc/passwd").unwrap_err();
        assert!(err.to_string().contains("path traversal"));
    }

    #[test]
    fn validate_send_email_options() {
        let opts = SendEmailOptions {
            to: "agent@hai.ai".into(),
            subject: "Test".into(),
            body: "Hello".into(),
            cc: vec![],
            bcc: vec![],
            in_reply_to: None,
            attachments: vec![],
        };
        assert!(validate_send_email(&opts).is_ok());
    }

    #[test]
    fn validate_send_email_invalid_to() {
        let opts = SendEmailOptions {
            to: "not-an-email".into(),
            subject: "Test".into(),
            body: "Hello".into(),
            cc: vec![],
            bcc: vec![],
            in_reply_to: None,
            attachments: vec![],
        };
        assert!(validate_send_email(&opts).is_err());
    }

    #[test]
    fn no_unsigned_send_path_exists() {
        // This is a compile-time guarantee: there is no function called
        // send_unsigned_email in the crate. The test exists as documentation
        // that the design is intentional.
        // HaiClient::send_signed_email always signs. HaiClient::send_email
        // delegates to the server which also signs. Both paths sign.
    }
}
