use haiai::{
    generate_verify_link, generate_verify_link_hosted, HaiError, MAX_VERIFY_DOCUMENT_BYTES,
    MAX_VERIFY_URL_LEN,
};

#[test]
fn basic_url_generation() {
    let doc = r#"{"jacsId":"abc123","data":"hello"}"#;
    let url = generate_verify_link(doc, None).expect("link");
    assert!(url.starts_with("https://hai.ai/jacs/verify?s="));
}

#[test]
fn uses_urlsafe_base64_without_padding() {
    let doc = r#"{"key":">>>>"}"#;
    let url = generate_verify_link(doc, None).expect("link");
    let query = url.split("?s=").nth(1).expect("query");
    assert!(!query.contains('+'));
    assert!(!query.contains('/'));
    assert!(!query.contains('='));
}

#[test]
fn enforces_length_limit() {
    let doc = "x".repeat(MAX_VERIFY_DOCUMENT_BYTES + 100);
    let err = generate_verify_link(&doc, None).expect_err("should fail");
    match err {
        HaiError::VerifyUrlTooLong { max_len } => assert_eq!(max_len, MAX_VERIFY_URL_LEN),
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn hosted_mode_uses_document_id() {
    let doc = r#"{"document_id":"doc-1"}"#;
    let url = generate_verify_link_hosted(doc, Some("https://example.com/")).expect("hosted");
    assert_eq!(url, "https://example.com/verify/doc-1");
}

#[test]
fn hosted_mode_requires_document_id() {
    let err = generate_verify_link_hosted(r#"{"data":"no-id"}"#, None).expect_err("missing id");
    match err {
        HaiError::MissingHostedDocumentId => {}
        other => panic!("unexpected error: {other}"),
    }
}
