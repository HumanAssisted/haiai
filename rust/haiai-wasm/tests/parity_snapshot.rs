//! HAIAI_WASM_PRD §5.2 Test A-E / Issue 007 — wasm side of the
//! cross-compat parity test.
//!
//! Mirrors `rust/haiai/tests/wasm_cross_compat_native.rs`: runs the
//! exact same fixtures through the exact same shared modules
//! (`haiai::transport::build_auth_header_with`, `haiai::sse_parse::
//! SseParser`, `haiai::ws_protocol::parse_frame_text`), then emits the
//! result as a JSON blob inside `console.log` between sentinel
//! markers. `scripts/ci/check_wasm_parity.sh` captures stdout from
//! `wasm-pack test --headless --chrome`, extracts the JSON, writes it
//! to `rust/target/parity/wasm.json`, and diffs against the native
//! snapshot.
//!
//! The fixtures live in `rust/haiai/tests/fixtures/wasm_compat/` and
//! are pulled into the wasm binary via `include_bytes!` (the wasm
//! tests can't read the host filesystem). The fixture path is relative
//! to THIS file (`rust/haiai-wasm/tests/parity_snapshot.rs`) →
//! `../../haiai/tests/fixtures/wasm_compat/...`.

#![cfg(target_arch = "wasm32")]

use base64::Engine as _;
use serde_json::{json, Value};
use wasm_bindgen_test::*;

use haiai::sse_parse::SseParser;
use haiai::transport::build_auth_header_with;
use haiai::ws_protocol::parse_frame_text;

wasm_bindgen_test_configure!(run_in_browser);

const FIXTURE_AUTH_HEADER_JSON: &[u8] =
    include_bytes!("../../haiai/tests/fixtures/wasm_compat/auth_header.json");
const FIXTURE_SSE_LINE: &[u8] =
    include_bytes!("../../haiai/tests/fixtures/wasm_compat/sse_line.txt");
const FIXTURE_SSE_EVENT_JSON: &[u8] =
    include_bytes!("../../haiai/tests/fixtures/wasm_compat/sse_event.json");
const FIXTURE_WS_FRAME: &[u8] =
    include_bytes!("../../haiai/tests/fixtures/wasm_compat/ws_frame.bin");
const FIXTURE_WS_EVENT_JSON: &[u8] =
    include_bytes!("../../haiai/tests/fixtures/wasm_compat/ws_event.json");
const FIXTURE_REGISTER_CANONICAL: &[u8] =
    include_bytes!("../../haiai/tests/fixtures/wasm_compat/register_options.canonical.bin");
const FIXTURE_SEND_CANONICAL: &[u8] =
    include_bytes!("../../haiai/tests/fixtures/wasm_compat/send_email_options.canonical.bin");

const PARITY_BEGIN: &str = "__WASM_PARITY_JSON_BEGIN__";
const PARITY_END: &str = "__WASM_PARITY_JSON_END__";

#[wasm_bindgen_test]
fn cross_compat_wasm_emits_snapshot() {
    // ── Test A: Auth header byte-identity ────────────────────────────
    let auth_input: Value = serde_json::from_slice(FIXTURE_AUTH_HEADER_JSON)
        .expect("auth_header.json parses");
    let jacs_id = auth_input["jacs_id"].as_str().expect("jacs_id field");
    let ts = auth_input["ts"].as_i64().expect("ts field");
    let nonce = auth_input["nonce"].as_str().expect("nonce field");
    let expected_sig = auth_input["expected_signature_b64"]
        .as_str()
        .expect("expected_signature_b64 field");
    let auth_header = build_auth_header_with(jacs_id, ts, nonce, |msg| {
        assert_eq!(msg, &format!("{jacs_id}:{ts}:{nonce}"));
        Ok(expected_sig.to_string())
    })
    .expect("auth header builds");

    // ── Test B: SSE event parse ──────────────────────────────────────
    let mut parser = SseParser::default();
    let events = parser.push_chunk(FIXTURE_SSE_LINE);
    assert_eq!(events.len(), 1, "sse_line yields exactly one event");
    let sse_event = serde_json::to_value(&events[0]).expect("serialize sse_event");

    // ── Test C: WS frame parse ───────────────────────────────────────
    let ws_frame_text = std::str::from_utf8(FIXTURE_WS_FRAME).expect("ws_frame is utf-8 text");
    let parsed = parse_frame_text(ws_frame_text);
    let ws_event = serde_json::to_value(&parsed.event).expect("serialize ws_event");

    // ── Test D + E: canonical bodies ─────────────────────────────────
    let register_canonical_b64 =
        base64::engine::general_purpose::STANDARD.encode(FIXTURE_REGISTER_CANONICAL);
    let send_canonical_b64 =
        base64::engine::general_purpose::STANDARD.encode(FIXTURE_SEND_CANONICAL);

    // Cross-reference against the static fixtures (mirrors the native
    // self-checks). Any drift here fails the wasm test before the
    // snapshot is even emitted.
    let sse_event_expected: Value = serde_json::from_slice(FIXTURE_SSE_EVENT_JSON)
        .expect("sse_event.json parses");
    assert_eq!(
        sse_event["event_type"], sse_event_expected["event_type"],
        "wasm sse_event diverged"
    );
    let ws_event_expected: Value = serde_json::from_slice(FIXTURE_WS_EVENT_JSON)
        .expect("ws_event.json parses");
    assert_eq!(
        ws_event["event_type"], ws_event_expected["event_type"],
        "wasm ws_event diverged"
    );
    assert_eq!(
        auth_header,
        auth_input["expected_authorization"]
            .as_str()
            .expect("expected_authorization field"),
        "wasm auth header diverged from the fixture"
    );

    let snapshot = json!({
        "auth_header": auth_header,
        "register_canonical_b64": register_canonical_b64,
        "send_canonical_b64": send_canonical_b64,
        "sse_event": sse_event,
        "ws_event": ws_event,
    });
    // Emit the snapshot between sentinel markers. The CI script parses
    // stdout for the markers, extracts the JSON, and writes it to
    // rust/target/parity/wasm.json. Keep this on one physical line so
    // the shell extractor can match it without multiline state.
    let compact = serde_json::to_string(&snapshot).expect("serialize snapshot");
    // wasm_bindgen_test routes Rust stdout to the console; printing the
    // marker block on one line keeps the regex in check_wasm_parity.sh
    // simple (no newlines between markers).
    println!("{PARITY_BEGIN}{compact}{PARITY_END}");
}
