#![cfg(feature = "jacs-crate")]

use std::fs;
use std::sync::Mutex;

use haiai::{
    CreateAgentOptions, EmailGenerationType, HaiClient, HaiClientOptions, JacsMediaProvider,
    JacsProvider, LocalJacsProvider, SendEmailOptions,
};
use serde_json::Value;
use uuid::Uuid;

static HTML_INLINE_DYN_PROVIDER_LOCK: Mutex<()> = Mutex::new(());

struct TestAgent {
    base: std::path::PathBuf,
    config: std::path::PathBuf,
}

impl TestAgent {
    fn new() -> Self {
        let base = std::env::current_dir()
            .expect("current dir")
            .join(format!("target/html-inline-dyn-{}", Uuid::new_v4()));
        fs::create_dir_all(&base).expect("create test base");
        let config = base.join("jacs.config.json");
        let options = CreateAgentOptions {
            name: "html-inline-dyn-provider-test".to_string(),
            password: "HtmlInlineDynProviderTest!2026".to_string(),
            algorithm: Some("ring-Ed25519".to_string()),
            data_directory: Some(base.join("data").display().to_string()),
            key_directory: Some(base.join("keys").display().to_string()),
            config_path: Some(config.display().to_string()),
            agent_type: Some("ai".to_string()),
            description: Some("HTML inline dyn provider regression test".to_string()),
            domain: None,
            default_storage: Some("fs".to_string()),
        };
        LocalJacsProvider::create_agent_with_options(&options).expect("create agent");

        Self { base, config }
    }

    fn provider(&self) -> LocalJacsProvider {
        unsafe {
            std::env::set_var(
                "JACS_PRIVATE_KEY_PASSWORD",
                "HtmlInlineDynProviderTest!2026",
            );
        }
        LocalJacsProvider::from_config_path(Some(&self.config), None).expect("load provider")
    }
}

impl Drop for TestAgent {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[test]
fn html_inline_envelope_signs_through_boxed_media_provider() {
    let _lock = HTML_INLINE_DYN_PROVIDER_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let agent = TestAgent::new();
    let provider: Box<dyn JacsMediaProvider> = Box::new(agent.provider());
    let raw_email = b"Date: Tue, 01 Apr 2025 00:00:00 +0000\r\n\
From: alice@example.com\r\n\
To: bob@example.com\r\n\
Subject: hello\r\n\
Message-ID: <html-inline-dyn-provider@example.com>\r\n\
\r\n\
hi\r\n";

    let envelope = provider
        .sign_html_inline_email_envelope(raw_email)
        .expect("html inline envelope must sign through Box<dyn JacsMediaProvider>");

    assert!(envelope.compact_header.starts_with("sha256:"));
    assert!(envelope.hidden_envelope_size_bytes > 0);

    let hidden: Value =
        serde_json::from_str(&envelope.hidden_envelope).expect("hidden envelope is json");
    assert_eq!(
        hidden
            .pointer("/jacsEnvelope/jacsType")
            .and_then(Value::as_str),
        Some("email_inline_signature")
    );

    let mut client = HaiClient::new(
        provider,
        HaiClientOptions {
            base_url: "https://api.example.test".to_string(),
            ..HaiClientOptions::default()
        },
    )
    .expect("client");
    client.set_agent_email("agent@hai.ai".to_string());

    let signed_email = client
        .create_signed_email(
            &SendEmailOptions {
                to: "recipient@example.com".to_string(),
                subject: "html inline dyn provider".to_string(),
                body: "body".to_string(),
                cc: Vec::new(),
                bcc: Vec::new(),
                in_reply_to: None,
                attachments: Vec::new(),
                labels: Vec::new(),
                append_footer: None,
                idempotency_key: None,
            },
            EmailGenerationType::HtmlInlineJacs,
        )
        .expect("html inline email must sign through HaiClient<Box<dyn JacsMediaProvider>>");

    assert_eq!(
        signed_email.generation_type,
        EmailGenerationType::HtmlInlineJacs
    );
    assert!(signed_email.hidden_envelope_size_bytes.unwrap_or(0) > 0);
    assert!(signed_email.signed_logo_size_bytes.unwrap_or(0) > 0);

    let raw = String::from_utf8_lossy(signed_email.as_bytes());
    assert!(raw.contains("data-hai-jacs-envelope"));
    assert!(raw.contains("cid:hai-jacs-logo@hai.ai"));
}
