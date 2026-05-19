// Copyright (c) 2026 Human Assisted Intelligence, Inc.
//
// Use of this software is governed by the Business Source License 1.1
// included in the LICENSE file.
//
// SPDX-License-Identifier: BUSL-1.1

//! `haiai-wasm` — browser-side wasm-bindgen wrapper around `haiai`
//! (compiled with the `wasm` feature).
//!
//! This crate is the Rust half of the published `@haiai/wasm` npm
//! package. It exposes:
//!
//! * `initHaiaiWasm()` — idempotent init; sets up panic hook, calls into
//!   the underlying `@jacs/wasm` init.
//! * `BrowserAgentHandle` — JS-facing handle that wraps a
//!   `jacs_wasm::CoreAgentHandle` plus a `haiai::client::HaiClient`.
//!   Methods land in Task 021 (lifecycle), Task 022 (local crypto), and
//!   Tasks 023-028 (HAI HTTP wrappers).
//! * `EventStreamHandle` — async iterator bridge for SSE / WS events
//!   (Task 029).
//!
//! ## Native stub
//!
//! When built on a non-wasm32 target the crate compiles to a tiny
//! no-op so workspace-wide `cargo check --workspace` stays green
//! (HAIAI_WASM_PRD §4.1: "Do not rely on 'not part of workspace' as
//! the gating mechanism").
//!
//! Real exports compile only on `target_arch = "wasm32"`. See
//! `browser_agent` for the wasm-bindgen surface.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

// Wasm-bindgen exports live behind the target gate. The submodules
// only compile on wasm32 — pulling them in on native would force the
// crate to link wasm-bindgen (which fails outside a browser host).
#[cfg(target_arch = "wasm32")]
mod browser_agent;

#[cfg(target_arch = "wasm32")]
pub use browser_agent::*;

/// Native no-op stub. Lets `cargo check --workspace` and `cargo
/// publish --dry-run` succeed when building haiai-wasm for the host
/// without crashing on wasm-bindgen runtime asserts.
#[cfg(not(target_arch = "wasm32"))]
pub fn _haiai_wasm_native_stub() -> &'static str {
    "haiai-wasm: native stub (real exports compile only on wasm32-unknown-unknown)"
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;

    #[test]
    fn native_stub_exists() {
        assert!(_haiai_wasm_native_stub().contains("native stub"));
    }
}
