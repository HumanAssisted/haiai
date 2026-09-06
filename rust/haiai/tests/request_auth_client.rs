//! Regression contract for the actual SDK request boundary, using a local
//! recording provider and loopback HTTP fixtures only.
use haiai::error::Result;
use haiai::{HaiClient, HaiClientOptions, JacsProvider, StaticJacsProvider};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
struct SignedRequest {
    method: String,
    url: String,
    body: Vec<u8>,
    audience: String,
}
struct RecordingProvider {
    calls: Arc<Mutex<Vec<SignedRequest>>>,
    inner: StaticJacsProvider,
}
impl JacsProvider for RecordingProvider {
    fn jacs_id(&self) -> &str {
        self.inner.jacs_id()
    }
    fn sign_string(&self, _: &str) -> Result<String> {
        panic!("HTTP must never use legacy string credentials")
    }
    fn sign_bytes(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        self.inner.sign_bytes(bytes)
    }
    fn key_id(&self) -> &str {
        self.inner.key_id()
    }
    fn algorithm(&self) -> &str {
        self.inner.algorithm()
    }
    fn canonical_json(&self, value: &Value) -> Result<String> {
        self.inner.canonical_json(value)
    }
    fn sign_response(&self, value: &Value) -> Result<haiai::SignedPayload> {
        self.inner.sign_response(value)
    }
    fn build_request_auth_header(
        &self,
        method: &str,
        url: &str,
        body: &[u8],
        audience: &str,
    ) -> Result<String> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(SignedRequest {
            method: method.into(),
            url: url.into(),
            body: body.into(),
            audience: audience.into(),
        });
        Ok(format!("JACS v2.local-fixture-{}", calls.len()))
    }
}
fn client(url: String) -> (HaiClient<RecordingProvider>, Arc<Mutex<Vec<SignedRequest>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let client = HaiClient::new(
        RecordingProvider {
            calls: calls.clone(),
            inner: StaticJacsProvider::new("request-agent"),
        },
        HaiClientOptions {
            base_url: url,
            max_retries: 1,
            ..Default::default()
        },
    )
    .unwrap();
    (client, calls)
}

#[tokio::test]
async fn actual_hello_signs_final_json_bytes_and_current_request_context() {
    let server = httpmock::MockServer::start_async().await;
    let expected =
        serde_json::to_vec(&json!({"agent_id":"request-agent", "include_test":true})).unwrap();
    let wire_bytes = expected.clone();
    let response = server
        .mock_async(|when, then| {
            when.method("POST")
                .path("/api/v1/agents/hello")
                .header("authorization", "JACS v2.local-fixture-1")
                .is_true(move |request: &httpmock::HttpMockRequest| {
                    request.body_ref() == wire_bytes.as_slice()
                });
            then.status(200).json_body(json!({"message":"hello"}));
        })
        .await;
    let (client, calls) = client(server.base_url());
    client.hello(true).await.unwrap();
    response.assert_async().await;
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "POST");
    assert_eq!(
        calls[0].url,
        format!("{}/api/v1/agents/hello", server.base_url())
    );
    assert_eq!(calls[0].body, expected);
    assert_eq!(calls[0].audience, "hai.ai");
}

#[tokio::test]
async fn authenticated_redirect_is_not_replayed_or_resigned() {
    let server = httpmock::MockServer::start_async().await;
    let redirect = server
        .mock_async(|when, then| {
            when.method("POST").path("/api/v1/agents/hello");
            then.status(307).header("location", "/different");
        })
        .await;
    let destination = server
        .mock_async(|when, then| {
            when.path("/different");
            then.status(200);
        })
        .await;
    let (client, calls) = client(server.base_url());
    assert!(client.hello(false).await.is_err());
    redirect.assert_calls_async(1).await;
    destination.assert_calls_async(0).await;
    assert_eq!(calls.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn retried_request_has_identical_entity_but_fresh_authentication() {
    let server = httpmock::MockServer::start_async().await;
    let first = server
        .mock_async(|when, then| {
            when.path("/api/v1/agents/hello")
                .header("authorization", "JACS v2.local-fixture-1");
            then.status(503);
        })
        .await;
    let second = server
        .mock_async(|when, then| {
            when.path("/api/v1/agents/hello")
                .header("authorization", "JACS v2.local-fixture-2");
            then.status(200).json_body(json!({"message":"hello"}));
        })
        .await;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let client = HaiClient::new(
        RecordingProvider {
            calls: calls.clone(),
            inner: StaticJacsProvider::new("retry-agent"),
        },
        HaiClientOptions {
            base_url: server.base_url(),
            max_retries: 2,
            ..Default::default()
        },
    )
    .unwrap();
    client.hello(false).await.unwrap();
    first.assert_calls_async(1).await;
    second.assert_calls_async(1).await;
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].body, calls[1].body);
    assert_eq!(calls[0].url, calls[1].url);
    assert_eq!(calls[0].method, calls[1].method);
}

#[tokio::test]
async fn query_and_escaped_path_are_bound_after_all_builder_updates() {
    let server = httpmock::MockServer::start_async().await;
    let response = server
        .mock_async(|when, then| {
            when.method("GET")
                .path("/api/agents/account%2Fsegment/email/messages")
                .query_param("label", "a+b & c")
                .query_param("limit", "7");
            then.status(200).json_body(json!({"messages":[]}));
        })
        .await;
    let (mut client, calls) = client(server.base_url());
    client.set_hai_agent_id("account/segment".into());
    client
        .list_messages(&haiai::ListMessagesOptions {
            limit: Some(7),
            label: Some("a+b & c".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    response.assert_async().await;
    let calls = calls.lock().unwrap();
    assert_eq!(calls[0].method, "GET");
    assert!(calls[0].body.is_empty());
    assert_eq!(
        calls[0].url,
        format!(
            "{}/api/agents/account%2Fsegment/email/messages?limit=7&label=a%2Bb+%26+c",
            server.base_url()
        )
    );
}

#[test]
fn deployment_audience_is_explicit_and_may_not_be_empty() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let client = HaiClient::new(
        RecordingProvider {
            calls: calls.clone(),
            inner: StaticJacsProvider::new("request-agent"),
        },
        HaiClientOptions {
            base_url: "https://hai.example".into(),
            request_auth_audience: "my-hai-deployment".into(),
            ..Default::default()
        },
    )
    .unwrap();
    client
        .build_request_auth_header("GET", "https://hai.example/api", &[])
        .unwrap();
    assert_eq!(calls.lock().unwrap()[0].audience, "my-hai-deployment");
    assert!(HaiClient::new(
        StaticJacsProvider::new("agent"),
        HaiClientOptions {
            request_auth_audience: " ".into(),
            ..Default::default()
        }
    )
    .is_err());
}

#[test]
fn caller_built_request_binds_binary_body_query_and_pinned_audience() {
    let (client, calls) = client("https://hai.example".into());
    let bytes = [0, 0xff, b'\r', b'\n'];
    client
        .build_request_auth_header(
            "PATCH",
            "https://hai.example/api/items/a%2Fb?x=%2B&x=two",
            &bytes,
        )
        .unwrap();
    let calls = calls.lock().unwrap();
    assert_eq!(calls[0].method, "PATCH");
    assert_eq!(
        calls[0].url,
        "https://hai.example/api/items/a%2Fb?x=%2B&x=two"
    );
    assert_eq!(calls[0].body, bytes);
    assert_eq!(calls[0].audience, "hai.ai");
}

#[test]
fn no_context_or_wrong_origin_refuses_before_signing() {
    let (client, calls) = client("https://hai.example".into());
    assert!(client
        .build_auth_header()
        .unwrap_err()
        .to_string()
        .contains("request"));
    for url in [
        "https://other.example/api",
        "http://hai.example/api",
        "https://hai.example:444/api",
        "https://user@hai.example/api",
        "https://hai.example/api#fragment",
    ] {
        assert!(
            client.build_request_auth_header("GET", url, &[]).is_err(),
            "{url}"
        );
    }
    assert!(client
        .build_request_auth_header(
            "POST",
            "https://hai.example/api",
            &vec![0; 10 * 1024 * 1024 + 1]
        )
        .is_err());
    assert!(calls.lock().unwrap().is_empty());
}
