//! Shared SSE line / event parser, target-agnostic.
//!
//! HAIAI_WASM_PRD §4.6: SSE parser logic must be SHARED between native and
//! wasm transports. The native consumer (`HaiClient::connect_sse`) and the
//! browser consumer (`WasmSseConnection` in `rust/haiai-wasm/`, Task 019)
//! both push bytes through this parser. No `tokio::*`, `std::fs::*`, or
//! native-only crate dep — pure logic + `HaiEvent` / `serde_json`.
//!
//! Byte-locked by the wasm-compat fixtures
//! `rust/haiai/tests/fixtures/wasm_compat/sse_line.txt` +
//! `sse_event.json` (Task 001). Any change here must round-trip the
//! fixture in `rust/haiai/tests/wasm_compat_fixtures.rs::sse_event_parse`.

use serde_json::Value;

use crate::types::HaiEvent;

/// Streaming SSE parser. Stateful — accumulates bytes across chunks
/// (TCP segment boundaries don't align to event boundaries on the wire)
/// and emits parsed [`HaiEvent`]s as full event records complete.
///
/// Wire format follows the `text/event-stream` spec:
///   - `event: foo\n` sets the event type for the next event
///   - `id: 123\n`    sets the event id
///   - `data: blob\n` appends to the data buffer; multi-line `data:`
///     lines join with `\n`
///   - blank line     terminates the event; the accumulated state is
///     flushed into a `HaiEvent` via [`parse_sse_event_payload`].
///   - lines starting with `:` (comments / keepalives) are ignored.
#[derive(Default)]
pub struct SseParser {
    buffer: Vec<u8>,
    event_type: String,
    event_id: Option<String>,
    data_lines: Vec<String>,
}

impl SseParser {
    /// Push a chunk of SSE bytes into the parser and return any events
    /// that completed on this chunk.
    pub fn push_chunk(&mut self, chunk: &[u8]) -> Vec<HaiEvent> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();

        while let Some(idx) = self.buffer.iter().position(|b| *b == b'\n') {
            let mut line_bytes = self.buffer.drain(..=idx).collect::<Vec<_>>();
            line_bytes.pop(); // strip the \n
            if line_bytes.ends_with(b"\r") {
                line_bytes.pop();
            }

            let Ok(line) = String::from_utf8(line_bytes) else {
                continue;
            };

            if line.is_empty() {
                if !self.data_lines.is_empty() {
                    let raw = self.data_lines.join("\n");
                    events.push(parse_sse_event_payload(
                        &self.event_type,
                        self.event_id.clone(),
                        &raw,
                    ));
                }
                self.event_type.clear();
                self.event_id = None;
                self.data_lines.clear();
                continue;
            }

            if let Some(rest) = line.strip_prefix("event:") {
                self.event_type = rest.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("id:") {
                self.event_id = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("data:") {
                self.data_lines.push(rest.trim().to_string());
            }
            // Comments (`:` prefix) and unknown fields are intentionally ignored.
        }

        events
    }
}

/// Parse a single complete SSE event payload (after a blank-line
/// terminator) into a [`HaiEvent`]. Public so the wasm-compat fixture
/// test can call it with a synthesized event line.
///
/// The `data` field is preferred when it parses as JSON — that's how the
/// HAI backend emits typed events (`{"type":"benchmark_job", ...}`). If
/// the payload is not JSON we fall back to wrapping the raw string in a
/// `Value::String`. The event type defaults to the literal `event:` line
/// value, with the JSON `type` field overriding when present.
pub fn parse_sse_event_payload(event_type: &str, id: Option<String>, raw: &str) -> HaiEvent {
    let data =
        serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.to_string()));
    let inferred = data
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or(event_type)
        .to_string();
    HaiEvent {
        event_type: if inferred.is_empty() {
            "message".to_string()
        } else {
            inferred
        },
        data,
        id,
        raw: raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_emits_event_after_blank_line() {
        let mut parser = SseParser::default();
        let events = parser.push_chunk(b"event: benchmark_job\ndata: {\"type\":\"benchmark_job\",\"jobId\":\"abc\"}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "benchmark_job");
        assert_eq!(events[0].data["jobId"], "abc");
    }

    #[test]
    fn parser_handles_multiline_data() {
        let mut parser = SseParser::default();
        let events = parser
            .push_chunk(b"event: chunk\ndata: line one\ndata: line two\n\n");
        assert_eq!(events.len(), 1);
        // Multi-line data joins with \n (per the SSE spec).
        assert!(events[0].raw.contains("line one\nline two"));
    }

    #[test]
    fn parser_skips_keepalive_comments() {
        let mut parser = SseParser::default();
        let events = parser.push_chunk(b":keepalive\n\n");
        assert_eq!(events.len(), 0);
    }

    #[test]
    fn parser_carries_state_across_chunks() {
        let mut parser = SseParser::default();
        let evs1 = parser.push_chunk(b"event: benchmark_job\nda");
        let evs2 = parser.push_chunk(b"ta: {\"type\":\"benchmark_job\"}\n\n");
        assert_eq!(evs1.len(), 0);
        assert_eq!(evs2.len(), 1);
        assert_eq!(evs2[0].event_type, "benchmark_job");
    }
}
