// Copyright (c) 2026 Human Assisted Intelligence, Inc.
// SPDX-License-Identifier: BUSL-1.1

//! `WasmWebSocket` — `WebSocketTransport` impl backed by
//! `web_sys::WebSocket` (HAIAI_WASM_PRD §4.6 / Task 018).
//!
//! Mirrors the native `tokio_tungstenite` transport in client.rs but
//! talks to the browser's built-in WebSocket. JS-side callbacks
//! (`onmessage`, `onclose`, `onerror`) are bridged into a Rust async
//! channel so the same `next_message` / `send_message` / `close` API is
//! exposed.
//!
//! Auth header: WebSocket spec forbids custom headers on the initial
//! handshake from the browser. HAIAI_WASM_PRD §4.6 documents the
//! decision: the wasm path uses a `?auth=<token>` query-string token
//! (the same auth header value the native path sends in the
//! `Authorization` header). Callers MUST build the URL via
//! [`build_authenticated_ws_url`] so the encoded shape matches what
//! hai/api expects.

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use async_trait::async_trait;
use futures_channel::mpsc::{unbounded, UnboundedReceiver};
use futures_util::StreamExt;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{BinaryType, CloseEvent, ErrorEvent, MessageEvent, WebSocket};

use crate::error::{HaiError, Result};
use crate::ws_protocol::{WebSocketTransport, WsMessage};

/// Build an authenticated WS URL by appending `?auth=<percent-encoded-header>`.
///
/// Browsers can't set `Authorization` on the WebSocket handshake (the
/// spec only allows `Sec-WebSocket-Protocol`), so we move the same JACS
/// auth header into a query parameter. hai/api inspects either header
/// or query token (per `docs/HAIAI_WASM_BACKEND_ASSUMPTIONS.md` —
/// Task 002).
pub fn build_authenticated_ws_url(base_ws_url: &str, auth_header: &str) -> String {
    let encoded = utf8_percent_encode(auth_header, NON_ALPHANUMERIC).to_string();
    if base_ws_url.contains('?') {
        format!("{base_ws_url}&auth={encoded}")
    } else {
        format!("{base_ws_url}?auth={encoded}")
    }
}

/// Wasm `WebSocketTransport` impl. Owns the underlying browser
/// `WebSocket` plus the closures that forward JS events into a
/// futures-channel mpsc receiver.
pub struct WasmWebSocket {
    ws: WebSocket,
    rx: UnboundedReceiver<WsMessage>,
    // Hold the Closures alive — dropping them invalidates the JS
    // listeners and the connection silently stops delivering events.
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_close: Closure<dyn FnMut(CloseEvent)>,
    _on_error: Closure<dyn FnMut(ErrorEvent)>,
    closed: Rc<RefCell<bool>>,
}

impl WasmWebSocket {
    /// Open a WebSocket connection. Returns once the constructor
    /// succeeds; the actual handshake completes asynchronously and the
    /// first `next_message` call awaits the JS event loop.
    pub fn connect(url: &str) -> Result<Self> {
        let ws = WebSocket::new(url)
            .map_err(|e| HaiError::Message(format!("WebSocket::new failed: {e:?}")))?;
        ws.set_binary_type(BinaryType::Arraybuffer);

        let (tx, rx) = unbounded::<WsMessage>();
        let closed = Rc::new(RefCell::new(false));

        let tx_msg = tx.clone();
        let on_message = Closure::wrap(Box::new(move |evt: MessageEvent| {
            // Text frames arrive as JS strings; binary frames as
            // ArrayBuffer. Both produce a WsMessage variant.
            if let Some(text) = evt.data().as_string() {
                let _ = tx_msg.unbounded_send(WsMessage::Text(text));
            } else if let Ok(buf) = evt.data().dyn_into::<js_sys::ArrayBuffer>() {
                let array = js_sys::Uint8Array::new(&buf);
                let mut bytes = vec![0u8; array.length() as usize];
                array.copy_to(&mut bytes);
                let _ = tx_msg.unbounded_send(WsMessage::Binary(bytes));
            }
            // Other event-data shapes (Blob) are unused by HAI today.
        }) as Box<dyn FnMut(_)>);
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));

        let tx_close = tx.clone();
        let closed_close = Rc::clone(&closed);
        let on_close = Closure::wrap(Box::new(move |_evt: CloseEvent| {
            *closed_close.borrow_mut() = true;
            let _ = tx_close.unbounded_send(WsMessage::Close);
            // Drop the sender so receivers see end-of-stream after
            // they drain the final Close marker.
            tx_close.close_channel();
        }) as Box<dyn FnMut(_)>);
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));

        let tx_err = tx.clone();
        let closed_err = Rc::clone(&closed);
        let on_error = Closure::wrap(Box::new(move |_evt: ErrorEvent| {
            *closed_err.borrow_mut() = true;
            // Errors collapse to Close; the connection is unusable.
            let _ = tx_err.unbounded_send(WsMessage::Close);
            tx_err.close_channel();
        }) as Box<dyn FnMut(_)>);
        ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));

        Ok(Self {
            ws,
            rx,
            _on_message: on_message,
            _on_close: on_close,
            _on_error: on_error,
            closed,
        })
    }
}

#[async_trait(?Send)]
impl WebSocketTransport for WasmWebSocket {
    async fn next_message(&mut self) -> Option<WsMessage> {
        self.rx.next().await
    }

    async fn send_message(&mut self, msg: WsMessage) -> Result<()> {
        if *self.closed.borrow() {
            return Err(HaiError::Message("WasmWebSocket: send on closed".into()));
        }
        match msg {
            WsMessage::Text(s) => self
                .ws
                .send_with_str(&s)
                .map_err(|e| HaiError::Message(format!("WasmWebSocket::send_with_str: {e:?}"))),
            WsMessage::Binary(bytes) => self
                .ws
                .send_with_u8_array(&bytes)
                .map_err(|e| HaiError::Message(format!("WasmWebSocket::send_with_u8_array: {e:?}"))),
            WsMessage::Close => self.close().await,
        }
    }

    async fn close(&mut self) -> Result<()> {
        if *self.closed.borrow() {
            return Ok(());
        }
        *self.closed.borrow_mut() = true;
        self.ws
            .close()
            .map_err(|e| HaiError::Message(format!("WasmWebSocket::close: {e:?}")))?;
        Ok(())
    }
}
