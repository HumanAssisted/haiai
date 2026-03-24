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

use hai_binding_core::HaiClientWrapper;

// Own static tokio runtime for this cdylib. Do not reuse hai-binding-core's RT --
// LazyLock<Runtime> in a cdylib loaded via dlopen may have initialization ordering
// issues with its dependencies on some platforms.
static RT: std::sync::LazyLock<tokio::runtime::Runtime> =
    std::sync::LazyLock::new(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create haiigo tokio runtime")
    });

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
    match CString::new(s) {
        Ok(cs) => cs.into_raw(),
        Err(_) => {
            // Interior NUL byte in response -- return error envelope instead
            // of silently returning an empty string.
            CString::new(r#"{"error":{"kind":"Generic","message":"response contained interior NUL byte"}}"#)
                .unwrap()
                .into_raw()
        }
    }
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

// Thread-local storage for the last error from hai_client_new.
// Used because hai_client_new returns a handle (pointer), not a JSON string,
// so it cannot return error details inline. Call hai_last_error() after a null
// return to retrieve the error details.
std::thread_local! {
    static LAST_ERROR: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Retrieve the last error from `hai_client_new` as a JSON string.
/// Returns null if no error is stored. Caller must free the returned string
/// with `hai_free_string`.
#[no_mangle]
pub extern "C" fn hai_last_error() -> *mut c_char {
    LAST_ERROR.with(|e| {
        let err = e.borrow_mut().take();
        match err {
            Some(s) => to_c_string(s),
            None => std::ptr::null_mut(),
        }
    })
}

/// Create a new HAI client from a config JSON string.
/// Returns an opaque handle. Caller must call `hai_client_free` when done.
/// On failure, returns null. Call `hai_last_error()` to get error details.
#[no_mangle]
pub extern "C" fn hai_client_new(config_json: *const c_char) -> HaiClientHandle {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let config = unsafe { c_str_to_string(config_json) };
        match HaiClientWrapper::from_config_json_auto(&config) {
            Ok(wrapper) => {
                let arc = Arc::new(wrapper);
                Box::into_raw(Box::new(arc)) as HaiClientHandle
            }
            Err(e) => {
                LAST_ERROR.with(|le| {
                    *le.borrow_mut() = Some(error_to_json(&e));
                });
                std::ptr::null()
            }
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
            if handle.is_null() {
                return to_c_string(r#"{"error":{"kind":"Generic","message":"null client handle"}}"#.to_string());
            }
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
            if handle.is_null() {
                return to_c_string(r#"{"error":{"kind":"Generic","message":"null client handle"}}"#.to_string());
            }
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
            if handle.is_null() {
                return to_c_string(r#"{"error":{"kind":"Generic","message":"null client handle"}}"#.to_string());
            }
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
    if handle.is_null() {
        return to_c_string(r#"{"error":{"kind":"Generic","message":"null client handle"}}"#.to_string());
    }
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
ffi_method_str!(hai_register_new_agent, register_new_agent);
ffi_method_str!(hai_rotate_keys, rotate_keys);
ffi_method_str!(hai_update_agent, update_agent);
ffi_method_str!(hai_submit_response, submit_response);

#[no_mangle]
pub extern "C" fn hai_verify_status(handle: HaiClientHandle, agent_id: *const c_char) -> *mut c_char {
    if handle.is_null() {
        return to_c_string(r#"{"error":{"kind":"Generic","message":"null client handle"}}"#.to_string());
    }
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
    if handle.is_null() {
        return to_c_string(r#"{"error":{"kind":"Generic","message":"null client handle"}}"#.to_string());
    }
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
    if handle.is_null() {
        return to_c_string(r#"{"error":{"kind":"Generic","message":"null client handle"}}"#.to_string());
    }
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
// FFI Methods — Server Keys
// =============================================================================

ffi_method_noarg!(hai_fetch_server_keys, fetch_server_keys);

// =============================================================================
// FFI Methods — Raw Email Sign/Verify
// =============================================================================

ffi_method_str!(hai_sign_email_raw, sign_email_raw);
ffi_method_str!(hai_verify_email_raw, verify_email_raw);

// =============================================================================
// FFI Methods — Attestations
// =============================================================================

ffi_method_str!(hai_create_attestation, create_attestation);
ffi_method_str!(hai_list_attestations, list_attestations);

#[no_mangle]
pub extern "C" fn hai_get_attestation(handle: HaiClientHandle, agent_id: *const c_char, doc_id: *const c_char) -> *mut c_char {
    if handle.is_null() {
        return to_c_string(r#"{"error":{"kind":"Generic","message":"null client handle"}}"#.to_string());
    }
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let client = unsafe { &*handle }.clone();
        let agent_id = unsafe { c_str_to_string(agent_id) };
        let doc_id = unsafe { c_str_to_string(doc_id) };
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            let r = client.get_attestation(&agent_id, &doc_id).await;
            let _ = tx.send(r);
        });
        to_c_string(result_to_json(rx.recv().unwrap()))
    }));
    result.unwrap_or_else(|_| panic_json())
}

ffi_method_str!(hai_verify_attestation, verify_attestation);

// =============================================================================
// FFI Methods — Email Templates
// =============================================================================

ffi_method_str!(hai_create_email_template, create_email_template);
ffi_method_str!(hai_list_email_templates, list_email_templates);
ffi_method_str!(hai_get_email_template, get_email_template);

#[no_mangle]
pub extern "C" fn hai_update_email_template(
    handle: HaiClientHandle,
    template_id: *const c_char,
    options_json: *const c_char,
) -> *mut c_char {
    if handle.is_null() {
        return to_c_string(r#"{"error":{"kind":"Generic","message":"null client handle"}}"#.to_string());
    }
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let client = unsafe { &*handle }.clone();
        let template_id = unsafe { c_str_to_string(template_id) };
        let options_json = unsafe { c_str_to_string(options_json) };
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            let r = client.update_email_template(&template_id, &options_json).await;
            let _ = tx.send(r);
        });
        to_c_string(result_to_json(rx.recv().unwrap()))
    }));
    result.unwrap_or_else(|_| panic_json())
}

ffi_method_void!(hai_delete_email_template, delete_email_template);

// =============================================================================
// FFI Methods — Keys
// =============================================================================

#[no_mangle]
pub extern "C" fn hai_fetch_remote_key(handle: HaiClientHandle, jacs_id: *const c_char, version: *const c_char) -> *mut c_char {
    if handle.is_null() {
        return to_c_string(r#"{"error":{"kind":"Generic","message":"null client handle"}}"#.to_string());
    }
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
    if handle.is_null() {
        return to_c_string(r#"{"error":{"kind":"Generic","message":"null client handle"}}"#.to_string());
    }
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let client = unsafe { &*handle }.clone();
        let name = unsafe { c_str_to_string(name) };
        let tier = unsafe { c_str_to_string(tier) };
        // Trim whitespace before checking for empty -- Go callers pass "" for None,
        // but whitespace-only strings should also be treated as absent.
        let name = name.trim().to_string();
        let tier = tier.trim().to_string();
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
    if handle.is_null() {
        return to_c_string(r#"{"error":{"kind":"Generic","message":"null client handle"}}"#.to_string());
    }
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let client = unsafe { &*handle }.clone();
        let transport = unsafe { c_str_to_string(transport) };
        let transport = transport.trim().to_string();
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
    if handle.is_null() {
        return to_c_string(r#"{"error":{"kind":"Generic","message":"null client handle"}}"#.to_string());
    }
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
    if handle.is_null() {
        return to_c_string(r#"{"error":{"kind":"Generic","message":"null client handle"}}"#.to_string());
    }
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
// FFI Methods — Client State (Read)
// =============================================================================

/// Get the JACS ID of the client.
/// Returns a JSON envelope: `{"ok":"<jacs_id>"}` or `{"error":...}`.
#[no_mangle]
pub extern "C" fn hai_jacs_id(handle: HaiClientHandle) -> *mut c_char {
    if handle.is_null() {
        return to_c_string(r#"{"error":{"kind":"Generic","message":"null client handle"}}"#.to_string());
    }
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let client = unsafe { &*handle }.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            let id = client.jacs_id().await;
            let _ = tx.send(id);
        });
        let id = rx.recv().unwrap();
        let json = serde_json::to_string(&id).unwrap_or_else(|_| format!("\"{}\"", id));
        to_c_string(format!(r#"{{"ok":{json}}}"#))
    }));
    result.unwrap_or_else(|_| panic_json())
}

// =============================================================================
// FFI Methods — Client State (Mutating)
// =============================================================================

#[no_mangle]
pub extern "C" fn hai_set_hai_agent_id(handle: HaiClientHandle, id: *const c_char) -> *mut c_char {
    if handle.is_null() {
        return to_c_string(r#"{"error":{"kind":"Generic","message":"null client handle"}}"#.to_string());
    }
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
    if handle.is_null() {
        return to_c_string(r#"{"error":{"kind":"Generic","message":"null client handle"}}"#.to_string());
    }
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

// =============================================================================
// FFI Methods — SSE Streaming
// =============================================================================

#[no_mangle]
pub extern "C" fn hai_connect_sse(handle: HaiClientHandle) -> u64 {
    if handle.is_null() {
        return 0;
    }
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let client = unsafe { &*handle }.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            let r = client.connect_sse().await;
            let _ = tx.send(r);
        });
        match rx.recv().unwrap() {
            Ok(h) => h,
            Err(_) => 0,
        }
    }));
    result.unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn hai_sse_next_event(handle_id: u64) -> *mut c_char {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            let r = hai_binding_core::sse_next_event(handle_id).await;
            let _ = tx.send(r);
        });
        match rx.recv().unwrap() {
            Ok(Some(json)) => to_c_string(format!(r#"{{"ok":{json}}}"#)),
            Ok(None) => to_c_string(r#"{"ok":null}"#.to_string()),
            Err(e) => to_c_string(error_to_json(&e)),
        }
    }));
    result.unwrap_or_else(|_| panic_json())
}

#[no_mangle]
pub extern "C" fn hai_sse_close(handle_id: u64) {
    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            let _ = hai_binding_core::sse_close(handle_id).await;
            let _ = tx.send(());
        });
        let _ = rx.recv();
    }));
}

// =============================================================================
// FFI Methods — WebSocket Streaming
// =============================================================================

#[no_mangle]
pub extern "C" fn hai_connect_ws(handle: HaiClientHandle) -> u64 {
    if handle.is_null() {
        return 0;
    }
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let client = unsafe { &*handle }.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            let r = client.connect_ws().await;
            let _ = tx.send(r);
        });
        match rx.recv().unwrap() {
            Ok(h) => h,
            Err(_) => 0,
        }
    }));
    result.unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn hai_ws_next_event(handle_id: u64) -> *mut c_char {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            let r = hai_binding_core::ws_next_event(handle_id).await;
            let _ = tx.send(r);
        });
        match rx.recv().unwrap() {
            Ok(Some(json)) => to_c_string(format!(r#"{{"ok":{json}}}"#)),
            Ok(None) => to_c_string(r#"{"ok":null}"#.to_string()),
            Err(e) => to_c_string(error_to_json(&e)),
        }
    }));
    result.unwrap_or_else(|_| panic_json())
}

#[no_mangle]
pub extern "C" fn hai_ws_close(handle_id: u64) {
    let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        RT.spawn(async move {
            let _ = hai_binding_core::ws_close(handle_id).await;
            let _ = tx.send(());
        });
        let _ = rx.recv();
    }));
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use hai_binding_core::{ErrorKind, HaiBindingError};

    #[test]
    fn result_to_json_ok_wraps_in_ok_envelope() {
        let json = result_to_json(Ok(r#"{"hello":"world"}"#.to_string()));
        assert!(json.starts_with(r#"{"ok":"#));
        assert!(json.contains(r#"{"hello":"world"}"#));
    }

    #[test]
    fn result_to_json_err_wraps_in_error_envelope() {
        let err = HaiBindingError::new(ErrorKind::AuthFailed, "token expired");
        let json = result_to_json(Err(err));
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("error").is_some());
        assert_eq!(parsed["error"]["kind"].as_str().unwrap(), "AuthFailed");
        assert_eq!(parsed["error"]["message"].as_str().unwrap(), "token expired");
    }

    #[test]
    fn result_unit_to_json_ok_returns_null() {
        let json = result_unit_to_json(Ok(()));
        assert_eq!(json, r#"{"ok":null}"#);
    }

    #[test]
    fn result_unit_to_json_err_returns_error_envelope() {
        let err = HaiBindingError::new(ErrorKind::NotFound, "resource missing");
        let json = result_unit_to_json(Err(err));
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("error").is_some());
        assert_eq!(parsed["error"]["kind"].as_str().unwrap(), "NotFound");
    }

    #[test]
    fn error_to_json_escapes_special_chars() {
        // Verify that newlines, tabs, and quotes in error messages are properly escaped
        let err = HaiBindingError::new(ErrorKind::Generic, "line1\nline2\ttab\"quote");
        let json = error_to_json(&err);
        // Should be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let msg = parsed["error"]["message"].as_str().unwrap();
        assert!(msg.contains("line1\nline2\ttab\"quote"));
    }

    #[test]
    fn panic_json_returns_valid_json() {
        let ptr = panic_json();
        assert!(!ptr.is_null());
        let s = unsafe { std::ffi::CStr::from_ptr(ptr) }.to_str().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(s).unwrap();
        assert!(parsed.get("error").is_some());
        assert_eq!(parsed["error"]["kind"].as_str().unwrap(), "Generic");
        // Clean up
        unsafe { drop(CString::from_raw(ptr)) };
    }

    #[test]
    fn to_c_string_roundtrips() {
        let original = "hello world";
        let ptr = to_c_string(original.to_string());
        assert!(!ptr.is_null());
        let s = unsafe { std::ffi::CStr::from_ptr(ptr) }.to_str().unwrap();
        assert_eq!(s, original);
        unsafe { drop(CString::from_raw(ptr)) };
    }

    #[test]
    fn to_c_string_handles_interior_nul_byte() {
        // String with an interior NUL byte should return an error envelope,
        // not an empty string.
        let bad = "hello\0world".to_string();
        let ptr = to_c_string(bad);
        assert!(!ptr.is_null());
        let s = unsafe { std::ffi::CStr::from_ptr(ptr) }.to_str().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(s).unwrap();
        assert!(parsed.get("error").is_some(), "Expected error envelope for NUL byte input");
        assert!(
            parsed["error"]["message"].as_str().unwrap().contains("NUL"),
            "Error message should mention NUL byte"
        );
        unsafe { drop(CString::from_raw(ptr)) };
    }

    #[test]
    fn c_str_to_string_handles_null() {
        let s = unsafe { c_str_to_string(std::ptr::null()) };
        assert_eq!(s, "");
    }

    #[test]
    fn hai_client_new_returns_null_for_invalid_config() {
        let config = CString::new("invalid json").unwrap();
        let handle = hai_client_new(config.as_ptr());
        assert!(handle.is_null(), "Expected null handle for invalid config");
        // Verify hai_last_error returns error details
        let err_ptr = hai_last_error();
        if !err_ptr.is_null() {
            let err_str = unsafe { CStr::from_ptr(err_ptr) }.to_str().unwrap();
            let parsed: serde_json::Value = serde_json::from_str(err_str).unwrap();
            assert!(parsed.get("error").is_some(), "Expected error envelope from hai_last_error");
            unsafe { drop(CString::from_raw(err_ptr)) };
        }
    }

    #[test]
    fn hai_last_error_returns_null_when_no_error() {
        // Clear any stale error first
        let _ = hai_last_error();
        let ptr = hai_last_error();
        assert!(ptr.is_null(), "Expected null when no error stored");
    }

    #[test]
    fn hai_client_free_handles_null() {
        // Should not panic or crash
        hai_client_free(std::ptr::null());
    }

    #[test]
    fn hai_jacs_id_returns_error_for_null_handle() {
        let ptr = hai_jacs_id(std::ptr::null());
        assert!(!ptr.is_null());
        let s = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(s).unwrap();
        assert!(parsed.get("error").is_some(), "Expected error for null handle");
        assert_eq!(parsed["error"]["message"].as_str().unwrap(), "null client handle");
        unsafe { drop(CString::from_raw(ptr)) };
    }
}
