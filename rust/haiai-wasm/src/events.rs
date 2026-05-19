// Copyright (c) 2026 Human Assisted Intelligence, Inc.
// SPDX-License-Identifier: BUSL-1.1

//! `EventStreamHandle` + `connectSse` / `connectWs` (HAIAI_WASM_PRD §4.3
//! event-stream section + §4.6 / Task 029).
//!
//! Unifies the wasm SSE consumer (`haiai::sse_wasm::WasmSseConnection`)
//! and the wasm WS consumer (`haiai::ws_wasm::WasmWebSocket` wrapped in
//! the shared parser) behind one JS-facing `EventStreamHandle`. The TS
//! wrapper (Task 033) turns the handle into an
//! `AsyncIterableIterator<HaiEvent>`.

#![cfg(target_arch = "wasm32")]

use serde::Serialize;
use wasm_bindgen::prelude::*;

use crate::errors::{js_error, map_hai_error};
use haiai::sse_wasm::WasmSseConnection;
use haiai::types::HaiEvent;
use haiai::ws_protocol::{parse_frame_text, WebSocketTransport, WsMessage};
use haiai::ws_wasm::{build_authenticated_ws_url, WasmWebSocket};

enum Inner {
    Sse(WasmSseConnection),
    Ws(WasmWebSocket),
    Closed,
}

#[wasm_bindgen]
pub struct EventStreamHandle {
    inner: Inner,
}

#[wasm_bindgen]
impl EventStreamHandle {
    /// Open an authenticated SSE stream against `url` using the
    /// supplied `Authorization` header. Errors map to typed wasm
    /// errors.
    #[wasm_bindgen(js_name = openSse)]
    pub async fn open_sse(url: &str, auth_header: &str) -> Result<EventStreamHandle, JsValue> {
        let conn = WasmSseConnection::connect(url, auth_header)
            .await
            .map_err(|e| JsValue::from(map_hai_error(e)))?;
        Ok(EventStreamHandle {
            inner: Inner::Sse(conn),
        })
    }

    /// Open a WebSocket connection. `base_ws_url` is the bare
    /// `wss://hai.ai/ws/...` endpoint; the auth header is appended as
    /// `?auth=<encoded>` because browsers can't set custom headers on
    /// WS handshake.
    #[wasm_bindgen(js_name = openWs)]
    pub async fn open_ws(
        base_ws_url: &str,
        auth_header: &str,
    ) -> Result<EventStreamHandle, JsValue> {
        // build_authenticated_ws_url enforces wss:// (refuses ws://) — see
        // ws_wasm.rs module docs. ConfigInvalid bubbles through map_hai_error.
        let url = build_authenticated_ws_url(base_ws_url, auth_header)
            .map_err(|e| JsValue::from(map_hai_error(e)))?;
        let ws = WasmWebSocket::connect(&url)
            .map_err(|e| JsValue::from(map_hai_error(e)))?;
        Ok(EventStreamHandle {
            inner: Inner::Ws(ws),
        })
    }

    /// Return the next `HaiEvent` as a JS object, or `null` when the
    /// stream ends. JS callers can wrap this into an async iterator
    /// (see the TS wrapper in `node-wasm/index.ts`).
    #[wasm_bindgen(js_name = nextEvent)]
    pub async fn next_event(&mut self) -> Result<JsValue, JsValue> {
        let event = match &mut self.inner {
            Inner::Sse(conn) => conn.next_event().await,
            Inner::Ws(ws) => loop {
                let Some(msg) = ws.next_message().await else {
                    break None;
                };
                match msg {
                    WsMessage::Text(text) => {
                        let parsed = parse_frame_text(&text);
                        if let Some(WsMessage::Text(pong)) = parsed.reply {
                            let _ = ws.send_message(WsMessage::Text(pong)).await;
                        }
                        break Some(parsed.event);
                    }
                    WsMessage::Binary(_) => continue,
                    WsMessage::Close => break None,
                }
            },
            Inner::Closed => None,
        };
        match event {
            Some(ev) => to_js(&ev),
            None => Ok(JsValue::NULL),
        }
    }

    /// Close the underlying transport. Idempotent.
    pub async fn close(&mut self) -> Result<(), JsValue> {
        match std::mem::replace(&mut self.inner, Inner::Closed) {
            Inner::Sse(mut c) => c.close().await,
            Inner::Ws(mut w) => w
                .close()
                .await
                .map_err(|e| JsValue::from(map_hai_error(e)))?,
            Inner::Closed => {}
        }
        Ok(())
    }
}

fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    serde_wasm_bindgen::to_value(value)
        .map_err(|e| JsValue::from(js_error("MalformedResponse", format!("to_value: {e}"))))
}
