use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use futures_util::SinkExt;
use haiai::{HaiClient, HaiClientOptions, StaticJacsProvider, TransportType};
use httpmock::Method::GET;
use httpmock::MockServer;
use serde_json::json;
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

#[tokio::test]
async fn connect_sse_streams_connected_and_benchmark_events() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET).path("/api/v1/agents/connect");
            then.status(200)
                .header("Content-Type", "text/event-stream")
                .body(concat!(
                    "event: connected\n",
                    "data: {\"type\":\"connected\",\"agent_id\":\"a-1\"}\n\n",
                    "event: benchmark_job\n",
                    "data: {\"type\":\"benchmark_job\",\"job_id\":\"job-1\",\"scenario_id\":\"s-1\"}\n\n",
                ));
        })
        .await;

    let client = make_client(&server.base_url());
    let mut conn = client.connect_sse().await.expect("connect sse");

    let first = timeout(Duration::from_secs(2), conn.next_event())
        .await
        .expect("first event timeout")
        .expect("first event");
    assert_eq!(first.event_type, "connected");

    let second = timeout(Duration::from_secs(2), conn.next_event())
        .await
        .expect("second event timeout")
        .expect("second event");
    assert_eq!(second.event_type, "benchmark_job");
    assert_eq!(
        second.data.get("job_id").and_then(|v| v.as_str()),
        Some("job-1")
    );

    conn.close().await;
    mock.assert_async().await;
}

async fn start_ws_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut ws = accept_async(stream).await.expect("handshake");

        ws.send(Message::Text(
            json!({
                "type": "connected",
                "agent_id": "a-1"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send connected");

        ws.send(Message::Text(
            json!({
                "type": "benchmark_job",
                "job_id": "job-1",
                "scenario_id": "s-1"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("send benchmark");

        ws.send(Message::Close(None)).await.expect("send close");
    });

    addr
}

#[tokio::test]
async fn connect_ws_streams_connected_and_benchmark_events() {
    let addr = start_ws_server().await;
    let base_url = format!("http://{}", addr);
    let client = make_client(&base_url);

    let mut conn = client.connect_ws().await.expect("connect ws");

    let first = timeout(Duration::from_secs(2), conn.next_event())
        .await
        .expect("first event timeout")
        .expect("first event");
    assert_eq!(first.event_type, "connected");

    let second = timeout(Duration::from_secs(2), conn.next_event())
        .await
        .expect("second event timeout")
        .expect("second event");
    assert_eq!(second.event_type, "benchmark_job");
    assert_eq!(
        second.data.get("job_id").and_then(|v| v.as_str()),
        Some("job-1")
    );

    conn.close().await;
}

#[tokio::test]
async fn on_benchmark_job_dispatches_sse_benchmark_events() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(GET).path("/api/v1/agents/connect");
            then.status(200)
                .header("Content-Type", "text/event-stream")
                .body(concat!(
                    "event: benchmark_job\n",
                    "data: {\"type\":\"benchmark_job\",\"job_id\":\"job-42\"}\n\n",
                    "event: disconnect\n",
                    "data: {\"type\":\"disconnect\",\"reason\":\"done\"}\n\n",
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
    mock.assert_async().await;
}
