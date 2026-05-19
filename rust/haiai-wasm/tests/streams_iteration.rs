//! Issue 010 — wasm-pack stream-parser tests.
//!
//! Drives the shared SSE and WebSocket parsers (which the
//! `EventStreamHandle::next_event` loop calls under the hood) through
//! multi-event fixtures, asserting that the same code path that
//! `eventStream({ transport })` walks produces three back-to-back
//! events without losing state. Cannot stand up a real SSE / WS
//! server inside `wasm-pack test --headless --chrome`, but the
//! parser is the only platform-specific piece — the transport layer
//! is thin glue around `web_sys::ReadableStream` / `WebSocket`.
//!
//! Tests live in the wasm crate so they actually compile + run under
//! wasm32 rather than only as native parser tests.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

use haiai::sse_parse::SseParser;
use haiai::ws_protocol::parse_frame_text;

wasm_bindgen_test_configure!(run_in_browser);

/// Stream three SSE events into the parser; assert each is delivered in
/// order and the parser state remains valid for the next push.
/// Mirrors `for await (const ev of agent.client.connectSse())` with
/// three yields.
#[wasm_bindgen_test]
fn sse_parser_yields_three_events_in_order() {
    let mut parser = SseParser::default();
    let chunk = b"data: {\"event_type\":\"hello\",\"seq\":1}\n\n\
                  data: {\"event_type\":\"tick\",\"seq\":2}\n\n\
                  data: {\"event_type\":\"bye\",\"seq\":3}\n\n";
    let events = parser.push_chunk(chunk);
    assert_eq!(events.len(), 3, "expected three events from one chunk");
    let raw_payloads: Vec<String> = events
        .into_iter()
        .map(|e| serde_json::to_string(&e).expect("event to_string"))
        .collect();
    assert!(raw_payloads[0].contains("\"hello\""), "first event is hello: {}", raw_payloads[0]);
    assert!(raw_payloads[1].contains("\"tick\""), "second event is tick: {}", raw_payloads[1]);
    assert!(raw_payloads[2].contains("\"bye\""), "third event is bye: {}", raw_payloads[2]);
}

/// SSE parser handles chunk boundaries that split events — the wasm
/// transport delivers bytes from a ReadableStream in arbitrarily-sized
/// chunks, so the parser must stitch events that span chunk
/// boundaries. Issue 010's "three events" requirement is meaningless
/// if a packet split drops one.
#[wasm_bindgen_test]
fn sse_parser_stitches_events_across_chunk_boundaries() {
    let mut parser = SseParser::default();
    // Split the first event across two pushes.
    let _ = parser.push_chunk(b"data: {\"event_type\":\"split-");
    let mid_events = parser.push_chunk(b"start\"}\n\n");
    assert_eq!(mid_events.len(), 1, "boundary-spanning event delivered once");
    let s = serde_json::to_string(&mid_events[0]).expect("serialize");
    assert!(s.contains("split-start"), "stitched event has full payload: {s}");

    // Then a normal second + third event back-to-back.
    let tail = parser.push_chunk(
        b"data: {\"event_type\":\"two\"}\n\ndata: {\"event_type\":\"three\"}\n\n",
    );
    assert_eq!(tail.len(), 2, "two more events delivered after boundary");
    let strs: Vec<String> = tail
        .into_iter()
        .map(|e| serde_json::to_string(&e).expect("ser"))
        .collect();
    assert!(strs[0].contains("\"two\""));
    assert!(strs[1].contains("\"three\""));
}

/// WS frame parser handles three text frames in succession, including
/// a heartbeat that produces a pong reply (the EventStreamHandle WS
/// loop echoes pong frames before delivering the next event).
/// The wire shape is `{"type": "..."}` per
/// `haiai::ws_protocol::parse_frame_text`.
#[wasm_bindgen_test]
fn ws_frame_parser_handles_three_text_messages_including_heartbeat() {
    // Frame 1: ordinary event.
    let f1 = parse_frame_text(r#"{"type":"first","seq":1}"#);
    assert_eq!(f1.event.event_type, "first");
    assert!(f1.reply.is_none(), "non-heartbeat frame has no reply");

    // Frame 2: a heartbeat — the parser sets f2.reply to a pong frame.
    let f2 = parse_frame_text(r#"{"type":"heartbeat","timestamp":1700000000}"#);
    assert_eq!(f2.event.event_type, "heartbeat");
    assert!(
        f2.reply.is_some(),
        "heartbeat frame must produce a pong reply for upstream"
    );

    // Frame 3: another event.
    let f3 = parse_frame_text(r#"{"type":"third","seq":3}"#);
    assert_eq!(f3.event.event_type, "third");
    assert!(f3.reply.is_none());
}
