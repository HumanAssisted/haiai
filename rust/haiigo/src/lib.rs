//! Go C FFI binding for HAI SDK.
//!
//! Produces a cdylib (`libhaiigo.dylib`/`.so`) loaded by Go via CGo or purego.
//! Uses the spawn+channel async pattern: each FFI call spawns a tokio task and
//! blocks the calling OS thread on `rx.recv()`.
//!
//! **Error convention:** Every function returns a `*mut c_char` containing JSON:
//! - Success: `{"ok": <result>}`
//! - Error: `{"error": {"kind": "...", "message": "..."}}`
//!
//! Every FFI function uses `catch_unwind` to prevent Rust panics from unwinding
//! across the FFI boundary.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use hai_binding_core::{HaiClientWrapper, RT};
use haiai::jacs::StaticJacsProvider;

// =============================================================================
// Helpers
// =============================================================================

/// Convert a HaiBindingResult to a JSON error envelope string.
///
/// Uses serde_json for error serialization to properly escape all control
/// characters (newlines, tabs, etc.) per RFC 8259.
fn result_to_json(result: Result<String, hai_binding_core::HaiBindingError>) -> String {
    match result {
        Ok(json) => format!(r#"{{"ok":{json}}}"#),
        Err(e) => error_to_json(&e),
    }
}

fn result_unit_to_json(result: Result<(), hai_binding_core::HaiBindingError>) -> String {
    match result {
        Ok(()) => r#"{"ok":null}"#.to_string(),
        Err(e) => error_to_json(&e),
    }
}

/// Serialize an error to a JSON envelope using serde_json for proper escaping.
fn error_to_json(e: &hai_binding_core::HaiBindingError) -> String {
    let err = serde_json::json!({
        "error": {
            "kind": e.kind.to_string(),
            "message": e.message,
        }
    });
    serde_json::to_string(&err).unwrap_or_else(|_|
        r#"{"error":{"kind":"Generic","message":"serialization failed"}}"#.to_string()
    )
}

fn to_c_string(s: String) -> *mut c_char {
    CString::new(s).unwrap_or_default().into_raw()
}

fn panic_json() -> *mut c_char {
    to_c_string(r#"{"error":{"kind":"Generic","message":"Rust panic"}}"#.to_string())
}

unsafe fn c_str_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr) }.to_str().unwrap_or("").to_string()
}

// =============================================================================
// Handle management
// =============================================================================

type HaiClientHandle = *const Arc<HaiClientWrapper>;

/// Create a new HAI client from a config JSON string.
/// Returns an opaque handle. Caller must call `hai_client_free` when done.
#[no_mangle]
pub extern "C" fn hai_client_new(config_json: *const c_char) -> HaiClientHandle {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let config = unsafe { c_str_to_string(config_json) };
        let config_val: serde_json::Value = match serde_json::from_str(&config) {
            Ok(v) => v,
            Err(_) => return std::ptr::null(),
        };

        let jacs_id = config_val
            .get("jacs_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let provider = StaticJacsProvider::new(jacs_id);
        match HaiClientWrapper::from_config_json(&config, Box::new(provider)) {
            Ok(wrapper) => {
                let arc = Arc::new(wrapper);
                Box::into_raw(Box::new(arc)) as HaiClientHandle
            }
            Err(_) => std::ptr::null(),
        }
    }));

    result.unwrap_or(std::ptr::null())
}

/// Free a HAI client handle.
#[no_mangle]
pub extern "C" fn hai_client_free(handle: HaiClientHandle) {
    if !handle.is_null() {
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
            unsafe { drop(Box::from_raw(handle as *mut Arc<HaiClientWrapper>)) };
        }));
    }
}

/// Free a string returned by any hai_* function.
#[no_mangle]
pub extern "C" fn hai_free_string(s: *mut c_char) {
    if !s.is_null() {
        let _ = std::panic::catch_unwind(|| {
            unsafe { drop(CString::from_raw(s)) };
        });
    }
}

// =============================================================================
// Macro for generating FFI methods
// =============================================================================

/// Generate a simple FFI function that takes a handle and one string arg.
macro_rules! ffi_method_str {
    ($fn_name:ident, $method:ident) => {
        #[no_mangle]
        pub extern "C" fn $fn_name(handle: HaiClientHandle, arg: *const c_char) -> *mut c_char {
            let client = unsafe { &*handle }.clone();
            let arg = unsafe { c_str_to_string(arg) };
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                let (tx, rx) = std::sync::mpsc::channel();
                RT.spawn(async move {
                    let r = client.$method(&arg).await;
                    let _ = tx.send(r);
                });
                to_c_string(result_to_json(rx.recv().unwrap()))
            }));
            result.unwrap_or_else(|_| panic_json())
        }
    };
}

/// Generate a simple FFI function that takes a handle and no args.
macro_rules! ffi_method_noarg {
    ($fn_name:ident, $method:ident) => {
        #[no_mangle]
        pub extern "C" fn $fn_name(handle: HaiClientHandle) -> *mut c_char {
            let client = unsafe { &*handle }.clone();
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                let (tx, rx) = std::sync::mpsc::channel();
                RT.spawn(async move {
                    let r = client.$method().await;
                    let _ = tx.send(r);
                });
                to_c_string(result_to_json(rx.recv().unwrap()))
            }));
            result.unwrap_or_else(|_| panic_json())
        }
    };
}

/// Generate a void FFI function (action that returns ()).
macro_rules! ffi_method_void {
    ($fn_name:ident, $method:ident) => {
        #[no_mangle]
        pub extern "C" fn $fn_name(handle: HaiClientHandle, arg: *const c_char) -> *mut c_char {
            let client = unsafe { &*handle }.clone();
            let arg = unsafe { c_str_to_string(arg) };
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                let (tx, rx) = std::sync::mpsc::channel();
                RT.spawn(async move {
                    let r = client.$method(&arg).await;
                    let _ = tx.send(r);
                });
                to_c_string(result_unit_to_json(rx.recv().unwrap()))
            }));
            result.unwrap_or_else(|_| panic_json())
        }
    };
}

// =============================================================================
// FFI Methods — Registration & Identity
// =============================================================================

#[no_mangle]
pub extern "C" fn hai_hello(handle: HaiClientHandle, include_test: bool) -> *mut c_char {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let client = unsafe { &*handle }.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            let r = client.hello(include_test).await;
            let _ = tx.send(r);
        });
        to_c_string(result_to_json(rx.recv().unwrap()))
    }));
    result.unwrap_or_else(|_| panic_json())
}

ffi_method_str!(hai_check_username, check_username);
ffi_method_str!(hai_register, register);
ffi_method_str!(hai_rotate_keys, rotate_keys);
ffi_method_str!(hai_update_agent, update_agent);
ffi_method_str!(hai_submit_response, submit_response);

#[no_mangle]
pub extern "C" fn hai_verify_status(handle: HaiClientHandle, agent_id: *const c_char) -> *mut c_char {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let client = unsafe { &*handle }.clone();
        let agent_id = unsafe { c_str_to_string(agent_id) };
        let agent_id_opt = if agent_id.is_empty() { None } else { Some(agent_id) };
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            let r = client.verify_status(agent_id_opt.as_deref()).await;
            let _ = tx.send(r);
        });
        to_c_string(result_to_json(rx.recv().unwrap()))
    }));
    result.unwrap_or_else(|_| panic_json())
}

// =============================================================================
// FFI Methods — Username
// =============================================================================

#[no_mangle]
pub extern "C" fn hai_claim_username(handle: HaiClientHandle, agent_id: *const c_char, username: *const c_char) -> *mut c_char {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let client = unsafe { &*handle }.clone();
        let agent_id = unsafe { c_str_to_string(agent_id) };
        let username = unsafe { c_str_to_string(username) };
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            let r = client.claim_username(&agent_id, &username).await;
            let _ = tx.send(r);
        });
        to_c_string(result_to_json(rx.recv().unwrap()))
    }));
    result.unwrap_or_else(|_| panic_json())
}

#[no_mangle]
pub extern "C" fn hai_update_username(handle: HaiClientHandle, agent_id: *const c_char, username: *const c_char) -> *mut c_char {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let client = unsafe { &*handle }.clone();
        let agent_id = unsafe { c_str_to_string(agent_id) };
        let username = unsafe { c_str_to_string(username) };
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            let r = client.update_username(&agent_id, &username).await;
            let _ = tx.send(r);
        });
        to_c_string(result_to_json(rx.recv().unwrap()))
    }));
    result.unwrap_or_else(|_| panic_json())
}

ffi_method_str!(hai_delete_username, delete_username);

// =============================================================================
// FFI Methods — Email
// =============================================================================

ffi_method_str!(hai_send_email, send_email);
ffi_method_str!(hai_send_signed_email, send_signed_email);
ffi_method_str!(hai_list_messages, list_messages);
ffi_method_str!(hai_update_labels, update_labels);
ffi_method_noarg!(hai_get_email_status, get_email_status);
ffi_method_str!(hai_get_message, get_message);
ffi_method_noarg!(hai_get_unread_count, get_unread_count);

// =============================================================================
// FFI Methods — Email Actions
// =============================================================================

ffi_method_void!(hai_mark_read, mark_read);
ffi_method_void!(hai_mark_unread, mark_unread);
ffi_method_void!(hai_delete_message, delete_message);
ffi_method_void!(hai_archive, archive);
ffi_method_void!(hai_unarchive, unarchive);
ffi_method_str!(hai_reply_with_options, reply_with_options);
ffi_method_str!(hai_forward, forward);

// =============================================================================
// FFI Methods — Search & Contacts
// =============================================================================

ffi_method_str!(hai_search_messages, search_messages);
ffi_method_noarg!(hai_contacts, contacts);

// =============================================================================
// FFI Methods — Keys
// =============================================================================

#[no_mangle]
pub extern "C" fn hai_fetch_remote_key(handle: HaiClientHandle, jacs_id: *const c_char, version: *const c_char) -> *mut c_char {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let client = unsafe { &*handle }.clone();
        let jacs_id = unsafe { c_str_to_string(jacs_id) };
        let version = unsafe { c_str_to_string(version) };
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            let r = client.fetch_remote_key(&jacs_id, &version).await;
            let _ = tx.send(r);
        });
        to_c_string(result_to_json(rx.recv().unwrap()))
    }));
    result.unwrap_or_else(|_| panic_json())
}

ffi_method_str!(hai_fetch_key_by_hash, fetch_key_by_hash);
ffi_method_str!(hai_fetch_key_by_email, fetch_key_by_email);
ffi_method_str!(hai_fetch_key_by_domain, fetch_key_by_domain);
ffi_method_str!(hai_fetch_all_keys, fetch_all_keys);

// =============================================================================
// FFI Methods — Verification
// =============================================================================

ffi_method_str!(hai_verify_document, verify_document);
ffi_method_str!(hai_get_verification, get_verification);
ffi_method_str!(hai_verify_agent_document, verify_agent_document);

// =============================================================================
// FFI Methods — Benchmarks
// =============================================================================

#[no_mangle]
pub extern "C" fn hai_benchmark(handle: HaiClientHandle, name: *const c_char, tier: *const c_char) -> *mut c_char {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let client = unsafe { &*handle }.clone();
        let name = unsafe { c_str_to_string(name) };
        let tier = unsafe { c_str_to_string(tier) };
        let name_opt = if name.is_empty() { None } else { Some(name) };
        let tier_opt = if tier.is_empty() { None } else { Some(tier) };
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            let r = client.benchmark(name_opt.as_deref(), tier_opt.as_deref()).await;
            let _ = tx.send(r);
        });
        to_c_string(result_to_json(rx.recv().unwrap()))
    }));
    result.unwrap_or_else(|_| panic_json())
}

#[no_mangle]
pub extern "C" fn hai_free_run(handle: HaiClientHandle, transport: *const c_char) -> *mut c_char {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let client = unsafe { &*handle }.clone();
        let transport = unsafe { c_str_to_string(transport) };
        let transport_opt = if transport.is_empty() { None } else { Some(transport) };
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            let r = client.free_run(transport_opt.as_deref()).await;
            let _ = tx.send(r);
        });
        to_c_string(result_to_json(rx.recv().unwrap()))
    }));
    result.unwrap_or_else(|_| panic_json())
}

ffi_method_str!(hai_pro_run, pro_run);

#[no_mangle]
pub extern "C" fn hai_enterprise_run(handle: HaiClientHandle) -> *mut c_char {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let client = unsafe { &*handle }.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            let r = client.enterprise_run().await;
            let _ = tx.send(r);
        });
        to_c_string(result_unit_to_json(rx.recv().unwrap()))
    }));
    result.unwrap_or_else(|_| panic_json())
}

// =============================================================================
// FFI Methods — JACS Delegation
// =============================================================================

ffi_method_str!(hai_sign_message, sign_message);
ffi_method_str!(hai_canonical_json, canonical_json);
ffi_method_str!(hai_verify_a2a_artifact, verify_a2a_artifact);

#[no_mangle]
pub extern "C" fn hai_build_auth_header(handle: HaiClientHandle) -> *mut c_char {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let client = unsafe { &*handle }.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            let r = client.build_auth_header().await;
            let _ = tx.send(r);
        });
        to_c_string(result_to_json(rx.recv().unwrap()))
    }));
    result.unwrap_or_else(|_| panic_json())
}

ffi_method_noarg!(hai_export_agent_json, export_agent_json);

// =============================================================================
// FFI Methods — Client State (Mutating)
// =============================================================================

#[no_mangle]
pub extern "C" fn hai_set_hai_agent_id(handle: HaiClientHandle, id: *const c_char) -> *mut c_char {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let client = unsafe { &*handle }.clone();
        let id = unsafe { c_str_to_string(id) };
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            client.set_hai_agent_id(id).await;
            tx.send(Ok::<(), hai_binding_core::HaiBindingError>(())).ok();
        });
        to_c_string(result_unit_to_json(rx.recv().unwrap()))
    }));
    result.unwrap_or_else(|_| panic_json())
}

#[no_mangle]
pub extern "C" fn hai_set_agent_email(handle: HaiClientHandle, email: *const c_char) -> *mut c_char {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let client = unsafe { &*handle }.clone();
        let email = unsafe { c_str_to_string(email) };
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            client.set_agent_email(email).await;
            tx.send(Ok::<(), hai_binding_core::HaiBindingError>(())).ok();
        });
        to_c_string(result_unit_to_json(rx.recv().unwrap()))
    }));
    result.unwrap_or_else(|_| panic_json())
}
