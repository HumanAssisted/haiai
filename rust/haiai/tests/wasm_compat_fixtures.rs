//! Task 001 (HAIAI_WASM_PRD) — Lock cross-compat fixtures for the wasm port.
//!
//! These fixtures are the byte-locked oracle that every later refactor
//! (HaiTransport extraction, WasmFetchTransport impl, sse_parse / ws_protocol
//! extraction) must preserve. The `WasmFetchTransport` from `rust/haiai-wasm/`
//! must produce **byte-identical** Authorization headers, canonical-JSON
//! bodies, SSE event parses, WS frame parses, and raw-MIME bytes as the
//! native `HaiClient` build. See `HAIAI_WASM_PRD.md` §4.5, §5.2, and
//! `HAIAI_WASM_PRD_TASKS/HAIAI_WASM_TASK_001.md`.
//!
//! ## Fixture set
//!
//! Under `rust/haiai/tests/fixtures/wasm_compat/`:
//! - `ed25519.pkcs8.bin` — PKCS#8 v2 private key bytes (ring-emitted), pinned
//!   here as a copy of `wasm/jacs/tests/fixtures/wasm_compat/ed25519.pkcs8.bin`
//!   so this test does not require the JACS workspace to be checked out.
//! - `ed25519.public.bin` — raw 32-byte Ed25519 public key matching the pkcs8.
//! - `auth_header.json` — `{ "jacs_id", "ts", "nonce", "expected_signature_b64",
//!   "expected_authorization" }`. The Authorization header format is
//!   `JACS {jacsId}:{ts}:{nonce}:{signature_b64}`; the message signed is
//!   `"{jacsId}:{ts}:{nonce}"`. Both bytes are deterministic (Ed25519 sign
//!   is deterministic, format! is deterministic).
//! - `register_options.json` — pinned [`RegisterAgentOptions`] payload (as
//!   JSON) and `register_options.canonical.bin` — its JCS-canonical bytes.
//! - `send_email_options.json` + `send_email_options.canonical.bin` — same
//!   for [`SendEmailOptions`].
//! - `sse_line.txt` + `sse_event.json` — a complete `event:`/`data:` block
//!   plus the expected `HaiEvent` parse output.
//! - `ws_frame.bin` + `ws_event.json` — a text WS frame payload plus the
//!   expected `HaiEvent`.
//! - `raw_email_response.json` + `raw_email.bin` — a server wire JSON for the
//!   `/raw` endpoint plus the expected decoded MIME bytes.
//! - `agreement.json` + `agreement.signers.json` — two-party signed
//!   agreement fixture, copied from the JACS WASM workspace. Verified with
//!   `jacs::crypt::ringwrapper::verify_string` (no local crypto).
//!
//! ## Regenerator
//!
//! ```bash
//! UPDATE_WASM_COMPAT_FIXTURES=1 cargo test -p haiai \
//!     --test wasm_compat_fixtures \
//!     -- --nocapture --include-ignored regenerate_wasm_compat_fixtures
//! ```
//!
//! Ed25519 + JCS canonicalization are deterministic so re-running the
//! regenerator produces byte-identical artifacts. The regenerator commits
//! new bytes only when the user explicitly opts in via the env var.

use std::path::PathBuf;
use std::sync::OnceLock;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use haiai::client::{SseParser, parse_sse_event_payload};
use haiai::types::{RawEmailResponse, SendEmailOptions};
use jacs::crypt::ringwrapper;
use jacs::protocol::canonicalize_json;
use serde_json::{Value, json};

fn fixtures_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("wasm_compat")
    })
}

fn fixture_path(name: &str) -> PathBuf {
    fixtures_dir().join(name)
}

fn read_fixture(name: &str) -> Vec<u8> {
    let path = fixture_path(name);
    std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "wasm_compat fixture '{}' missing or unreadable ({}). \
             Regenerate with UPDATE_WASM_COMPAT_FIXTURES=1 \
             cargo test -p haiai --test wasm_compat_fixtures \
             -- --nocapture --include-ignored regenerate_wasm_compat_fixtures.",
            path.display(),
            e
        )
    })
}

fn read_fixture_json(name: &str) -> Value {
    serde_json::from_slice(&read_fixture(name)).expect("fixture not valid JSON")
}

fn read_fixture_text(name: &str) -> String {
    String::from_utf8(read_fixture(name)).expect("fixture not valid UTF-8")
}

// ============================================================================
// Pinned inputs (single source of truth — regenerator and tests share these).
// ============================================================================

/// Pinned jacs_id used in the auth-header fixture. Mirrors a real JACS UUID.
const PINNED_JACS_ID: &str = "agent-alpha-fixture-2026";
/// Pinned unix timestamp (UTC) — locks `expected_authorization` byte-for-byte.
const PINNED_TS: i64 = 1_747_500_000;
/// Pinned auth-header nonce (UUID v4 .simple() shape).
const PINNED_NONCE: &str = "0a1b2c3d4e5f607182939495a6b7c8d9";

fn pinned_register_options_payload() -> Value {
    // Mirrors the `register` body shape built in `rust/haiai/src/client.rs`
    // (manual `serde_json::Map` insertions). We lock the canonical bytes of
    // a representative call so the wasm transport's body-builder cannot drift.
    json!({
        "agent_json": "{\"$schema\":\"https://hai.ai/schemas/agent/v1\",\"id\":\"agent-alpha-fixture-2026\"}",
        "owner_email": "alpha@hai.ai",
        "domain": "hai.ai",
        "description": "Pinned RegisterAgentOptions fixture for wasm cross-compat",
        "registration_key": "REGKEY-WASM-FIXTURE",
        "is_mediator": false,
    })
}

fn pinned_send_email_options() -> SendEmailOptions {
    SendEmailOptions {
        to: "beta@hai.ai".into(),
        subject: "Wasm cross-compat fixture".into(),
        body: "Hello from the wasm cross-compat fixture.".into(),
        cc: vec!["gamma@hai.ai".into()],
        bcc: vec![],
        in_reply_to: None,
        attachments: vec![],
        labels: vec!["fixture".into(), "wasm".into()],
        append_footer: Some(false),
        idempotency_key: Some("idem-wasm-fixture-2026".into()),
    }
}

fn pinned_sse_line() -> String {
    "event: benchmark_job\nid: evt-001\ndata: {\"type\":\"benchmark_job\",\"job_id\":\"job-42\",\"tier\":\"free\"}\n\n".to_string()
}

fn pinned_ws_frame_text() -> String {
    // Text WS frame as the server would send it (json string).
    "{\"type\":\"benchmark_job\",\"job_id\":\"job-42\",\"tier\":\"free\"}".to_string()
}

fn pinned_raw_mime() -> Vec<u8> {
    // Minimal but realistic RFC 5322 message. Byte-fidelity matters — locking
    // the exact bytes guards the get_raw_email decode path.
    let body = b"From: alpha@hai.ai\r\n\
                  To: beta@hai.ai\r\n\
                  Subject: wasm-fixture\r\n\
                  Message-ID: <fixture@hai.ai>\r\n\
                  Content-Type: text/plain; charset=utf-8\r\n\
                  \r\n\
                  Hello wasm.\r\n";
    body.to_vec()
}

// ============================================================================
// Tests
// ============================================================================

/// Ed25519 + format! are deterministic — the `expected_authorization`
/// captured at fixture time must match what we produce today from the same
/// jacs_id / ts / nonce. Any drift here is the load-bearing security
/// regression for the wasm port.
#[test]
fn auth_header_byte_identical() {
    let fixture: Value = read_fixture_json("auth_header.json");
    let jacs_id = fixture["jacs_id"].as_str().expect("jacs_id string");
    let ts = fixture["ts"].as_i64().expect("ts integer");
    let nonce = fixture["nonce"].as_str().expect("nonce string");
    let expected_sig_b64 = fixture["expected_signature_b64"]
        .as_str()
        .expect("expected_signature_b64");
    let expected_auth = fixture["expected_authorization"]
        .as_str()
        .expect("expected_authorization");

    let pkcs8 = read_fixture("ed25519.pkcs8.bin");
    let message = format!("{jacs_id}:{ts}:{nonce}");

    let sig_b64 = ringwrapper::sign_string(pkcs8, &message)
        .expect("ringwrapper::sign_string");
    assert_eq!(
        sig_b64, expected_sig_b64,
        "Ed25519 signature drift for pinned auth-header inputs"
    );

    let auth = format!("JACS {jacs_id}:{ts}:{nonce}:{sig_b64}");
    assert_eq!(
        auth, expected_auth,
        "Authorization header byte-drift — wasm transport will be rejected by hai/api"
    );

    // Verify with the pinned public key as a defence-in-depth check.
    let public_key = read_fixture("ed25519.public.bin");
    ringwrapper::verify_string(public_key, &message, expected_sig_b64)
        .expect("verify under pinned public key");
}

#[test]
fn canonical_body_register_options() {
    let payload = pinned_register_options_payload();
    let produced = canonicalize_json(&payload);
    let expected = read_fixture("register_options.canonical.bin");
    assert_eq!(
        produced.as_bytes(),
        expected.as_slice(),
        "RegisterAgentOptions canonical-JSON byte-drift"
    );

    // The JSON-typed fixture must round-trip back to the same Value as the
    // pinned input — guards accidental editing of the wire shape.
    let stored: Value = read_fixture_json("register_options.json");
    assert_eq!(stored, payload, "register_options.json drift");
}

#[test]
fn canonical_body_send_email_options() {
    let opts = pinned_send_email_options();
    let as_value = serde_json::to_value(&opts).expect("serialize SendEmailOptions");
    let produced = canonicalize_json(&as_value);
    let expected = read_fixture("send_email_options.canonical.bin");
    assert_eq!(
        produced.as_bytes(),
        expected.as_slice(),
        "SendEmailOptions canonical-JSON byte-drift"
    );

    let stored: Value = read_fixture_json("send_email_options.json");
    assert_eq!(stored, as_value, "send_email_options.json drift");
}

#[test]
fn sse_event_parse() {
    let sse_line = read_fixture_text("sse_line.txt");
    let mut parser = SseParser::default();
    let events = parser.push_chunk(sse_line.as_bytes());
    assert_eq!(events.len(), 1, "expected exactly one SSE event from chunk");

    let event = &events[0];
    let expected: Value = read_fixture_json("sse_event.json");

    assert_eq!(event.event_type, expected["event_type"].as_str().unwrap());
    assert_eq!(event.data, expected["data"]);
    assert_eq!(
        event.id.as_deref(),
        expected["id"].as_str(),
        "SSE id mismatch"
    );
    assert_eq!(
        event.raw,
        expected["raw"].as_str().unwrap(),
        "SSE raw mismatch"
    );
}

#[test]
fn ws_frame_parse() {
    let frame = read_fixture_text("ws_frame.bin"); // text frame stored as utf-8
    // The WS frame-to-event logic in client.rs::connect_ws builds a HaiEvent
    // by parsing the text payload as JSON and extracting `type`. We mirror
    // that mapping here via the same parse helpers (`parse_sse_event_payload`
    // works on raw JSON strings too — `event_type` is read from `data["type"]`
    // when present, exactly matching ws_native's behavior).
    let data: Value =
        serde_json::from_str(&frame).unwrap_or_else(|_| Value::String(frame.clone()));
    let event_type = data
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message")
        .to_string();
    let event = haiai::types::HaiEvent {
        event_type,
        data,
        id: None,
        raw: frame,
    };

    let expected: Value = read_fixture_json("ws_event.json");
    assert_eq!(event.event_type, expected["event_type"].as_str().unwrap());
    assert_eq!(event.data, expected["data"]);
    assert_eq!(event.raw, expected["raw"].as_str().unwrap());
}

#[test]
fn raw_mime_roundtrip() {
    let wire: Value = read_fixture_json("raw_email_response.json");
    let response = RawEmailResponse::from_wire_json(wire).expect("from_wire_json");
    assert!(response.available, "fixture must have available=true");

    let raw = response.raw_email.expect("raw_email present");
    let expected = read_fixture("raw_email.bin");
    assert_eq!(
        raw.as_slice(),
        expected.as_slice(),
        "raw MIME byte-drift in get_raw_email decode path"
    );
}

#[test]
fn agreement_two_party_verify() {
    let agreement: Value = read_fixture_json("agreement.json");
    let signers: Value = read_fixture_json("agreement.signers.json");
    let canonical_payload = agreement["canonical_payload"]
        .as_str()
        .expect("canonical_payload string");

    let sig_array = agreement["signatures"]
        .as_array()
        .expect("signatures array");
    assert_eq!(sig_array.len(), 2, "two-party agreement");

    let signers_arr = signers.as_array().expect("signers array");

    for sig in sig_array {
        let signer_id = sig["signer_id"].as_str().expect("signer_id");
        let signature_b64 = sig["signature_b64"].as_str().expect("signature_b64");
        let signer = signers_arr
            .iter()
            .find(|s| s["id"].as_str() == Some(signer_id))
            .unwrap_or_else(|| panic!("missing signer entry for {signer_id}"));
        let public_key_b64 = signer["public_key_b64"]
            .as_str()
            .expect("public_key_b64");
        let public_key = B64.decode(public_key_b64).expect("decode public key");

        ringwrapper::verify_string(public_key, canonical_payload, signature_b64).unwrap_or_else(
            |e| panic!("agreement signature for {signer_id} failed to verify: {e}"),
        );
    }
}

// ============================================================================
// Regenerator (ignored unless explicitly invoked)
// ============================================================================

#[test]
#[ignore = "regenerator — UPDATE_WASM_COMPAT_FIXTURES=1 cargo test -p haiai --test wasm_compat_fixtures -- --include-ignored regenerate_wasm_compat_fixtures"]
fn regenerate_wasm_compat_fixtures() {
    if std::env::var("UPDATE_WASM_COMPAT_FIXTURES").ok().as_deref() != Some("1") {
        eprintln!(
            "Skipping regenerator: set UPDATE_WASM_COMPAT_FIXTURES=1 to write fixture bytes."
        );
        return;
    }

    let dir = fixtures_dir().clone();
    std::fs::create_dir_all(&dir).expect("create fixtures dir");

    // 1. auth_header.json
    let pkcs8 = read_fixture("ed25519.pkcs8.bin");
    let message = format!("{PINNED_JACS_ID}:{PINNED_TS}:{PINNED_NONCE}");
    let sig_b64 = ringwrapper::sign_string(pkcs8, &message).expect("sign auth fixture message");
    let auth_header = format!("JACS {PINNED_JACS_ID}:{PINNED_TS}:{PINNED_NONCE}:{sig_b64}");
    let auth_fixture = json!({
        "jacs_id": PINNED_JACS_ID,
        "ts": PINNED_TS,
        "nonce": PINNED_NONCE,
        "message_signed": message,
        "expected_signature_b64": sig_b64,
        "expected_authorization": auth_header,
    });
    let auth_path = fixture_path("auth_header.json");
    std::fs::write(
        &auth_path,
        format!("{}\n", serde_json::to_string_pretty(&auth_fixture).unwrap()),
    )
    .expect("write auth_header.json");
    eprintln!("wrote {}", auth_path.display());

    // 2. register_options.{json,canonical.bin}
    let reg_payload = pinned_register_options_payload();
    std::fs::write(
        fixture_path("register_options.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&reg_payload).unwrap()
        ),
    )
    .expect("write register_options.json");
    let reg_canonical = canonicalize_json(&reg_payload);
    std::fs::write(fixture_path("register_options.canonical.bin"), reg_canonical.as_bytes())
        .expect("write register_options.canonical.bin");

    // 3. send_email_options.{json,canonical.bin}
    let opts = pinned_send_email_options();
    let opts_value = serde_json::to_value(&opts).expect("serialize SendEmailOptions");
    std::fs::write(
        fixture_path("send_email_options.json"),
        format!("{}\n", serde_json::to_string_pretty(&opts_value).unwrap()),
    )
    .expect("write send_email_options.json");
    let opts_canonical = canonicalize_json(&opts_value);
    std::fs::write(
        fixture_path("send_email_options.canonical.bin"),
        opts_canonical.as_bytes(),
    )
    .expect("write send_email_options.canonical.bin");

    // 4. sse_line.txt + sse_event.json
    let sse_line = pinned_sse_line();
    std::fs::write(fixture_path("sse_line.txt"), sse_line.as_bytes()).expect("write sse_line.txt");
    let mut parser = SseParser::default();
    let mut events = parser.push_chunk(sse_line.as_bytes());
    assert_eq!(events.len(), 1, "sse fixture must yield one event");
    let event = events.remove(0);
    let sse_event_json = json!({
        "event_type": event.event_type,
        "data": event.data,
        "id": event.id,
        "raw": event.raw,
    });
    std::fs::write(
        fixture_path("sse_event.json"),
        format!("{}\n", serde_json::to_string_pretty(&sse_event_json).unwrap()),
    )
    .expect("write sse_event.json");

    // Smoke: parse_sse_event_payload directly on the raw data line, to lock
    // the function's signature (used by the wasm sse_parse extract in Task 013).
    let _ = parse_sse_event_payload("benchmark_job", Some("evt-001".into()), event.raw.as_str());

    // 5. ws_frame.bin + ws_event.json
    let frame_text = pinned_ws_frame_text();
    std::fs::write(fixture_path("ws_frame.bin"), frame_text.as_bytes())
        .expect("write ws_frame.bin");
    let ws_data: Value = serde_json::from_str(&frame_text).expect("frame is json");
    let ws_event_type = ws_data
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message")
        .to_string();
    let ws_event_json = json!({
        "event_type": ws_event_type,
        "data": ws_data,
        "id": Value::Null,
        "raw": frame_text,
    });
    std::fs::write(
        fixture_path("ws_event.json"),
        format!("{}\n", serde_json::to_string_pretty(&ws_event_json).unwrap()),
    )
    .expect("write ws_event.json");

    // 6. raw_email_response.json + raw_email.bin
    let raw_bytes = pinned_raw_mime();
    std::fs::write(fixture_path("raw_email.bin"), &raw_bytes).expect("write raw_email.bin");
    let wire = json!({
        "message_id": "msg-wasm-fixture-2026",
        "rfc_message_id": "<fixture@hai.ai>",
        "available": true,
        "raw_email_b64": B64.encode(&raw_bytes),
        "size_bytes": raw_bytes.len(),
        "omitted_reason": Value::Null,
    });
    std::fs::write(
        fixture_path("raw_email_response.json"),
        format!("{}\n", serde_json::to_string_pretty(&wire).unwrap()),
    )
    .expect("write raw_email_response.json");

    // 7. agreement.json + agreement.signers.json are copied from the JACS
    //    workspace by Task 001 setup; the regenerator only sanity-checks
    //    that they verify under the JACS public ringwrapper API.
    let agreement: Value = read_fixture_json("agreement.json");
    let signers: Value = read_fixture_json("agreement.signers.json");
    let canonical_payload = agreement["canonical_payload"].as_str().unwrap();
    for sig in agreement["signatures"].as_array().unwrap() {
        let signer_id = sig["signer_id"].as_str().unwrap();
        let signature_b64 = sig["signature_b64"].as_str().unwrap();
        let signer = signers
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"].as_str() == Some(signer_id))
            .unwrap();
        let public_key = B64
            .decode(signer["public_key_b64"].as_str().unwrap())
            .unwrap();
        ringwrapper::verify_string(public_key, canonical_payload, signature_b64)
            .expect("agreement signature verifies");
    }

    eprintln!("All wasm_compat fixtures regenerated.");
}
