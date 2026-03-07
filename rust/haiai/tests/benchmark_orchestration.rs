use std::time::Duration;

use haiai::{
    DnsCertifiedRunOptions, HaiClient, HaiClientOptions, StaticJacsProvider, TransportType,
};
use httpmock::Method::{GET, POST};
use httpmock::MockServer;
use serde_json::json;

fn make_client(base_url: &str) -> HaiClient<StaticJacsProvider> {
    let provider = StaticJacsProvider::new("agent123456");
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
async fn free_run_posts_expected_payload_and_parses_result() {
    let server = MockServer::start_async().await;

    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/benchmark/run")
                .json_body(json!({
                    "name": "Free Run - agent123",
                    "tier": "free",
                    "transport": "sse"
                }));
            then.status(200).json_body(json!({
                "run_id": "run-free-1",
                "transcript": [
                    {
                        "role": "party_a",
                        "content": "hello",
                        "timestamp": "2026-01-01T00:00:00Z",
                        "annotations": ["start"]
                    }
                ],
                "upsell_message": "upgrade"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let result = client
        .free_run(Some(TransportType::Sse))
        .await
        .expect("free run");

    assert!(result.success);
    assert_eq!(result.run_id, "run-free-1");
    assert_eq!(result.transcript.len(), 1);
    assert_eq!(result.transcript[0].role, "party_a");
    assert_eq!(result.upsell_message, "upgrade");

    mock.assert_async().await;
}

#[tokio::test]
async fn dns_certified_run_polls_payment_and_runs_benchmark() {
    let server = MockServer::start_async().await;

    let purchase = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/benchmark/purchase")
                .json_body(json!({
                    "tier": "dns_certified",
                    "agent_id": "agent123456"
                }));
            then.status(200).json_body(json!({
                "checkout_url": "https://pay.example/checkout",
                "payment_id": "pay/123"
            }));
        })
        .await;

    let status = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/api/benchmark/payments/pay%2F123/status");
            then.status(200).json_body(json!({
                "status": "paid"
            }));
        })
        .await;

    let run = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/api/benchmark/run")
                .json_body(json!({
                    "name": "DNS Certified Run - agent123",
                    "tier": "dns_certified",
                    "payment_id": "pay/123",
                    "transport": "ws"
                }));
            then.status(200).json_body(json!({
                "run_id": "run-dns-1",
                "score": 93.5,
                "transcript": [],
                "payment_id": "pay/123"
            }));
        })
        .await;

    let client = make_client(&server.base_url());
    let result = client
        .dns_certified_run(&DnsCertifiedRunOptions {
            transport: TransportType::Ws,
            poll_interval: Duration::from_millis(1),
            poll_timeout: Duration::from_secs(1),
        })
        .await
        .expect("dns certified run");

    assert!(result.success);
    assert_eq!(result.run_id, "run-dns-1");
    assert_eq!(result.score, 93.5);
    assert_eq!(result.payment_id, "pay/123");

    purchase.assert_async().await;
    status.assert_async().await;
    run.assert_async().await;
}
