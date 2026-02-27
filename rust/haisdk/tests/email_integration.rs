//! Live integration tests for HAI email CRUD operations.
//!
//! Gated behind `HAI_LIVE_TEST=1`. Requires a running HAI API at
//! `HAI_URL` (defaults to `http://localhost:3000`) backed by Stalwart.
//!
//! Run:
//! ```bash
//! HAI_LIVE_TEST=1 cargo test -p haisdk email_integration -- --nocapture
//! ```

#![cfg(any(feature = "jacs-crate", feature = "jacs-local"))]

use haisdk::{
    CreateAgentOptions, HaiClient, HaiClientOptions, ListMessagesOptions, LocalJacsProvider,
    RegisterAgentOptions, SearchOptions, SendEmailOptions,
};
use std::env;

fn api_url() -> String {
    env::var("HAI_URL").unwrap_or_else(|_| "http://localhost:3000".to_string())
}

fn is_live() -> bool {
    env::var("HAI_LIVE_TEST").unwrap_or_default() == "1"
}

/// Full email lifecycle test: send → list → get → mark read → mark unread →
/// search → unread count → email status → reply → delete → verify deleted.
///
/// Uses a single sequential test because each step depends on the previous.
#[tokio::test]
async fn email_integration_lifecycle() {
    if !is_live() {
        eprintln!("skipping email_integration_lifecycle: set HAI_LIVE_TEST=1 to run");
        return;
    }

    let base_url = api_url();
    let agent_name = format!("rust-integ-{}", uuid_v4_short());

    // ── Setup: create a JACS agent and register with the API ──────────────
    let tmp = tempfile::tempdir().expect("create temp dir");
    let key_dir = tmp.path().join("keys");
    let data_dir = tmp.path().join("data");
    std::fs::create_dir_all(&key_dir).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();

    let config_path = tmp.path().join("jacs.config.json");

    let create_result = LocalJacsProvider::create_agent_with_options(&CreateAgentOptions {
        name: agent_name.clone(),
        password: "test-password-1234".to_string(),
        algorithm: Some("ed25519".to_string()),
        data_directory: Some(data_dir.display().to_string()),
        key_directory: Some(key_dir.display().to_string()),
        config_path: Some(config_path.display().to_string()),
        agent_type: None,
        description: Some("Rust integration test agent".to_string()),
        domain: None,
        default_storage: None,
    })
    .expect("create_agent_with_options");

    eprintln!("Created agent: id={}", create_result.agent_id);

    // Load the provider from the config that create_agent_with_options wrote.
    let provider =
        LocalJacsProvider::from_config_path(Some(config_path.as_path())).expect("load provider");
    let agent_json = provider.export_agent_json().expect("export agent json");
    let public_key_pem = provider.public_key_pem().expect("public key pem");

    let mut client = HaiClient::new(
        provider,
        HaiClientOptions {
            base_url: base_url.clone(),
            ..HaiClientOptions::default()
        },
    )
    .expect("build HaiClient");

    let reg = client
        .register(&RegisterAgentOptions {
            agent_json,
            public_key_pem: Some(public_key_pem),
            owner_email: Some(env::var("HAI_OWNER_EMAIL").unwrap_or_else(|_| "jonathan@hai.io".to_string())),
            domain: None,
            description: Some("Rust integration test agent".to_string()),
        })
        .await
        .expect("register agent");

    eprintln!("Registered agent: jacs_id={}, agent_id={}", reg.jacs_id, reg.agent_id);
    assert!(reg.success || !reg.jacs_id.is_empty(), "registration should succeed");

    // Store the HAI-assigned agent UUID for email URL paths.
    if !reg.agent_id.is_empty() {
        client.set_hai_agent_id(reg.agent_id.clone());
    }

    // ── 0. Claim username (provisions email address) ────────────────────
    let claim = client
        .claim_username(&reg.agent_id, &agent_name)
        .await
        .expect("claim_username");
    eprintln!("Claimed username: {}, email={}", claim.username, claim.email);
    assert!(!claim.email.is_empty(), "claim should return email");

    // ── 1. Send email ────────────────────────────────────────────────────
    let subject = format!("rust-integ-test-{}", uuid_v4_short());
    let body = "Hello from Rust integration test!";

    let send_result = client
        .send_email(&SendEmailOptions {
            to: format!("{}@hai.ai", agent_name),
            subject: subject.clone(),
            body: body.to_string(),
            in_reply_to: None,
            attachments: Vec::new(),
        })
        .await
        .expect("send_email");

    let message_id = &send_result.message_id;
    eprintln!("Sent email: message_id={}", message_id);
    assert!(!message_id.is_empty(), "message_id should not be empty");

    // Small delay for async delivery
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // ── 2. List messages ─────────────────────────────────────────────────
    let messages = client
        .list_messages(&ListMessagesOptions {
            limit: Some(10),
            offset: None,
            direction: None,
        })
        .await
        .expect("list_messages");

    eprintln!("Listed {} messages", messages.len());
    assert!(!messages.is_empty(), "should have at least one message");

    // ── 3. Get message ───────────────────────────────────────────────────
    let msg = client
        .get_message(message_id)
        .await
        .expect("get_message");

    assert_eq!(msg.subject, subject);
    assert!(
        msg.body_text.contains(body),
        "body should contain our text"
    );
    eprintln!("Got message: subject={}", msg.subject);

    // ── 4. Mark read ─────────────────────────────────────────────────────
    client.mark_read(message_id).await.expect("mark_read");
    eprintln!("Marked read");

    // ── 5. Mark unread ───────────────────────────────────────────────────
    client.mark_unread(message_id).await.expect("mark_unread");
    eprintln!("Marked unread");

    // ── 6. Search messages ───────────────────────────────────────────────
    let search_results = client
        .search_messages(&SearchOptions {
            q: Some(subject.clone()),
            direction: None,
            from_address: None,
            to_address: None,
            since: None,
            until: None,
            limit: None,
            offset: None,
        })
        .await
        .expect("search_messages");

    eprintln!("Search found {} results", search_results.len());
    assert!(
        !search_results.is_empty(),
        "search should find the sent message"
    );

    // ── 7. Unread count ──────────────────────────────────────────────────
    let unread = client.get_unread_count().await.expect("get_unread_count");
    eprintln!("Unread count: {}", unread);
    // Just assert it's a valid number (>= 0 always true for u64)

    // ── 8. Email status ──────────────────────────────────────────────────
    let status = client.get_email_status().await.expect("get_email_status");
    eprintln!("Email status: email={}, tier={}", status.email, status.tier);
    assert!(!status.email.is_empty(), "status should include email");

    // ── 9. Reply ─────────────────────────────────────────────────────────
    let rfc_message_id = msg.message_id.as_deref().unwrap_or(message_id);
    let reply_result = client
        .reply(rfc_message_id, "Reply from Rust integration test!", None)
        .await
        .expect("reply");

    eprintln!("Reply sent: message_id={}", reply_result.message_id);
    assert!(
        !reply_result.message_id.is_empty(),
        "reply message_id should not be empty"
    );

    // ── 10. Delete ───────────────────────────────────────────────────────
    client
        .delete_message(message_id)
        .await
        .expect("delete_message");
    eprintln!("Deleted message: {}", message_id);

    // ── 11. Verify deleted ───────────────────────────────────────────────
    let get_deleted = client.get_message(message_id).await;
    assert!(
        get_deleted.is_err(),
        "get_message on deleted message should error"
    );
    eprintln!("Verified deleted message returns error");

    eprintln!("All email integration tests passed!");
}

/// Generate a short pseudo-UUID for unique agent/subject names.
fn uuid_v4_short() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    format!("{:x}", ts)
}
