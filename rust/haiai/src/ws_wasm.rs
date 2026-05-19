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
//! ## Auth header (interim — see "Future protocol" below)
//!
//! WebSocket spec forbids custom headers on the initial handshake from
//! the browser. The wasm path currently uses a `?auth=<token>`
//! query-string token carrying the same JACS auth header value the
//! native path sends in the `Authorization` header. Callers MUST build
//! the URL via [`build_authenticated_ws_url`], which:
//!
//! 1. Refuses non-`wss://` URLs — `ws://` would put the auth token
//!    in cleartext on the wire and into proxy logs. Returns
//!    `HaiError::Configuration` rather than silently downgrading.
//! 2. Percent-encodes the auth header value with `NON_ALPHANUMERIC`
//!    so the JACS `Authorization` payload (which contains `:` and
//!    base64 chars) is URL-safe.
//!
//! ### Known leak surface
//!
//! Even with `wss://` enforced, the auth token shows up in:
//! - The server's HTTP access logs (URL query string).
//! - Any reverse proxy / WAF logs in front of hai/api.
//! - Browser DevTools' Network tab (always visible to the user, but
//!   also visible to any other JS on the page that opens DevTools
//!   programmatically — rare in practice).
//!
//! It does NOT show up in browser history (WS connections don't
//! populate history) or in cross-origin scripts (same-origin policy
//! protects the URL).
//!
//! ### Future protocol (Option C — recommended)
//!
//! The clean fix is a first-frame auth message:
//!
//! 1. Browser opens unauthed `wss://hai.ai/ws/...`.
//! 2. hai/api accepts the connection but holds it in an unauthenticated
//!    state, accepting no subscriptions or sends.
//! 3. Client sends `{"type":"auth","token":"<JACS auth header value>"}`
//!    as the first text frame.
//! 4. hai/api validates and either flips the connection into
//!    authenticated state or closes with code 4401.
//!
//! No URL leak, no proxy log exposure, no custom-header limitation.
//! Requires the matching backend change documented in
//! `docs/HAIAI_WASM_BACKEND_ASSUMPTIONS.md`; until that ships,
//! `build_authenticated_ws_url` is the supported path.

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use async_trait::async_trait;
use futures_channel::mpsc::{unbounded, UnboundedReceiver};
use futures_util::StreamExt;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{BinaryType, CloseEvent, ErrorEvent, MessageEvent, WebSocket};

use crate::error::{HaiError, Result};
use crate::ws_protocol::{WebSocketTransport, WsMessage};

// `build_authenticated_ws_url` was moved to `ws_protocol::build_authenticated_ws_url`
// so its scheme-guard tests can run on native CI (this file is `cfg(target_arch =
// "wasm32")`). Re-export for callers that still import from `ws_wasm`.
pub use crate::ws_protocol::build_authenticated_ws_url;


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

