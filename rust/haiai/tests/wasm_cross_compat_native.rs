//! HAIAI_WASM_PRD §5.2 Test A-D / Task 041 — native side of the
//! cross-compat parity test.
//!
//! Writes a JSON snapshot of the production code's output for the
//! pinned wasm-compat fixtures. The wasm side runs the SAME helpers
//! through the SAME inputs and produces a matching JSON; the CI step
//! `scripts/ci/check_wasm_parity.sh` (Task 041) diffs the two.
//!
//! Byte-for-byte parity is the load-bearing guarantee: any drift on
//! either side fails CI.
//!
//! Inputs (all from `tests/fixtures/wasm_compat/`):
//!   - auth_header.json — auth header builder
//!   - sse_line.txt + sse_event.json — SSE parser
//!   - ws_frame.bin + ws_event.json — WS parser
//!
//! Output (target/parity/native.json):
//!   {
//!     "auth_header": "<expected_authorization>",
//!     "sse_event": { ... },
//!     "ws_event": { ... }
//!   }

use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};

use haiai::sse_parse::SseParser;
use haiai::transport::build_auth_header_with;
use haiai::ws_protocol::parse_frame_text;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm_compat")
}

fn target_parity_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("haiai parent dir")
        .join("target/parity")
}

#[test]
fn cross_compat_native_writes_snapshot() {
    let fixtures = fixtures_dir();

    // ── Test A: Auth header byte-identity ────────────────────────────
    let auth_input: Value = serde_json::from_slice(
        &fs::read(fixtures.join("auth_header.json")).expect("read auth_header.json"),
    )
    .expect("auth_header.json parses");
    let jacs_id = auth_input["jacs_id"].as_str().expect("jacs_id field");
    let ts = auth_input["ts"].as_i64().expect("ts field");
    let nonce = auth_input["nonce"].as_str().expect("nonce field");
    let expected_sig = auth_input["expected_signature_b64"]
        .as_str()
        .expect("expected_signature_b64 field");
    let auth_header = build_auth_header_with(jacs_id, ts, nonce, |msg| {
        // The fixture signature is pre-computed; the test simulates a
        // signer that returns that fixed value for the canonical
        // `<jacs_id>:<ts>:<nonce>` message. Both targets sign the
        // same input via the same shared helper, so the headers match.
        assert_eq!(msg, &format!("{jacs_id}:{ts}:{nonce}"));
        Ok(expected_sig.to_string())
    })
    .expect("auth header builds");

    // ── Test B: SSE event parse ──────────────────────────────────────
    let sse_line = fs::read_to_string(fixtures.join("sse_line.txt")).expect("read sse_line.txt");
    let mut parser = SseParser::default();
    let events = parser.push_chunk(sse_line.as_bytes());
    assert_eq!(events.len(), 1, "sse_line yields exactly one event");
    let sse_event = serde_json::to_value(&events[0]).expect("serialize sse_event");

    // ── Test C: WS frame parse ───────────────────────────────────────
    let ws_frame = fs::read(fixtures.join("ws_frame.bin")).expect("read ws_frame.bin");
    let ws_frame_text = std::str::from_utf8(&ws_frame).expect("ws_frame is utf-8 text");
    let parsed = parse_frame_text(ws_frame_text);
    let ws_event = serde_json::to_value(&parsed.event).expect("serialize ws_event");

    // ── Emit snapshot ────────────────────────────────────────────────
    let snapshot = json!({
        "auth_header": auth_header,
        "sse_event": sse_event,
        "ws_event": ws_event,
    });

    let out_dir = target_parity_dir();
    fs::create_dir_all(&out_dir).expect("create target/parity");
    let out_path = out_dir.join("native.json");
    fs::write(&out_path, serde_json::to_string_pretty(&snapshot).unwrap())
        .expect("write native.json");
    eprintln!("cross_compat_native: wrote {}", out_path.display());

    // Self-check: cross-reference against the fixture's expected_authorization.
    assert_eq!(
        auth_header,
        auth_input["expected_authorization"]
            .as_str()
            .expect("expected_authorization field"),
        "native auth header diverged from the wasm-compat fixture",
    );

    // Self-check: cross-reference SSE event_type against the fixture
    // snapshot the wasm-compat tests use.
    let sse_event_expected: Value = serde_json::from_slice(
        &fs::read(fixtures.join("sse_event.json")).expect("read sse_event.json"),
    )
    .expect("sse_event.json parses");
    assert_eq!(
        sse_event["event_type"], sse_event_expected["event_type"],
        "native sse_event diverged"
    );

    // Self-check: cross-reference WS event shape.
    let ws_event_expected: Value = serde_json::from_slice(
        &fs::read(fixtures.join("ws_event.json")).expect("read ws_event.json"),
    )
    .expect("ws_event.json parses");
    assert_eq!(
        ws_event["event_type"], ws_event_expected["event_type"],
        "native ws_event diverged"
    );
}
