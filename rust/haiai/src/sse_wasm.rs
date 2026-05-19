// Copyright (c) 2026 Human Assisted Intelligence, Inc.
// SPDX-License-Identifier: BUSL-1.1

//! `WasmSseConnection` — authenticated SSE consumer for the browser
//! (HAIAI_WASM_PRD §4.6 / Task 019).
//!
//! Native uses `reqwest::Response::bytes_stream()` (`stream` feature);
//! reqwest's wasm32 shim does NOT expose a streaming body, so we drop
//! down to `web_sys` directly: `fetch()` → `Response::body()` →
//! `ReadableStream::get_reader()` → loop pulling chunks via the
//! `ReadableStreamDefaultReader`. UTF-8 decode happens at the
//! `TextDecoder` boundary; bytes feed into the shared
//! `sse_parse::SseParser` (Task 013) so the event shape is byte-
//! identical to the native consumer.
//!
//! Auth header: SSE in the browser uses `fetch()` + `ReadableStream`
//! specifically because `EventSource` does NOT allow custom request
//! headers, and HAI requires `Authorization: JACS …`. The caller passes
//! the same auth header value the native `connect_sse` builds.

#![cfg(target_arch = "wasm32")]

use js_sys::Uint8Array;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, ReadableStreamDefaultReader, Request, RequestInit, Response, TextDecoder};

use crate::error::{HaiError, Result};
use crate::sse_parse::SseParser;
use crate::types::HaiEvent;

/// Browser SSE consumer. Owns the response reader plus the streaming
/// parser. `next_event` reads chunks until the parser emits the next
/// event or the stream closes.
pub struct WasmSseConnection {
    reader: ReadableStreamDefaultReader,
    decoder: TextDecoder,
    parser: SseParser,
    pending: std::collections::VecDeque<HaiEvent>,
    closed: bool,
}

impl WasmSseConnection {
    /// Open an authenticated SSE stream. Builds a `GET` request with
    /// the supplied Authorization header, awaits `fetch()`, opens the
    /// body's `ReadableStream` reader.
    pub async fn connect(url: &str, auth_header: &str) -> Result<Self> {
        let global = js_sys::global();
        let window = global
            .dyn_into::<web_sys::WorkerGlobalScope>()
            .map(|w| w.unchecked_into::<js_sys::Object>())
            .or_else(|_| {
                web_sys::window()
                    .map(|w| w.unchecked_into::<js_sys::Object>())
                    .ok_or_else(|| {
                        HaiError::Message(
                            "WasmSseConnection: no global window/worker scope".to_string(),
                        )
                    })
            })?;

        let init = RequestInit::new();
        init.set_method("GET");

        let headers = Headers::new()
            .map_err(|e| HaiError::Message(format!("Headers::new failed: {e:?}")))?;
        headers
            .set("Authorization", auth_header)
            .map_err(|e| HaiError::Message(format!("Headers::set Authorization: {e:?}")))?;
        headers
            .set("Accept", "text/event-stream")
            .map_err(|e| HaiError::Message(format!("Headers::set Accept: {e:?}")))?;
        headers
            .set("Cache-Control", "no-cache")
            .map_err(|e| HaiError::Message(format!("Headers::set Cache-Control: {e:?}")))?;
        init.set_headers(&headers);

        let request = Request::new_with_str_and_init(url, &init)
            .map_err(|e| HaiError::Message(format!("Request::new failed: {e:?}")))?;

        // Spec: globalThis.fetch is the universal handle in both Window
        // and Worker contexts.
        let fetch_fn = js_sys::Reflect::get(&window, &JsValue::from_str("fetch"))
            .map_err(|e| HaiError::Message(format!("globalThis.fetch missing: {e:?}")))?;
        let fetch = fetch_fn
            .dyn_into::<js_sys::Function>()
            .map_err(|_| HaiError::Message("globalThis.fetch is not a function".to_string()))?;
        let promise = fetch
            .call1(&window, &request)
            .map_err(|e| HaiError::Message(format!("fetch invocation failed: {e:?}")))?;
        let promise: js_sys::Promise = promise
            .dyn_into()
            .map_err(|_| HaiError::Message("fetch did not return a Promise".to_string()))?;

        let response_value = JsFuture::from(promise)
            .await
            .map_err(|e| HaiError::Message(format!("fetch awaited: {e:?}")))?;
        let response: Response = response_value
            .dyn_into()
            .map_err(|_| HaiError::Message("fetch resolved to non-Response".to_string()))?;

        if !response.ok() {
            return Err(HaiError::Message(format!(
                "SSE fetch failed: HTTP {}",
                response.status()
            )));
        }
        let body = response
            .body()
            .ok_or_else(|| HaiError::Message("Response has no body stream".to_string()))?;
        let reader = body
            .get_reader()
            .dyn_into::<ReadableStreamDefaultReader>()
            .map_err(|_| HaiError::Message("body.get_reader() not a default reader".to_string()))?;

        let decoder = TextDecoder::new_with_label("utf-8")
            .map_err(|e| HaiError::Message(format!("TextDecoder::new: {e:?}")))?;

        Ok(Self {
            reader,
            decoder,
            parser: SseParser::default(),
            pending: std::collections::VecDeque::new(),
            closed: false,
        })
    }

    /// Read until the next `HaiEvent` is available or the stream closes.
    /// Returns `None` after the underlying ReadableStream signals done.
    pub async fn next_event(&mut self) -> Option<HaiEvent> {
        // Drain the FIFO buffer of events the parser produced on a prior
        // call before pulling more chunks from the stream.
        if let Some(ev) = self.pending.pop_front() {
            return Some(ev);
        }

        while !self.closed {
            let read_promise = self.reader.read();
            let result_value = match JsFuture::from(read_promise).await {
                Ok(v) => v,
                Err(_) => {
                    self.closed = true;
                    return None;
                }
            };

            // The reader returns `{ value: Uint8Array, done: bool }`.
            let done = js_sys::Reflect::get(&result_value, &JsValue::from_str("done"))
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if done {
                self.closed = true;
                return None;
            }
            let value = match js_sys::Reflect::get(&result_value, &JsValue::from_str("value")) {
                Ok(v) => v,
                Err(_) => {
                    self.closed = true;
                    return None;
                }
            };
            let bytes_array = match value.dyn_into::<Uint8Array>() {
                Ok(a) => a,
                Err(_) => continue,
            };
            let mut bytes = vec![0u8; bytes_array.length() as usize];
            bytes_array.copy_to(&mut bytes);

            // Decode + feed into the shared SSE parser.
            let chunk_text = self
                .decoder
                .decode_with_u8_array(&bytes)
                .unwrap_or_default();
            for ev in self.parser.push_chunk(chunk_text.as_bytes()) {
                self.pending.push_back(ev);
            }
            if let Some(ev) = self.pending.pop_front() {
                return Some(ev);
            }
        }

        None
    }

    /// Cancel the underlying reader. Subsequent `next_event` calls
    /// return `None`. Idempotent.
    pub async fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        let _ = JsFuture::from(self.reader.cancel()).await;
    }
}
