use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use futures_util::SinkExt;
use haiai::{A2AMediatedJobOptions, A2ATrustPolicy, TransportType};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
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

/// No mediated run in this file may outlive this budget. Every mock server
/// here speaks raw HTTP or WebSocket frames, so a contract change that leaves
/// the client waiting must surface as a failure, never as a hung suite.
const MEDIATED_RUN_BUDGET: Duration = Duration::from_secs(20);

fn can_bind_localhost() -> bool {
    std::net::TcpListener::bind("127.0.0.1:0").is_ok()
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

/// Serve the key prefetch on the first connection, then the WebSocket upgrade
/// on the second — the exact order `connect_ws` performs them in.
// Tungstenite requires its concrete HTTP rejection response in this callback.
#[allow(clippy::result_large_err)]
async fn start_ws_server(
    fixture: SignedEventFixture,
    events: Vec<serde_json::Value>,
) -> SocketAddr {
    let key_document = fixture.key_document().to_string();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        let (mut key_stream, _) = listener.accept().await.expect("accept key request");
        let request = read_http_request(&mut key_stream).await;
        assert!(
            String::from_utf8_lossy(&request).starts_with("GET /.well-known/hai-keys.json "),
            "unexpected key request: {}",
            String::from_utf8_lossy(&request)
        );
        write_json_response(&mut key_stream, &key_document).await;

        let (stream, _) = listener.accept().await.expect("accept");
        let mut auth = String::new();
        let mut ws = accept_hdr_async(stream, |request: &Request, response: Response| {
            auth = request.headers()["authorization"]
                .to_str()
                .unwrap()
                .to_string();
            Ok(response)
        })
        .await
        .expect("handshake");
        for event in events {
            ws.send(Message::Text(fixture.sign(event, &auth, true).into()))
                .await
                .expect("send signed event");
        }
        ws.send(Message::Close(None)).await.expect("send close");
    });

    addr
}

async fn start_flaky_sse_server(
    fixture: SignedEventFixture,
    fail_first: bool,
) -> (
    String,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
    oneshot::Sender<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let key_document = fixture.key_document().to_string();
    let connect_calls = Arc::new(AtomicUsize::new(0));
    let submit_calls = Arc::new(AtomicUsize::new(0));
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

    let connect_calls_task = Arc::clone(&connect_calls);
    let submit_calls_task = Arc::clone(&submit_calls);

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => {
                    break;
                }
                incoming = listener.accept() => {
                    let Ok((mut stream, _)) = incoming else {
                        break;
                    };

                    let mut buf = vec![0_u8; 16 * 1024];
                    let read = timeout(Duration::from_secs(2), stream.read(&mut buf)).await;
                    let Ok(Ok(n)) = read else {
                        continue;
                    };
                    if n == 0 {
                        continue;
                    }

                    let req = String::from_utf8_lossy(&buf[..n]);
                    // Every `connect_sse` attempt — including each reconnect —
                    // prefetches the key document before the stream GET, so
                    // this branch has to keep answering for the life of the
                    // server rather than only once.
                    if req.starts_with("GET /.well-known/hai-keys.json ") {
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            key_document.len(),
                            key_document
                        );
                        let _ = stream.write_all(response.as_bytes()).await;
                        let _ = stream.shutdown().await;
                        continue;
                    }

                    if req.starts_with("GET /api/v1/agents/connect ") {
                        let attempt = connect_calls_task.fetch_add(1, Ordering::SeqCst);
                        if fail_first && attempt == 0 {
                            let body = "temporary";
                            let response = format!(
                                "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                body.len(),
                                body
                            );
                            let _ = stream.write_all(response.as_bytes()).await;
                            let _ = stream.shutdown().await;
                            continue;
                        }

                        let auth = req.lines().find_map(|line| {
                            let (name,value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("authorization").then(||value.trim())
                        }).expect("request authentication");
                        let benchmark_event = fixture.sign(json!({"type":"benchmark_job","job_id":"job-77"}), auth, false);
                        let disconnect_event = fixture.sign(json!({"type":"disconnect"}), auth, false);
                        let body = format!(
                            "event: benchmark_job\ndata: {benchmark_event}\n\nevent: disconnect\ndata: {disconnect_event}\n\n"
                        );
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes()).await;
                        let _ = stream.shutdown().await;
                        continue;
                    }

                    if req.starts_with("POST /api/v1/agents/jobs/job-77/response ") {
                        // The reply must keep both A2A artifacts inside the signed job response.
                        let mut request_bytes = buf[..n].to_vec();
                        let header_end = request_bytes.windows(4).position(|w|w==b"\r\n\r\n").unwrap()+4;
                        let length: usize = req.lines().find_map(|line| {
                            let (name,value)=line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length").then(||value.trim().parse().unwrap())
                        }).unwrap();
                        while request_bytes.len()<header_end+length {
                            let n=stream.read(&mut buf).await.expect("read reply body");
                            assert!(n>0);request_bytes.extend_from_slice(&buf[..n]);
                        }
                        let body=String::from_utf8_lossy(&request_bytes[header_end..]);
                        assert!(body.contains("a2aTask") && body.contains("a2aResult"));
                        submit_calls_task.fetch_add(1, Ordering::SeqCst);
                        let body = "{\"success\":true,\"job_id\":\"job-77\",\"message\":\"ok\"}";
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(response.as_bytes()).await;
                        let _ = stream.shutdown().await;
                        continue;
                    }

                    let body = "{\"error\":\"not_found\"}";
                    let response = format!(
                        "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.shutdown().await;
                }
            }
        }
    });

    (
        format!("http://{addr}"),
        connect_calls,
        submit_calls,
        shutdown_tx,
    )
}

#[tokio::test]
async fn mediated_sse_signs_and_submits_wrapped_artifacts() {
    if !can_bind_localhost() {
        eprintln!(
            "skipping mediated_sse_signs_and_submits_wrapped_artifacts: localhost bind unavailable"
        );
        return;
    }

    let _guard = TEST_ENV_LOCK.lock().await;
    let _password = RestorePassword::set();
    let fixture = SignedEventFixture::new();
    let requester = SignedEventFixture::new();
    let (base_url, connect_calls, submit_calls, shutdown_tx) =
        start_flaky_sse_server(fixture, false).await;
    let client = make_client(&base_url, &requester);
    let a2a = client.get_a2a(Some(A2ATrustPolicy::Verified));

    timeout(
        MEDIATED_RUN_BUDGET,
        a2a.on_mediated_benchmark_job(A2AMediatedJobOptions::default(), |task| async move {
            assert_eq!(task.jacs_type, "a2a-task");
            Ok(json!({
                "message": "handled",
                "decision": "allow"
            }))
        }),
    )
    .await
    .expect("mediated sse run timed out")
    .expect("mediated sse run");

    assert_eq!(connect_calls.load(Ordering::SeqCst), 1);
    assert_eq!(submit_calls.load(Ordering::SeqCst), 1);
    let _ = shutdown_tx.send(());
}

#[tokio::test]
async fn mediated_ws_rejects_untrusted_card_when_policy_enforced() {
    if !can_bind_localhost() {
        eprintln!("skipping mediated_ws_rejects_untrusted_card_when_policy_enforced: localhost bind unavailable");
        return;
    }

    let _guard = TEST_ENV_LOCK.lock().await;
    let _password = RestorePassword::set();
    let requester = SignedEventFixture::new();
    let fixture = SignedEventFixture::new();
    let addr = start_ws_server(
        fixture,
        vec![json!({
            "type": "benchmark_job",
            "job_id": "job-9",
            "remoteAgentCard": {
                "name": "remote-agent",
                "metadata": {"jacsId": "unknown-agent"},
                "capabilities": {}
            }
        })],
    )
    .await;

    let client = make_client(&format!("http://{addr}"), &requester);
    let a2a = client.get_a2a(Some(A2ATrustPolicy::Strict));
    let handler_called = Arc::new(AtomicBool::new(false));
    let handler_called_inner = Arc::clone(&handler_called);

    let err = timeout(
        MEDIATED_RUN_BUDGET,
        a2a.on_mediated_benchmark_job(
            A2AMediatedJobOptions {
                transport: TransportType::Ws,
                enforce_trust_policy: true,
                ..A2AMediatedJobOptions::default()
            },
            move |_task| {
                handler_called_inner.store(true, Ordering::SeqCst);
                async move { Ok(json!({"message":"unreachable"})) }
            },
        ),
    )
    .await
    .expect("mediated ws run timed out")
    .expect_err("expected trust rejection");

    assert!(
        err.to_string()
            .contains("trust policy rejected remote agent"),
        "unexpected error: {err}"
    );
    assert!(!handler_called.load(Ordering::SeqCst));
}

#[tokio::test]
async fn mediated_ws_rejects_invalid_inbound_signature() {
    if !can_bind_localhost() {
        eprintln!(
            "skipping mediated_ws_rejects_invalid_inbound_signature: localhost bind unavailable"
        );
        return;
    }

    let _guard = TEST_ENV_LOCK.lock().await;
    let _password = RestorePassword::set();
    let requester = SignedEventFixture::new();
    let fixture = SignedEventFixture::new();
    // The transport envelope is authentic; the *inner* A2A task carries a
    // bogus signature, which is the rejection this test is about.
    let addr = start_ws_server(
        fixture,
        vec![json!({
            "type": "benchmark_job",
            "job_id": "job-11",
            "a2aTask": {
                "jacsId": "inbound-task-1",
                "jacsVersion": "1.0.0",
                "jacsType": "a2a-task",
                "jacsLevel": "artifact",
                "jacsVersionDate": "2026-02-24T00:00:00Z",
                "a2aArtifact": {"taskId": "task-1"},
                "jacsSignature": {
                    "agentID": "agent/with/slash",
                    "date": "2026-02-24T00:00:00Z",
                    "signature": "not-a-valid-signature"
                }
            }
        })],
    )
    .await;

    let client = make_client(&format!("http://{addr}"), &requester);
    let a2a = client.get_a2a(Some(A2ATrustPolicy::Verified));

    let err = timeout(
        MEDIATED_RUN_BUDGET,
        a2a.on_mediated_benchmark_job(
            A2AMediatedJobOptions {
                transport: TransportType::Ws,
                verify_inbound_artifact: true,
                ..A2AMediatedJobOptions::default()
            },
            |_task| async move { Ok(json!({"message":"unreachable"})) },
        ),
    )
    .await
    .expect("mediated ws run timed out")
    .expect_err("expected signature rejection");

    assert!(
        err.to_string()
            .contains("inbound a2a task signature invalid"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn mediated_sse_reconnects_after_initial_failure() {
    if !can_bind_localhost() {
        eprintln!(
            "skipping mediated_sse_reconnects_after_initial_failure: localhost bind unavailable"
        );
        return;
    }

    let _guard = TEST_ENV_LOCK.lock().await;
    let _password = RestorePassword::set();
    let fixture = SignedEventFixture::new();
    let requester = SignedEventFixture::new();
    let (base_url, connect_calls, submit_calls, shutdown_tx) =
        start_flaky_sse_server(fixture, true).await;
    let client = make_client(&base_url, &requester);
    let a2a = client.get_a2a(Some(A2ATrustPolicy::Verified));

    timeout(
        MEDIATED_RUN_BUDGET,
        a2a.on_mediated_benchmark_job(
            A2AMediatedJobOptions {
                transport: TransportType::Sse,
                max_reconnect_attempts: 1,
                ..A2AMediatedJobOptions::default()
            },
            |_task| async move { Ok(json!({"message":"recovered"})) },
        ),
    )
    .await
    .expect("mediated reconnect timed out")
    .expect("mediated reconnect");

    assert!(
        connect_calls.load(Ordering::SeqCst) >= 2,
        "expected at least 2 connect attempts"
    );
    assert_eq!(submit_calls.load(Ordering::SeqCst), 1);
    let _ = shutdown_tx.send(());
}
