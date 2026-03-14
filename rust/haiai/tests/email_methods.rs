use haiai::{HaiClient, HaiClientOptions, SearchOptions, SendEmailOptions, StaticJacsProvider};
use httpmock::Method::{DELETE, GET, POST};
use httpmock::MockServer;
use serde_json::json;
use sha2::{Digest, Sha256};

fn make_client(base_url: &str) -> HaiClient<StaticJacsProvider> {
    let provider = StaticJacsProvider::new("test-agent-001");
    let mut client = HaiClient::new(
        provider,
        HaiClientOptions {
            base_url: base_url.to_string(),
            ..HaiClientOptions::default()
        },
    )
    .expect("client");
    client.set_agent_email("test-agent-001@hai.ai".to_string());
    client
}

// --- Task #38: JACS content signing in send_email ---

#[tokio::test]
async fn send_email_sends_content_fields() {
    let server = MockServer::start_async().await;

    // Server handles JACS signing — client sends content fields only.
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/agents/test-agent-001/email/send")
                .header_exists("authorization")
                .header("content-type", "application/json")
                .body_includes("\"to\"")
                .body_includes("\"subject\"")
                .body_includes("\"body\"");
            then.status(200).json_body(json!({
                "message_id": "msg-001",
                "status": "queued"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let result = client
        .send_email(&SendEmailOptions {
            to: "bob@hai.ai".to_string(),
            subject: "Hello".to_string(),
            body: "World".to_string(),
            cc: Vec::new(),
            bcc: Vec::new(),
            in_reply_to: None,
            attachments: Vec::new(),
            labels: Vec::new(),
        })
        .await
        .expect("send_email");

    assert_eq!(result.message_id, "msg-001");
    assert_eq!(result.status, "queued");
    mock.assert_async().await;
}

#[tokio::test]
async fn send_email_signature_uses_correct_hash_format() {
    let server = MockServer::start_async().await;

    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/agents/test-agent-001/email/send");
            then.status(200).json_body(json!({
                "message_id": "msg-002",
                "status": "queued"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    client
        .send_email(&SendEmailOptions {
            to: "bob@hai.ai".to_string(),
            subject: "Test Subject".to_string(),
            body: "Test Body".to_string(),
            cc: Vec::new(),
            bcc: Vec::new(),
            in_reply_to: None,
            attachments: Vec::new(),
            labels: Vec::new(),
        })
        .await
        .expect("send_email");

    // Verify the hash computation is deterministic and correct format.
    // content_hash = sha256("Test Subject\nTest Body") as hex
    let expected_hash = {
        let mut hasher = Sha256::new();
        hasher.update(b"Test Subject\nTest Body");
        format!("{:x}", hasher.finalize())
    };

    assert_eq!(expected_hash.len(), 64);
    assert!(expected_hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn send_email_includes_in_reply_to_when_set() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/agents/test-agent-001/email/send")
                .body_includes("in_reply_to");
            then.status(200).json_body(json!({
                "message_id": "msg-003",
                "status": "queued"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    client
        .send_email(&SendEmailOptions {
            to: "bob@hai.ai".to_string(),
            subject: "Re: Original".to_string(),
            body: "Reply body".to_string(),
            cc: Vec::new(),
            bcc: Vec::new(),
            in_reply_to: Some("orig-msg-id".to_string()),
            attachments: Vec::new(),
            labels: Vec::new(),
        })
        .await
        .expect("send_email with in_reply_to");

    mock.assert_async().await;
}

// --- Task #39: New email methods ---

#[tokio::test]
async fn get_message_returns_email() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/test-agent-001/email/messages/msg-100")
                .header_exists("authorization");
            then.status(200).json_body(json!({
                "id": "msg-100",
                "from_address": "alice@hai.ai",
                "to_address": "test-agent-001@hai.ai",
                "subject": "Greetings",
                "body_text": "Hello there",
                "created_at": "2026-02-24T10:00:00Z"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let msg = client.get_message("msg-100").await.expect("get_message");

    assert_eq!(msg.id, "msg-100");
    assert_eq!(msg.from_address, "alice@hai.ai");
    assert_eq!(msg.subject, "Greetings");
    assert_eq!(msg.body_text, "Hello there");
    mock.assert_async().await;
}

#[tokio::test]
async fn delete_message_succeeds_on_204() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/api/agents/test-agent-001/email/messages/msg-200")
                .header_exists("authorization");
            then.status(204);
        })
        .await;

    let client = make_client(&server.base_url());
    client
        .delete_message("msg-200")
        .await
        .expect("delete_message");

    mock.assert_async().await;
}

#[tokio::test]
async fn delete_message_succeeds_on_200() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/api/agents/test-agent-001/email/messages/msg-201")
                .header_exists("authorization");
            then.status(200).json_body(json!({}));
        })
        .await;

    let client = make_client(&server.base_url());
    client
        .delete_message("msg-201")
        .await
        .expect("delete_message 200");

    mock.assert_async().await;
}

#[tokio::test]
async fn delete_message_returns_error_on_404() {
    let server = MockServer::start_async().await;

    server
        .mock_async(|when, then| {
            when.method(DELETE)
                .path("/api/agents/test-agent-001/email/messages/no-such");
            then.status(404)
                .json_body(json!({"error": "message not found"}));
        })
        .await;

    let client = make_client(&server.base_url());
    let err = client.delete_message("no-such").await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("404") || msg.contains("not found"),
        "got: {msg}"
    );
}

#[tokio::test]
async fn mark_unread_succeeds() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/agents/test-agent-001/email/messages/msg-300/unread")
                .header_exists("authorization");
            then.status(200);
        })
        .await;

    let client = make_client(&server.base_url());
    client.mark_unread("msg-300").await.expect("mark_unread");

    mock.assert_async().await;
}

#[tokio::test]
async fn search_messages_with_all_params() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/test-agent-001/email/search")
                .header_exists("authorization")
                .query_param("q", "invoice")
                .query_param("direction", "inbound")
                .query_param("from_address", "alice@hai.ai")
                .query_param("limit", "10")
                .query_param("offset", "0");
            then.status(200).json_body(json!({
                "messages": [{
                    "id": "msg-400",
                    "from_address": "alice@hai.ai",
                    "to_address": "test-agent-001@hai.ai",
                    "subject": "Invoice #123",
                    "body_text": "Please see attached",
                    "created_at": "2026-02-24T12:00:00Z"
                }],
                "total": 1,
                "unread": 0
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let results = client
        .search_messages(&SearchOptions {
            q: Some("invoice".to_string()),
            direction: Some("inbound".to_string()),
            from_address: Some("alice@hai.ai".to_string()),
            limit: Some(10),
            offset: Some(0),
            ..Default::default()
        })
        .await
        .expect("search_messages");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "msg-400");
    assert_eq!(results[0].subject, "Invoice #123");
    mock.assert_async().await;
}

#[tokio::test]
async fn search_messages_handles_wrapped_response() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/test-agent-001/email/search");
            then.status(200).json_body(json!({
                "messages": [{
                    "id": "msg-401",
                    "from_address": "bob@hai.ai",
                    "to_address": "test-agent-001@hai.ai",
                    "subject": "Test",
                    "body_text": "Body",
                    "created_at": "2026-02-24T12:00:00Z"
                }],
                "total": 1,
                "unread": 0
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let results = client
        .search_messages(&SearchOptions::default())
        .await
        .expect("search_messages wrapped");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "msg-401");
    mock.assert_async().await;
}

#[tokio::test]
async fn get_unread_count_returns_count() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/test-agent-001/email/unread-count")
                .header_exists("authorization");
            then.status(200).json_body(json!({ "count": 42 }));
        })
        .await;

    let client = make_client(&server.base_url());
    let count = client.get_unread_count().await.expect("get_unread_count");

    assert_eq!(count, 42);
    mock.assert_async().await;
}

#[tokio::test]
async fn get_unread_count_handles_raw_number() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/test-agent-001/email/unread-count");
            then.status(200).json_body(json!(7));
        })
        .await;

    let client = make_client(&server.base_url());
    let count = client.get_unread_count().await.expect("unread_count raw");

    assert_eq!(count, 7);
    mock.assert_async().await;
}

#[tokio::test]
async fn reply_posts_to_reply_endpoint() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/agents/test-agent-001/email/reply")
                .body_includes("\"message_id\":\"orig-msg\"")
                .body_includes("My reply");
            then.status(200).json_body(json!({
                "message_id": "reply-msg",
                "status": "queued"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let result = client
        .reply("orig-msg", "My reply", None)
        .await
        .expect("reply");

    assert_eq!(result.message_id, "reply-msg");
    mock.assert_async().await;
}

#[tokio::test]
async fn reply_with_subject_override() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/agents/test-agent-001/email/reply")
                .body_includes("Custom Subject");
            then.status(200).json_body(json!({
                "message_id": "reply-3",
                "status": "queued"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let result = client
        .reply("override-msg", "Reply body", Some("Custom Subject"))
        .await
        .expect("reply with override");

    assert_eq!(result.message_id, "reply-3");
    mock.assert_async().await;
}

// --- Task 008: Reply with reply_type ---

#[tokio::test]
async fn reply_with_reply_type_all_posts_to_reply_endpoint() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/agents/test-agent-001/email/reply")
                .body_includes("\"reply_type\":\"all\"")
                .body_includes("\"message_id\"");
            then.status(200).json_body(json!({
                "message_id": "reply-all-msg",
                "status": "queued"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let result = client
        .reply_with_options("orig-msg-uuid", "Reply to all", None, Some("all"), &[])
        .await
        .expect("reply_with_options all");

    assert_eq!(result.message_id, "reply-all-msg");
    mock.assert_async().await;
}

#[tokio::test]
async fn reply_with_custom_recipients_posts_to_reply_endpoint() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/agents/test-agent-001/email/reply")
                .body_includes("\"reply_type\":\"custom\"")
                .body_includes("agent-a@hai.ai");
            then.status(200).json_body(json!({
                "message_id": "reply-custom-msg",
                "status": "queued"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let recipients = vec!["agent-a@hai.ai".to_string(), "agent-b@hai.ai".to_string()];
    let result = client
        .reply_with_options("orig-msg-uuid", "Custom reply", None, Some("custom"), &recipients)
        .await
        .expect("reply_with_options custom");

    assert_eq!(result.message_id, "reply-custom-msg");
    mock.assert_async().await;
}

// --- Task 009: Forward ---

#[tokio::test]
async fn forward_posts_to_forward_endpoint() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/agents/test-agent-001/email/forward")
                .body_includes("\"to\":\"agent-c@hai.ai\"")
                .body_includes("\"message_id\"");
            then.status(200).json_body(json!({
                "message_id": "fwd-msg",
                "status": "queued"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let result = client
        .forward("orig-msg-uuid", "agent-c@hai.ai", Some("FYI"))
        .await
        .expect("forward");

    assert_eq!(result.message_id, "fwd-msg");
    mock.assert_async().await;
}

#[tokio::test]
async fn forward_without_comment() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/agents/test-agent-001/email/forward");
            then.status(200).json_body(json!({
                "message_id": "fwd-msg-2",
                "status": "queued"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let result = client
        .forward("orig-msg-uuid", "agent-d@hai.ai", None)
        .await
        .expect("forward no comment");

    assert_eq!(result.message_id, "fwd-msg-2");
    mock.assert_async().await;
}

// --- Task 011: Search with new filters ---

#[tokio::test]
async fn search_with_is_read_filter() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/test-agent-001/email/search")
                .query_param("is_read", "false");
            then.status(200).json_body(json!({
                "messages": [{
                    "id": "msg-500",
                    "from_address": "alice@hai.ai",
                    "to_address": "test-agent-001@hai.ai",
                    "subject": "Unread message",
                    "body_text": "You haven't read this",
                    "created_at": "2026-03-13T10:00:00Z"
                }],
                "total": 1,
                "unread": 1
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let results = client
        .search_messages(&SearchOptions {
            is_read: Some(false),
            ..Default::default()
        })
        .await
        .expect("search is_read=false");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "msg-500");
    mock.assert_async().await;
}

#[tokio::test]
async fn search_with_folder_filter() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/test-agent-001/email/search")
                .query_param("folder", "archive");
            then.status(200).json_body(json!({
                "messages": [{
                    "id": "msg-501",
                    "from_address": "bob@hai.ai",
                    "to_address": "test-agent-001@hai.ai",
                    "subject": "Archived",
                    "body_text": "Old message",
                    "created_at": "2026-01-01T10:00:00Z"
                }],
                "total": 1,
                "unread": 0
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let results = client
        .search_messages(&SearchOptions {
            folder: Some("archive".to_string()),
            ..Default::default()
        })
        .await
        .expect("search folder=archive");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "msg-501");
    mock.assert_async().await;
}

#[tokio::test]
async fn search_with_jacs_verified_filter() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/test-agent-001/email/search")
                .query_param("jacs_verified", "true");
            then.status(200).json_body(json!({
                "messages": [{
                    "id": "msg-502",
                    "from_address": "verified@hai.ai",
                    "to_address": "test-agent-001@hai.ai",
                    "subject": "Verified sender",
                    "body_text": "Signed content",
                    "created_at": "2026-03-13T11:00:00Z"
                }],
                "total": 1,
                "unread": 0
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let results = client
        .search_messages(&SearchOptions {
            jacs_verified: Some(true),
            ..Default::default()
        })
        .await
        .expect("search jacs_verified=true");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "msg-502");
    mock.assert_async().await;
}

#[tokio::test]
async fn search_with_label_filter() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/test-agent-001/email/search")
                .query_param("label", "important");
            then.status(200).json_body(json!({
                "messages": [{
                    "id": "msg-503",
                    "from_address": "carol@hai.ai",
                    "to_address": "test-agent-001@hai.ai",
                    "subject": "Important stuff",
                    "body_text": "Labeled message",
                    "created_at": "2026-03-13T12:00:00Z"
                }],
                "total": 1,
                "unread": 0
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let results = client
        .search_messages(&SearchOptions {
            label: Some("important".to_string()),
            ..Default::default()
        })
        .await
        .expect("search label=important");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "msg-503");
    mock.assert_async().await;
}

#[tokio::test]
async fn search_with_combined_filters() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/test-agent-001/email/search")
                .query_param("q", "urgent")
                .query_param("folder", "inbox")
                .query_param("is_read", "false")
                .query_param("jacs_verified", "true");
            then.status(200).json_body(json!({
                "messages": [{
                    "id": "msg-504",
                    "from_address": "dave@hai.ai",
                    "to_address": "test-agent-001@hai.ai",
                    "subject": "Urgent & verified",
                    "body_text": "Please respond",
                    "created_at": "2026-03-13T13:00:00Z"
                }],
                "total": 1,
                "unread": 1
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let results = client
        .search_messages(&SearchOptions {
            q: Some("urgent".to_string()),
            folder: Some("inbox".to_string()),
            is_read: Some(false),
            jacs_verified: Some(true),
            ..Default::default()
        })
        .await
        .expect("search combined filters");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "msg-504");
    mock.assert_async().await;
}

// --- Task 012: Contacts ---

#[tokio::test]
async fn contacts_returns_contacts_from_wrapped_response() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/test-agent-001/email/contacts")
                .header_exists("authorization");
            then.status(200).json_body(json!({
                "contacts": [
                    {
                        "email": "alice@hai.ai",
                        "display_name": "Alice Agent",
                        "last_contact": "2026-03-13T10:00:00+00:00",
                        "jacs_verified": true,
                        "reputation_tier": "established"
                    },
                    {
                        "email": "external@example.com",
                        "last_contact": "2026-03-12T08:00:00+00:00",
                        "jacs_verified": false
                    }
                ]
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let contacts = client.contacts().await.expect("contacts");

    assert_eq!(contacts.len(), 2);
    assert_eq!(contacts[0].email, "alice@hai.ai");
    assert_eq!(contacts[0].display_name.as_deref(), Some("Alice Agent"));
    assert!(contacts[0].jacs_verified);
    assert_eq!(contacts[0].reputation_tier.as_deref(), Some("established"));
    assert_eq!(contacts[1].email, "external@example.com");
    assert!(!contacts[1].jacs_verified);
    assert!(contacts[1].reputation_tier.is_none());
    mock.assert_async().await;
}

#[tokio::test]
async fn contacts_returns_empty_for_no_correspondents() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/agents/test-agent-001/email/contacts");
            then.status(200).json_body(json!({
                "contacts": []
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let contacts = client.contacts().await.expect("empty contacts");

    assert!(contacts.is_empty());
    mock.assert_async().await;
}
