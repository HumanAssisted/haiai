use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use futures_util::SinkExt;
use haisdk::{
    A2AMediatedJobOptions, A2ATrustPolicy, HaiClient, HaiClientOptions, StaticJacsProvider,
    TransportType,
};
use httpmock::Method::{GET, POST};
use httpmock::MockServer;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
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

fn can_bind_localhost() -> bool {
    std::net::TcpListener::bind("127.0.0.1:0").is_ok()
}

async fn start_ws_server(events: Vec<serde_json::Value>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut ws = accept_async(stream).await.expect("handshake");
        for event in events {
            ws.send(Message::Text(event.to_string().into()))
                .await
                .expect("send event");
        }
        ws.send(Message::Close(None)).await.expect("send close");
    });

    addr
}

async fn start_flaky_sse_server() -> (
    String,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
    oneshot::Sender<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
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
                    if req.starts_with("GET /api/v1/agents/connect ") {
                        let attempt = connect_calls_task.fetch_add(1, Ordering::SeqCst);
                        if attempt == 0 {
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

                        let body = concat!(
                            "event: benchmark_job\n",
                            "data: {\"type\":\"benchmark_job\",\"job_id\":\"job-77\"}\n\n",
                            "event: disconnect\n",
                            "data: {\"type\":\"disconnect\"}\n\n",
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

    let server = MockServer::start_async().await;

    let connect = server
        .mock_async(|when, then| {
            when.method(GET).path("/api/v1/agents/connect");
            then.status(200)
                .header("Content-Type", "text/event-stream")
                .body(concat!(
                    "event: benchmark_job\n",
                    "data: {\"type\":\"benchmark_job\",\"job_id\":\"job-42\"}\n\n",
                    "event: disconnect\n",
                    "data: {\"type\":\"disconnect\"}\n\n",
                ));
        })
        .await;

    let submit = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/v1/agents/jobs/job-42/response")
                .body_includes("a2aTask")
                .body_includes("a2aResult");
            then.status(200).json_body(json!({
                "success": true,
                "job_id": "job-42",
                "message": "ok"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let a2a = client.get_a2a(Some(A2ATrustPolicy::Verified));

    a2a.on_mediated_benchmark_job(A2AMediatedJobOptions::default(), |task| async move {
        assert_eq!(task.jacs_type, "a2a-task");
        Ok(json!({
            "message": "handled",
            "decision": "allow"
        }))
    })
    .await
    .expect("mediated sse run");

    connect.assert_async().await;
    submit.assert_async().await;
}

#[tokio::test]
async fn mediated_ws_rejects_untrusted_card_when_policy_enforced() {
    if !can_bind_localhost() {
        eprintln!("skipping mediated_ws_rejects_untrusted_card_when_policy_enforced: localhost bind unavailable");
        return;
    }

    let addr = start_ws_server(vec![json!({
        "type": "benchmark_job",
        "job_id": "job-9",
        "remoteAgentCard": {
            "name": "remote-agent",
            "metadata": {"jacsId": "unknown-agent"},
            "capabilities": {}
        }
    })])
    .await;

    let client = make_client(&format!("http://{addr}"));
    let a2a = client.get_a2a(Some(A2ATrustPolicy::Strict));
    let handler_called = Arc::new(AtomicBool::new(false));
    let handler_called_inner = Arc::clone(&handler_called);

    let err = a2a
        .on_mediated_benchmark_job(
            A2AMediatedJobOptions {
                transport: TransportType::Ws,
                enforce_trust_policy: true,
                ..A2AMediatedJobOptions::default()
            },
            move |_task| {
                handler_called_inner.store(true, Ordering::SeqCst);
                async move { Ok(json!({"message":"unreachable"})) }
            },
        )
        .await
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

    let addr = start_ws_server(vec![json!({
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
    })])
    .await;

    let client = make_client(&format!("http://{addr}"));
    let a2a = client.get_a2a(Some(A2ATrustPolicy::Verified));

    let err = a2a
        .on_mediated_benchmark_job(
            A2AMediatedJobOptions {
                transport: TransportType::Ws,
                verify_inbound_artifact: true,
                ..A2AMediatedJobOptions::default()
            },
            |_task| async move { Ok(json!({"message":"unreachable"})) },
        )
        .await
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

    let (base_url, connect_calls, submit_calls, shutdown_tx) = start_flaky_sse_server().await;
    let client = make_client(&base_url);
    let a2a = client.get_a2a(Some(A2ATrustPolicy::Verified));

    a2a.on_mediated_benchmark_job(
        A2AMediatedJobOptions {
            transport: TransportType::Sse,
            max_reconnect_attempts: 1,
            ..A2AMediatedJobOptions::default()
        },
        |_task| async move { Ok(json!({"message":"recovered"})) },
    )
    .await
    .expect("mediated reconnect");

    assert!(
        connect_calls.load(Ordering::SeqCst) >= 2,
        "expected at least 2 connect attempts"
    );
    assert_eq!(submit_calls.load(Ordering::SeqCst), 1);
    let _ = shutdown_tx.send(());
}
