//! HAIAI_WASM_PRD §5.4 / Task 042 / Issues 006 + 010 — typed
//! `StorageUnavailable` and `QuotaExceeded` browser tests for the
//! `save_encrypted_agent` code path.
//!
//! These tests close the gap called out in Issue 010 + Issue 006
//! re-review: the previous secret-leak test could only show
//! `RefusedPayload` for malformed input; it could not show that an
//! actual `localStorage.setItem` failure surfaces as a typed
//! `StorageUnavailable` or `QuotaExceeded`.
//!
//! Monkey-patching strategy:
//!
//!   - `monkey_patch_set_item(Throws::Quota)` replaces
//!     `Storage.prototype.setItem` with a JS shim that throws a
//!     `DOMException` whose `name === "QuotaExceededError"`. The
//!     `jacs_wasm::local_store` quota-classification helper matches on
//!     that name and emits `LocalStoreError::QuotaExceeded`.
//!   - `monkey_patch_set_item(Throws::Generic)` throws a plain
//!     `Error("disabled")` — the same helper falls through to
//!     `LocalStoreError::StorageUnavailable`.
//!
//! Each patch returns an RAII restorer that reinstalls the original
//! `setItem` on drop, so a panicking assert can't leave the next test
//! with a broken storage.

#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

const STORAGE_KEY: &str = "haiai-wasm-storage-errors-test-agent";
/// Minimal but valid encrypted-agent payload (V2 envelope shape) — the
/// shape check passes so the only thing that can fail is the
/// `setItem` call we monkey-patched.
const VALID_ENCRYPTED_PAYLOAD: &str = r#"{
    "config": {},
    "agent": {},
    "public_key": [1, 2, 3],
    "encrypted_private_key": {
        "jacsEncryptedPrivateKeyVersion": 2,
        "cipher": "AES-256-GCM",
        "ciphertext": "deadbeef",
        "salt": "saltsalt",
        "nonce": "noncenonce"
    },
    "algorithm": "ed25519"
}"#;

// ---------------------------------------------------------------------------
// Monkey-patch helpers.
// ---------------------------------------------------------------------------

/// Which error the patched `setItem` should throw.
enum Throws {
    Quota,
    Generic,
}

/// Restore the original `Storage.prototype.setItem` on drop.
struct SetItemRestorer {
    prototype: JsValue,
    original: JsValue,
}

impl Drop for SetItemRestorer {
    fn drop(&mut self) {
        // Replace the patched setItem with the original. We use
        // `defineProperty` so the writable/configurable bits aren't
        // re-set unexpectedly.
        let descriptor = js_sys::Object::new();
        js_sys::Reflect::set(
            &descriptor,
            &JsValue::from_str("value"),
            &self.original,
        )
        .expect("descriptor.value");
        js_sys::Reflect::set(
            &descriptor,
            &JsValue::from_str("writable"),
            &JsValue::TRUE,
        )
        .expect("descriptor.writable");
        js_sys::Reflect::set(
            &descriptor,
            &JsValue::from_str("configurable"),
            &JsValue::TRUE,
        )
        .expect("descriptor.configurable");
        let _ = js_sys::Reflect::define_property(
            self.prototype.unchecked_ref::<js_sys::Object>(),
            &JsValue::from_str("setItem"),
            &descriptor,
        );
    }
}

/// Replace `Storage.prototype.setItem` with a shim that always throws.
/// Returns a guard that restores the original on drop.
fn monkey_patch_set_item(throws: Throws) -> SetItemRestorer {
    let window = web_sys::window().expect("window");
    let storage = window
        .local_storage()
        .expect("local_storage() ok")
        .expect("storage is Some");
    let storage_value: JsValue = storage.into();
    let prototype = js_sys::Object::get_prototype_of(&storage_value).into();
    let original = js_sys::Reflect::get(&prototype, &JsValue::from_str("setItem"))
        .expect("read original setItem");

    // Build the throwing shim via JS source so DOMException construction is
    // platform-faithful. The shim closes over the error name; QuotaExceededError
    // is what every major browser raises on actual quota exhaustion.
    let body = match throws {
        Throws::Quota => {
            "function patched_setItem(_k, _v) { \
             throw new DOMException('Quota exceeded by haiai-wasm test', 'QuotaExceededError'); \
             }; return patched_setItem;"
        }
        Throws::Generic => {
            "function patched_setItem(_k, _v) { \
             throw new Error('localStorage disabled by haiai-wasm test'); \
             }; return patched_setItem;"
        }
    };
    let make_fn = js_sys::Function::new_no_args(body);
    let patched = make_fn
        .call0(&JsValue::UNDEFINED)
        .expect("build patched setItem");

    let descriptor = js_sys::Object::new();
    js_sys::Reflect::set(&descriptor, &JsValue::from_str("value"), &patched)
        .expect("descriptor.value");
    js_sys::Reflect::set(
        &descriptor,
        &JsValue::from_str("writable"),
        &JsValue::TRUE,
    )
    .expect("descriptor.writable");
    js_sys::Reflect::set(
        &descriptor,
        &JsValue::from_str("configurable"),
        &JsValue::TRUE,
    )
    .expect("descriptor.configurable");
    js_sys::Reflect::define_property(
        prototype.unchecked_ref::<js_sys::Object>(),
        &JsValue::from_str("setItem"),
        &descriptor,
    )
    .expect("install patched setItem");

    SetItemRestorer { prototype, original }
}

/// Remove our test key from any prior failed run, ignoring errors that
/// might come from the monkey-patched setItem already being uninstalled.
fn clear_test_keys() {
    if let Some(w) = web_sys::window() {
        if let Ok(Some(s)) = w.local_storage() {
            let len = s.length().unwrap_or(0);
            let mut to_remove = Vec::new();
            for i in 0..len {
                if let Ok(Some(k)) = s.key(i) {
                    if k.contains(STORAGE_KEY) {
                        to_remove.push(k);
                    }
                }
            }
            for k in to_remove {
                let _ = s.remove_item(&k);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------

/// Sanity-check the patcher itself: without any patch in place, the
/// valid payload persists. Drives the same call path the failure tests
/// will exercise, so a green here means the failure tests genuinely
/// observe the monkey-patched throw and not some other validation
/// short-circuit.
#[wasm_bindgen_test]
fn save_encrypted_agent_succeeds_without_patch() {
    clear_test_keys();
    jacs_wasm::local_store::save_encrypted_agent(STORAGE_KEY, VALID_ENCRYPTED_PAYLOAD)
        .expect("happy-path save");
    clear_test_keys();
}

/// `QuotaExceededError` from `setItem` must surface as the
/// `QuotaExceeded` typed error (PRD §5.4 + Issue 010 follow-up).
#[wasm_bindgen_test]
fn save_encrypted_agent_surfaces_typed_quota_exceeded() {
    clear_test_keys();
    let _restorer = monkey_patch_set_item(Throws::Quota);

    let err = jacs_wasm::local_store::save_encrypted_agent(STORAGE_KEY, VALID_ENCRYPTED_PAYLOAD)
        .expect_err("monkey-patched setItem must throw");
    assert_eq!(
        err.code(),
        "QuotaExceeded",
        "patched QuotaExceededError must classify as QuotaExceeded, got code: {}",
        err.code()
    );

    // Defense-in-depth: monkey-patched setItem refused to write, so no
    // entry should be visible in the (real, unpatched) storage either.
    // Drop the restorer first so we can call get_item without re-tripping
    // the patch.
    drop(_restorer);
    let w = web_sys::window().expect("window");
    let s = w
        .local_storage()
        .expect("local_storage")
        .expect("Some");
    // Use the namespaced lookup path the library uses internally.
    let stored = s
        .get_item(&format!("jacs:{STORAGE_KEY}"))
        .expect("get_item");
    assert!(stored.is_none(), "no entry should be persisted after a rejected save");
}

/// A generic `Error` (not a `QuotaExceededError`) from `setItem` must
/// surface as the `StorageUnavailable` typed error. Mirrors
/// Safari/Firefox private-mode disabled-storage behavior.
#[wasm_bindgen_test]
fn save_encrypted_agent_surfaces_typed_storage_unavailable() {
    clear_test_keys();
    let _restorer = monkey_patch_set_item(Throws::Generic);

    let err = jacs_wasm::local_store::save_encrypted_agent(STORAGE_KEY, VALID_ENCRYPTED_PAYLOAD)
        .expect_err("monkey-patched setItem must throw");
    assert_eq!(
        err.code(),
        "StorageUnavailable",
        "non-quota throws must classify as StorageUnavailable, got code: {}",
        err.code()
    );

    drop(_restorer);
    let w = web_sys::window().expect("window");
    let s = w
        .local_storage()
        .expect("local_storage")
        .expect("Some");
    let stored = s
        .get_item(&format!("jacs:{STORAGE_KEY}"))
        .expect("get_item");
    assert!(stored.is_none(), "no entry should be persisted after a rejected save");
}
