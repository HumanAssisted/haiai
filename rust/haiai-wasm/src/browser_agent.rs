// Copyright (c) 2026 Human Assisted Intelligence, Inc.
// SPDX-License-Identifier: BUSL-1.1

//! `BrowserAgentHandle` — placeholder. The real lifecycle constructors
//! (`createEphemeral`, `importEncrypted`, `publicOnly`, `clearSecrets`,
//! `isUnlocked`, `exportAgent`, `getPublicKeyBase64`) land in Task 021
//! (HAIAI_WASM_PRD §4.3).
//!
//! Local-crypto methods (`signMessageJson`, `verifyJson`, `signAgreement`,
//! `verifyAgreement`) land in Task 022. HAI HTTP wrappers (registration,
//! email, key/verification, benchmark, local helpers) land in Tasks
//! 023–028. Event streams (`connectSse`, `connectWs`,
//! `EventStreamHandle`) land in Task 029.
//!
//! This skeleton ships only `initHaiaiWasm` so consumers can verify
//! `wasm-pack build --target web` and the npm artifact load before the
//! real methods land.

use wasm_bindgen::prelude::*;

/// Initialize the wasm runtime. Idempotent.
///
/// HAIAI_WASM_PRD §4.3: `initHaiaiWasm()` calls `initJacsWasm()` under
/// the hood (delegates to `jacs_wasm::init_jacs_wasm()`) plus any
/// haiai-wasm-side setup (panic hook, tracing). For the skeleton we
/// only install the panic hook; the JACS init handle and any
/// haiai-side tracing land alongside the lifecycle constructors in
/// Task 021.
#[wasm_bindgen(js_name = initHaiaiWasm)]
pub fn init_haiai_wasm() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    Ok(())
}

/// Return the package version string. Used by the TS wrapper for
/// telemetry and for the cross-package version-sync check
/// (HAIAI_WASM_PRD §4.11 / Task 040).
#[wasm_bindgen(js_name = version)]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// One-line build descriptor for diagnostics.
#[wasm_bindgen(js_name = about)]
pub fn about() -> String {
    format!(
        "{} v{} (browser bindings for HAI API on JACS WASM; see https://hai.ai)",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    )
}
