//! All four facades share this completion fixture; no live model is called.
use haiai::{HaiEvent, RegisterAgentOptions, SignedJobResponsePayloadV2};
use serde_json::{json, Value};

#[test]
fn benchmark_mediator_prompt_and_response_survive_native_types() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../fixtures/benchmark_mediator_contract.json"
    ))
    .unwrap();
    let registration: RegisterAgentOptions =
        serde_json::from_value(fixture["registration"].clone()).unwrap();
    assert_eq!(registration.is_mediator, Some(true));
    let event = HaiEvent {
        event_type: "benchmark_job".into(),
        data: fixture["event"].clone(),
        ..Default::default()
    };
    let received: HaiEvent = serde_json::from_value(serde_json::to_value(&event).unwrap()).unwrap();
    assert_eq!(received.data, fixture["event"]);
    let reply = SignedJobResponsePayloadV2::new(
        fixture["event"]["job_id"].as_str().unwrap(),
        fixture["response"].clone(),
    );
    let wire = serde_json::to_value(reply).unwrap();
    assert_eq!(wire["job_id"], fixture["event"]["job_id"]);
    assert_eq!(wire["response"], fixture["response"]);
    assert_eq!(
        fixture["ffi_submit_response"],
        json!({
            "job_id":fixture["event"]["job_id"], "message":wire["response"]["message"],
            "metadata":wire["response"]["metadata"], "processing_time_ms":wire["response"]["processing_time_ms"],
        })
    );
}
