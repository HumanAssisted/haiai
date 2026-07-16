use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use futures_util::SinkExt;
use haiai::{HaiClient, HaiClientOptions, StaticJacsProvider, TransportType};
use httpmock::Method::GET;
use httpmock::MockServer;
use jacs::simple::SimpleAgent;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::{timeout, Duration};
use tokio_tungstenite::{accept_async, tungstenite::Message};

fn make_client(base_url: &str) -> HaiClient<StaticJacsProvider> {
    let provider = StaticJacsProvider::new("agent/with/slash");
    HaiClient::new(
        provider,
        HaiClientOptions {
            base_url: base_url.to_string(),
            ..HaiClientOptions::default()
        },
    )
    .expect("client")
}

struct SignedEventFixture {
    signer: SimpleAgent,
    signer_id: String,
    public_key_pem: String,
}

impl SignedEventFixture {
    fn new() -> Self {
        let (signer, _) = SimpleAgent::ephemeral(Some("ed25519")).expect("ephemeral signer");
        let probe = signer
            .sign_response(&json!({"type": "fixture_probe"}))
            .expect("sign fixture probe");
        let signer_id = probe["jacsSignature"]["agentID"]
            .as_str()
            .expect("fixture signer id")
            .to_string();
        let public_key_pem = signer.get_public_key_pem().expect("fixture public key");
        Self {
            signer,
            signer_id,
            public_key_pem,
        }
    }

    fn sign(&self, payload: serde_json::Value) -> String {
        serde_json::to_string(
            &self
                .signer
                .sign_response(&payload)
                .expect("sign transport event"),
        )
        .expect("serialize signed transport event")
    }

    fn key_document(&self) -> serde_json::Value {
        json!({
            "keys": [{
                "signer_id": self.signer_id,
                "public_key": self.public_key_pem,
                "is_active": true
            }]
        })
    }
}

#[tokio::test]
async fn connect_sse_streams_connected_and_benchmark_events() {
    let server = MockServer::start_async().await;
    let fixture = SignedEventFixture::new();
    let connected = fixture.sign(json!({"type": "connected", "agent_id": "a-1"}));
    let benchmark = fixture.sign(json!({
        "type": "benchmark_job",
        "job_id": "job-1",
        "scenario_id": "s-1"
    }));

    let keys = server
        .mock_async(|when, then| {
            when.method(GET).path("/.well-known/hai-keys.json");
            then.status(200).json_body(fixture.key_document());
        })
        .await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET).path("/api/v1/agents/connect");
            then.status(200)
                .header("Content-Type", "text/event-stream")
                .body(format!(
                    "event: connected\ndata: {connected}\n\nevent: benchmark_job\ndata: {benchmark}\n\n"
                ));
        })
        .await;

    let client = make_client(&server.base_url());
    let mut conn = client.connect_sse().await.expect("connect sse");

    let first = timeout(Duration::from_secs(2), conn.next_event())
        .await
        .expect("first event timeout")
        .expect("first event verification")
        .expect("first event");
    assert_eq!(first.event_type, "connected");
    assert_eq!(first.verification.status, "verified");

    let second = timeout(Duration::from_secs(2), conn.next_event())
        .await
        .expect("second event timeout")
        .expect("second event verification")
        .expect("second event");
    assert_eq!(second.event_type, "benchmark_job");
    assert_eq!(
        second.data.get("job_id").and_then(|v| v.as_str()),
        Some("job-1")
    );

    conn.close().await;
    keys.assert_async().await;
    mock.assert_async().await;
}

async fn start_ws_server(key_document: String, events: Vec<String>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        let (mut key_stream, _) = listener.accept().await.expect("accept key request");
        let mut request = Vec::new();
        let mut chunk = [0_u8; 1024];
        while !request.ends_with(b"\r\n\r\n") {
            let read = key_stream.read(&mut chunk).await.expect("read key request");
            assert!(read > 0, "key request closed before headers completed");
            request.extend_from_slice(&chunk[..read]);
            assert!(request.len() <= 16 * 1024, "key request headers too large");
        }
        assert!(
            String::from_utf8_lossy(&request).starts_with("GET /.well-known/hai-keys.json "),
            "unexpected key request: {}",
            String::from_utf8_lossy(&request)
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            key_document.len(),
            key_document
        );
        key_stream
            .write_all(response.as_bytes())
            .await
            .expect("write key response");
        key_stream.shutdown().await.expect("close key response");

        let (stream, _) = listener.accept().await.expect("accept");
        let mut ws = accept_async(stream).await.expect("handshake");

        for event in events {
            ws.send(Message::Text(event.into()))
                .await
                .expect("send signed event");
        }

        ws.send(Message::Close(None)).await.expect("send close");
    });

    addr
}

async fn read_http_request(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 1024];
    while !request.ends_with(b"\r\n\r\n") {
        let read = stream.read(&mut chunk).await.expect("read HTTP request");
        assert!(read > 0, "HTTP request closed before headers completed");
        request.extend_from_slice(&chunk[..read]);
        assert!(request.len() <= 16 * 1024, "HTTP request headers too large");
    }
    request
}

async fn write_json_response(stream: &mut tokio::net::TcpStream, document: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        document.len(),
        document
    );
    stream
        .write_all(response.as_bytes())
        .await
        .expect("write JSON response");
    stream.shutdown().await.expect("close JSON response");
}

async fn start_refreshing_sse_server(
    initial_keys: String,
    replacement_keys: String,
    refreshed_event: String,
    retired_event: String,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let task = tokio::spawn(async move {
        let (mut initial_key_stream, _) = listener.accept().await.expect("initial key request");
        let request = read_http_request(&mut initial_key_stream).await;
        assert!(String::from_utf8_lossy(&request).starts_with("GET /.well-known/hai-keys.json "));
        write_json_response(&mut initial_key_stream, &initial_keys).await;

        let (mut sse_stream, _) = listener.accept().await.expect("SSE request");
        let request = read_http_request(&mut sse_stream).await;
        assert!(String::from_utf8_lossy(&request).starts_with("GET /api/v1/agents/connect "));
        sse_stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .expect("write SSE response headers");

        // A direct connection (without the reconnect helper) must refresh its
        // key snapshot on the configured timer before another payload arrives.
        let (mut refresh_stream, _) = listener.accept().await.expect("refresh key request");
        let request = read_http_request(&mut refresh_stream).await;
        assert!(String::from_utf8_lossy(&request).starts_with("GET /.well-known/hai-keys.json "));
        write_json_response(&mut refresh_stream, &replacement_keys).await;

        let body = format!("data: {refreshed_event}\n\ndata: {retired_event}\n\n");
        sse_stream
            .write_all(body.as_bytes())
            .await
            .expect("write post-refresh SSE events");
        sse_stream.shutdown().await.expect("close SSE response");
    });
    (addr, task)
}

#[tokio::test]
async fn connect_ws_streams_connected_and_benchmark_events() {
    let fixture = SignedEventFixture::new();
    let events = vec![
        fixture.sign(json!({"type": "connected", "agent_id": "a-1"})),
        fixture.sign(json!({
            "type": "benchmark_job",
            "job_id": "job-1",
            "scenario_id": "s-1"
        })),
    ];
    let addr = start_ws_server(fixture.key_document().to_string(), events).await;
    let base_url = format!("http://{}", addr);
    let client = make_client(&base_url);

    let mut conn = client.connect_ws().await.expect("connect ws");

    let first = timeout(Duration::from_secs(2), conn.next_event())
        .await
        .expect("first event timeout")
        .expect("first event verification")
        .expect("first event");
    assert_eq!(first.event_type, "connected");

    let second = timeout(Duration::from_secs(2), conn.next_event())
        .await
        .expect("second event timeout")
        .expect("second event verification")
        .expect("second event");
    assert_eq!(second.event_type, "benchmark_job");
    assert_eq!(
        second.data.get("job_id").and_then(|v| v.as_str()),
        Some("job-1")
    );

    conn.close().await;
}

#[tokio::test]
async fn direct_sse_connection_atomically_replaces_keys_on_bounded_refresh() {
    let retired = SignedEventFixture::new();
    let replacement = SignedEventFixture::new();
    let refreshed_event = replacement.sign(json!({
        "type": "benchmark_job",
        "job_id": "new-key-job"
    }));
    let retired_event = retired.sign(json!({
        "type": "benchmark_job",
        "job_id": "retired-key-job"
    }));
    let (addr, server_task) = start_refreshing_sse_server(
        retired.key_document().to_string(),
        replacement.key_document().to_string(),
        refreshed_event,
        retired_event,
    )
    .await;
    let client = make_client(&format!("http://{addr}"))
        .with_server_key_refresh_interval(Duration::from_millis(20))
        .expect("short test refresh interval");
    let mut conn = client.connect_sse().await.expect("connect SSE");

    let refreshed = timeout(Duration::from_secs(2), conn.next_event())
        .await
        .expect("refreshed event timeout")
        .expect("refreshed event verification")
        .expect("refreshed event");
    assert_eq!(refreshed.data["job_id"], "new-key-job");
    assert_eq!(refreshed.verification.signer_id, replacement.signer_id);

    let retired_error = timeout(Duration::from_secs(2), conn.next_event())
        .await
        .expect("retired event timeout")
        .expect_err("retired key must not remain unioned into the refreshed snapshot");
    assert!(retired_error.to_string().contains("Unknown signer"));

    conn.close().await;
    server_task.await.expect("refreshing SSE server");
}

#[tokio::test]
async fn on_benchmark_job_dispatches_sse_benchmark_events() {
    let server = MockServer::start_async().await;
    let fixture = SignedEventFixture::new();
    let benchmark = fixture.sign(json!({"type": "benchmark_job", "job_id": "job-42"}));
    let disconnect = fixture.sign(json!({"type": "disconnect", "reason": "done"}));

    let keys = server
        .mock_async(|when, then| {
            when.method(GET).path("/.well-known/hai-keys.json");
            then.status(200).json_body(fixture.key_document());
        })
        .await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET).path("/api/v1/agents/connect");
            then.status(200)
                .header("Content-Type", "text/event-stream")
                .body(format!(
                    "event: benchmark_job\ndata: {benchmark}\n\nevent: disconnect\ndata: {disconnect}\n\n"
                ));
        })
        .await;

    let client = make_client(&server.base_url());
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen_clone = Arc::clone(&seen);

    client
        .on_benchmark_job(TransportType::Sse, move |data| {
            let seen_inner = Arc::clone(&seen_clone);
            async move {
                if let Some(job_id) = data.get("job_id").and_then(|v| v.as_str()) {
                    seen_inner
                        .lock()
                        .expect("lock seen")
                        .push(job_id.to_string());
                }
                Ok(())
            }
        })
        .await
        .expect("on benchmark job");

    assert_eq!(seen.lock().expect("lock seen").as_slice(), ["job-42"]);
    keys.assert_async().await;
    mock.assert_async().await;
}
