use haisdk::{HaiClient, HaiClientOptions, StaticJacsProvider};
use httpmock::Method::POST;
use httpmock::MockServer;
use serde_json::json;

#[tokio::test]
async fn verify_document_posts_to_public_endpoint_without_auth() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(POST).path("/api/jacs/verify").json_body(json!({
                "document": "{\"jacsId\":\"agent-1\"}"
            }));
            then.status(200).json_body(json!({
                "valid": true,
                "verified_at": "2026-01-01T00:00:00Z",
                "document_type": "JacsDocument",
                "issuer_verified": true,
                "signature_verified": true,
                "signer_id": "agent-1",
                "signed_at": "2026-01-01T00:00:00Z"
            }));
        })
        .await;

    let client = HaiClient::new(
        StaticJacsProvider::new("agent/with/slash"),
        HaiClientOptions {
            base_url: server.base_url(),
            ..HaiClientOptions::default()
        },
    )
    .expect("client");

    let result = client
        .verify_document("{\"jacsId\":\"agent-1\"}")
        .await
        .expect("verify document");
    assert!(result.valid);
    assert_eq!(result.document_type, "JacsDocument");

    mock.assert_async().await;
}
