// Copyright (c) 2026 Human Assisted Intelligence, Inc.
// SPDX-License-Identifier: BUSL-1.1

//! Target-agnostic WebSocket protocol layer shared between the native
//! `tokio_tungstenite` transport (`ws_native.rs` / inline in `client.rs`)
//! and the wasm `web_sys::WebSocket` transport (`haiai-wasm`).
//!
//! HAIAI_WASM_PRD §4.6 + Task 014: lifts the frame-to-`HaiEvent` parser,
//! the heartbeat / pong pairing, and the reconnect backoff constants out
//! of `client.rs::connect_ws` so both the native and browser impls share
//! exactly one implementation.
//!
//! ## What ships here
//!
//! 1. [`WsMessage`] — neutral message enum produced by either transport.
//! 2. [`WebSocketTransport`] — trait both the native and wasm impls
//!    fulfil. `?Send` on wasm32 (browser futures are not `Send`).
//! 3. [`parse_frame_text`] — JSON-text-to-[`HaiEvent`] parser used by
//!    every consumer. Also returns the optional pong reply that the
//!    consumer should send back upstream for `heartbeat` frames.
//! 4. Reconnect backoff constants ([`WS_RECONNECT_INITIAL_MS`],
//!    [`WS_RECONNECT_MAX_MS`]) shared by both impls + the JS
//!    `EventStreamHandle` reconnect path in haiai-wasm.

use async_trait::async_trait;
use serde_json::{json, Value};
use time::OffsetDateTime;

use crate::error::Result;
use crate::types::HaiEvent;

// ── Reconnect backoff constants ──────────────────────────────────────────
//
// Shared between native (`client.rs::on_benchmark_job_with_reconnect`) and
// wasm (`haiai-wasm::EventStreamHandle`) so the operator-observable
// reconnect cadence is identical across targets. Numeric values match the
// pre-Task-014 native defaults exactly so this commit ships zero behavior
// change for the native path.

/// First reconnect delay, milliseconds. Doubles every consecutive failure
/// up to [`WS_RECONNECT_MAX_MS`].
pub const WS_RECONNECT_INITIAL_MS: u64 = 1_000;

/// Reconnect backoff cap, milliseconds.
pub const WS_RECONNECT_MAX_MS: u64 = 30_000;

// ── Neutral message envelope ─────────────────────────────────────────────

/// Neutral message produced by [`WebSocketTransport`] implementations.
///
/// Both `tokio_tungstenite::Message` and `web_sys::MessageEvent` collapse
/// into this enum so the consumer (`client.rs::connect_ws` /
/// `haiai-wasm::EventStreamHandle`) sees the same shape across targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsMessage {
    Text(String),
    Binary(Vec<u8>),
    Close,
}

// ── Trait surface ────────────────────────────────────────────────────────

/// WebSocket transport trait. Native uses `tokio_tungstenite`, wasm
/// uses `web_sys::WebSocket` — both produce the same [`WsMessage`]
/// stream and accept the same outgoing frames.
///
/// On wasm32 the trait is `?Send` because browser futures are not
/// `Send`-bounded (single-threaded event loop). On native it is
/// `Send + 'static` so the consumer can park the connection inside a
/// `tokio::spawn`-ed task.
#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
pub trait WebSocketTransport: Send + 'static {
    /// Block until the next message arrives, the connection closes, or
    /// the underlying transport errors. Returns `None` on close/error.
    async fn next_message(&mut self) -> Option<WsMessage>;

    /// Send a message upstream. Returns the transport error on failure.
    async fn send_message(&mut self, msg: WsMessage) -> Result<()>;

    /// Close the connection. After calling this, subsequent
    /// `next_message` calls MUST return `None`.
    async fn close(&mut self) -> Result<()>;
}

#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
pub trait WebSocketTransport: 'static {
    async fn next_message(&mut self) -> Option<WsMessage>;
    async fn send_message(&mut self, msg: WsMessage) -> Result<()>;
    async fn close(&mut self) -> Result<()>;
}

// ── Frame parsing ────────────────────────────────────────────────────────

/// Outcome of parsing a single text frame.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedFrame {
    /// `HaiEvent` to deliver to the consumer.
    pub event: HaiEvent,
    /// Optional reply frame the transport should send upstream. Today
    /// this is non-empty only for `heartbeat` frames (the server expects
    /// a `pong` to keep the connection alive — HAIAI_WASM_PRD §4.6).
    pub reply: Option<WsMessage>,
}

/// Parse a JSON text frame into a [`HaiEvent`].
///
/// The frame body is parsed as JSON; non-JSON frames fall back to a
/// `HaiEvent { event_type: "message", data: String(raw) }`. `heartbeat`
/// frames produce a `pong` reply with the heartbeat's timestamp echoed
/// back (or `now()` if the heartbeat omitted one).
pub fn parse_frame_text(raw: &str) -> ParsedFrame {
    let data: Value = serde_json::from_str::<Value>(raw)
        .unwrap_or_else(|_| Value::String(raw.to_string()));

    let event_type = data
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message")
        .to_string();

    let reply = if event_type == "heartbeat" {
        let timestamp = data
            .get("timestamp")
            .cloned()
            .unwrap_or_else(|| Value::from(OffsetDateTime::now_utc().unix_timestamp()));
        let pong = json!({
            "type": "pong",
            "timestamp": timestamp,
        });
        Some(WsMessage::Text(pong.to_string()))
    } else {
        None
    };

    let event = HaiEvent {
        event_type,
        data,
        id: None,
        raw: raw.to_string(),
    };

    ParsedFrame { event, reply }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_frame_text_matches_fixture() {
        // Mirror tests/fixtures/wasm_compat/{ws_frame.bin, ws_event.json}.
        // The frame is plain JSON; the parser MUST produce the same event
        // shape across native and wasm so the cross-compat parity test
        // (Task 041) sees byte-identical outputs.
        let raw = r#"{"type":"benchmark_job","job_id":"job-42","tier":"free"}"#;
        let parsed = parse_frame_text(raw);
        assert_eq!(parsed.event.event_type, "benchmark_job");
        assert_eq!(parsed.event.raw, raw);
        assert!(parsed.reply.is_none(), "non-heartbeat has no reply");
        assert_eq!(parsed.event.data["job_id"], "job-42");
        assert_eq!(parsed.event.data["tier"], "free");
    }

    #[test]
    fn parse_frame_text_heartbeat_returns_pong_reply() {
        let raw = r#"{"type":"heartbeat","timestamp":1234567890}"#;
        let parsed = parse_frame_text(raw);
        assert_eq!(parsed.event.event_type, "heartbeat");
        match parsed.reply {
            Some(WsMessage::Text(pong)) => {
                let v: Value = serde_json::from_str(&pong).expect("pong parses");
                assert_eq!(v["type"], "pong");
                assert_eq!(v["timestamp"], 1234567890);
            }
            other => panic!("expected pong text reply, got {other:?}"),
        }
    }

    #[test]
    fn parse_frame_text_non_json_falls_back_to_message_event() {
        let raw = "hello world";
        let parsed = parse_frame_text(raw);
        assert_eq!(parsed.event.event_type, "message");
        assert_eq!(parsed.event.data, Value::String("hello world".to_string()));
        assert!(parsed.reply.is_none());
    }
}
