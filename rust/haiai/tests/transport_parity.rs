use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use futures_util::SinkExt;
use haiai::TransportType;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::{timeout, Duration};
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        handshake::server::{Request, Response},
        Message,
    },
};

#[path = "support/signed_transport.rs"]
mod signed_transport;
use signed_transport::*;

#[tokio::test]
async fn connect_sse_streams_connected_and_benchmark_events() {
    let _env_guard = TEST_ENV_LOCK.lock().await;
    let _password = RestorePassword::set();
    let fixture = SignedEventFixture::new();
    let requester = SignedEventFixture::new();
    let (addr, server_task) = start_sse_server(
        fixture,
        vec![
            json!({"type": "connected", "agent_id": "a-1"}),
            json!({"type": "benchmark_job", "job_id": "job-1", "scenario_id": "s-1"}),
        ],
    )
    .await;
    let client = make_client(&format!("http://{addr}"), &requester);
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
    server_task.await.expect("SSE server");
}

async fn start_ws_server(
    fixture: SignedEventFixture,
    events: Vec<serde_json::Value>,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");

    let task = tokio::spawn(async move {
        let (mut key_stream, _) = listener.accept().await.expect("accept key request");
        let request = read_http_request(&mut key_stream).await;
        assert!(
            String::from_utf8_lossy(&request).starts_with("GET /.well-known/hai-keys.json "),
            "unexpected key request: {}",
            String::from_utf8_lossy(&request)
        );
        write_json_response(&mut key_stream, &fixture.key_document().to_string()).await;

        let (stream, _) = listener.accept().await.expect("accept");
        let mut auth = String::new();
        // Tungstenite fixes this callback's error type to its full HTTP response.
        #[allow(clippy::result_large_err)]
        let capture_auth = |request: &Request, response: Response| {
            assert_eq!(request.uri().path(), "/ws/agent/connect");
            auth = request
                .headers()
                .get("authorization")
                .expect("upgrade auth")
                .to_str()
                .expect("auth text")
                .to_string();
            Ok(response)
        };
        let mut ws = accept_hdr_async(stream, capture_auth)
            .await
            .expect("handshake");

        for event in events {
            ws.send(Message::Text(fixture.sign(event, &auth, true).into()))
                .await
                .expect("send signed event");
        }

        ws.send(Message::Close(None)).await.expect("send close");
    });

    (addr, task)
}

fn request_auth_header(request: &[u8]) -> String {
    std::str::from_utf8(request)
        .expect("HTTP request text")
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("authorization")
                .then(|| value.trim().to_string())
        })
        .expect("received request authorization")
}

async fn start_sse_server(
    fixture: SignedEventFixture,
    events: Vec<serde_json::Value>,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let task = tokio::spawn(async move {
        let (mut key_stream, _) = listener.accept().await.expect("key request");
        let request = read_http_request(&mut key_stream).await;
        assert!(String::from_utf8_lossy(&request).starts_with("GET /.well-known/hai-keys.json "));
        write_json_response(&mut key_stream, &fixture.key_document().to_string()).await;

        let (mut stream, _) = listener.accept().await.expect("SSE request");
        let request = read_http_request(&mut stream).await;
        assert!(String::from_utf8_lossy(&request).starts_with("GET /api/v1/agents/connect "));
        let auth = request_auth_header(&request);
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .expect("SSE headers");
        for event in events {
            let event_type = event["type"].as_str().expect("event type").to_string();
            let signed = fixture.sign(event, &auth, false);
            stream
                .write_all(format!("event: {event_type}\ndata: {signed}\n\n").as_bytes())
                .await
                .expect("SSE event");
        }
        stream.shutdown().await.expect("close SSE response");
    });
    (addr, task)
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
    retired: SignedEventFixture,
    replacement: SignedEventFixture,
) -> (
    SocketAddr,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let (shutdown, mut shutdown_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut initial_key_stream, _) = listener.accept().await.expect("initial key request");
        let request = read_http_request(&mut initial_key_stream).await;
        assert!(String::from_utf8_lossy(&request).starts_with("GET /.well-known/hai-keys.json "));
        write_json_response(&mut initial_key_stream, &retired.key_document().to_string()).await;

        let (mut sse_stream, _) = listener.accept().await.expect("SSE request");
        let request = read_http_request(&mut sse_stream).await;
        assert!(String::from_utf8_lossy(&request).starts_with("GET /api/v1/agents/connect "));
        let auth = request_auth_header(&request);
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
        let replacement_keys = replacement.key_document().to_string();
        write_json_response(&mut refresh_stream, &replacement_keys).await;

        let refreshed_event = replacement.sign(
            json!({"type": "benchmark_job", "job_id": "new-key-job"}),
            &auth,
            false,
        );
        let retired_event = retired.sign(
            json!({"type": "benchmark_job", "job_id": "retired-key-job"}),
            &auth,
            false,
        );
        let body = format!("data: {refreshed_event}\n\ndata: {retired_event}\n\n");
        sse_stream
            .write_all(body.as_bytes())
            .await
            .expect("write post-refresh SSE events");
        sse_stream.shutdown().await.expect("close SSE response");

        // The production client can legitimately refresh again before consuming
        // queued events. Keep this fixture's key service alive until the test
        // closes its connection; do not race a 20ms timer by closing the listener.
        timeout(Duration::from_secs(10), async {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => break,
                    accepted = listener.accept() => {
                        let (mut stream, _) = accepted.expect("repeated refresh request");
                        let request = read_http_request(&mut stream).await;
                        assert!(String::from_utf8_lossy(&request)
                            .starts_with("GET /.well-known/hai-keys.json "));
                        write_json_response(&mut stream, &replacement_keys).await;
                    }
                }
            }
        })
        .await
        .expect("refresh fixture was not shut down by the test");
    });
    (addr, shutdown, task)
}

#[tokio::test]
async fn connect_ws_streams_connected_and_benchmark_events() {
    let _env_guard = TEST_ENV_LOCK.lock().await;
    let _password = RestorePassword::set();
    let fixture = SignedEventFixture::new();
    let requester = SignedEventFixture::new();
    let events = vec![
        json!({"type": "connected", "agent_id": "a-1"}),
        json!({
            "type": "benchmark_job",
            "job_id": "job-1",
            "scenario_id": "s-1"
        }),
    ];
    let (addr, server_task) = start_ws_server(fixture, events).await;
    let base_url = format!("http://{}", addr);
    let client = make_client(&base_url, &requester);

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
    server_task.await.expect("WebSocket server");
}

#[tokio::test]
async fn direct_sse_connection_atomically_replaces_keys_on_bounded_refresh() {
    let _env_guard = TEST_ENV_LOCK.lock().await;
    let _password = RestorePassword::set();
    let retired = SignedEventFixture::new();
    let replacement = SignedEventFixture::new();
    let replacement_id = replacement.signer_id.clone();
    let requester = SignedEventFixture::new();
    let (addr, shutdown, server_task) = start_refreshing_sse_server(retired, replacement).await;
    let client = make_client(&format!("http://{addr}"), &requester)
        .with_server_key_refresh_interval(Duration::from_millis(20))
        .expect("short test refresh interval");
    let mut conn = client.connect_sse().await.expect("connect SSE");

    let refreshed = timeout(Duration::from_secs(2), conn.next_event())
        .await
        .expect("refreshed event timeout")
        .expect("refreshed event verification")
        .expect("refreshed event");
    assert_eq!(refreshed.data["job_id"], "new-key-job");
    assert_eq!(refreshed.verification.signer_id, replacement_id);

    let retired_error = timeout(Duration::from_secs(2), conn.next_event())
        .await
        .expect("retired event timeout")
        .expect_err("retired key must not remain unioned into the refreshed snapshot");
    assert!(retired_error
        .to_string()
        .contains("event signer is not in the trusted active server key set"));

    conn.close().await;
    shutdown.send(()).expect("shut down refresh fixture");
    server_task.await.expect("refreshing SSE server");
}

#[tokio::test]
async fn on_benchmark_job_dispatches_sse_benchmark_events() {
    let _env_guard = TEST_ENV_LOCK.lock().await;
    let _password = RestorePassword::set();
    let fixture = SignedEventFixture::new();
    let requester = SignedEventFixture::new();
    let (addr, server_task) = start_sse_server(
        fixture,
        vec![
            json!({"type": "benchmark_job", "job_id": "job-42"}),
            json!({"type": "disconnect", "reason": "done"}),
        ],
    )
    .await;
    let client = make_client(&format!("http://{addr}"), &requester);
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
    server_task.await.expect("callback SSE server");
}
