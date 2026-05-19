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
mod errors;
#[cfg(target_arch = "wasm32")]
mod events;

#[cfg(target_arch = "wasm32")]
pub use browser_agent::*;
#[cfg(target_arch = "wasm32")]
pub use events::*;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Initialize the wasm runtime. Idempotent.
///
/// HAIAI_WASM_PRD §4.3: `initHaiaiWasm()` calls `initJacsWasm()` under
/// the hood (delegates via `jacs_core` indirectly through the haiai
/// `JacsWasmProvider`). For the wrapper we install the panic hook so
/// Rust panics surface in `console.error` rather than as bare
/// `RuntimeError`s.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(js_name = initHaiaiWasm)]
pub fn init_haiai_wasm() {
    console_error_panic_hook::set_once();
}

/// Return the package version string. Used by the TS wrapper for
/// telemetry and for the cross-package version-sync check
/// (HAIAI_WASM_PRD §4.11 / Task 040).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(js_name = version)]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// One-line build descriptor for diagnostics.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(js_name = about)]
pub fn about() -> String {
    format!(
        "{} v{} (browser bindings for HAI API on JACS WASM; see https://hai.ai)",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    )
}

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
