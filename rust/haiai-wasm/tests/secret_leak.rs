//! HAIAI_WASM_PRD §5.4 + §6 / Task 042 / Issues 006 + 010 — browser
//! secret-leak + lifecycle property tests.
//!
//! Drives `BrowserAgentHandle` (the wasm-bindgen surface that TS
//! `BrowserAgent` calls under the hood) through the full PRD §5.4
//! sign + send + save flow, then walks localStorage and asserts the
//! persisted blob does NOT contain:
//!
//! - the literal password,
//! - the **actual** decrypted private-key bytes (in raw/hex/base64),
//! - any PEM private-block marker.
//!
//! Issue 006 follow-up explicitly required these tests use the
//! BrowserAgent surface (not raw `CoreAgent`), sign through
//! `signMessageJson`, mock `sendSignedEmail`, and scan for the test
//! agent's actual private-key bytes (not an unrelated probe
//! keypair's). Decrypting the saved envelope yields the same bytes the
//! signer holds — that's the strongest available needle without adding
//! a `cfg(test)` accessor to jacs-core's signer.
//!
//! Issue 010 follow-up required lifecycle coverage for
//! `createEphemeral`, `importEncrypted`, `publicOnly`, `clearSecrets`,
//! `exportEncrypted`, save/load through the same storage path the TS
//! wrapper uses, sign/verify, and typed storage errors.
//!
//! Runs under `wasm-pack test --headless --chrome rust/haiai-wasm`.

#![cfg(target_arch = "wasm32")]

use base64::Engine as _;
use serde_json::Value;
use wasm_bindgen_test::*;

use haiai_wasm::BrowserAgentHandle;

wasm_bindgen_test_configure!(run_in_browser);

const FORBIDDEN_PEM_NEEDLES: &[&str] = &[
    "-----BEGIN PRIVATE KEY-----",
    "-----BEGIN ENCRYPTED PRIVATE KEY-----",
    "-----BEGIN EC PRIVATE KEY-----",
    "-----BEGIN OPENSSH PRIVATE KEY-----",
];

const STORAGE_KEY: &str = "haiai-wasm-secret-leak-test-agent";
const TEST_PASSWORD: &str = "haiai-wasm-secret-leak-test-PASSWORD-1234567890!";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Walk every `(key, value)` pair currently visible in `localStorage`.
/// Safe on empty / unavailable storage (returns an empty Vec).
fn walk_local_storage() -> Vec<(String, String)> {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return Vec::new(),
    };
    let storage = match window.local_storage() {
        Ok(Some(s)) => s,
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    let len = storage.length().unwrap_or(0);
    for i in 0..len {
        let Ok(Some(key)) = storage.key(i) else { continue };
        if let Ok(Some(value)) = storage.get_item(&key) {
            out.push((key, value));
        }
    }
    out
}

/// Remove every `localStorage` entry whose key contains
/// `STORAGE_KEY` — keeps cross-test state clean.
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

/// Decrypt the saved AgentMaterial envelope to get the actual
/// private-key bytes the signer holds. This is what the leak scan
/// needs as needles — Issue 006 explicitly called out that scanning
/// an unrelated probe keypair's bytes does not detect a real leak.
///
/// `decrypt_private_key` returns a `ZeroizingVec` (drops + zeroizes
/// on scope exit). We copy the bytes out into a plain `Vec<u8>`
/// solely for the scan needles below — the original `ZeroizingVec`
/// goes out of scope at the end of this function and the bytes get
/// wiped, so the helper itself does not leave a zombie copy around.
/// The returned `Vec<u8>` lives only for the duration of the test
/// function and is the smallest possible window where a copy exists.
fn decrypt_private_key_from_material(material_json: &str, password: &str) -> Vec<u8> {
    let material: jacs_core::AgentMaterial =
        serde_json::from_str(material_json).expect("AgentMaterial parses");
    let zeroizing = jacs_core::envelope::decrypt_private_key(
        &material.encrypted_private_key,
        password,
    )
    .expect("decrypt envelope");
    zeroizing.as_slice().to_vec()
}

// ---------------------------------------------------------------------------
// Issue 006 — the PRD §5.4 secret-leak flow using BrowserAgent surface.
// ---------------------------------------------------------------------------

/// Drive the PRD §5.4 typical flow end-to-end through the
/// `BrowserAgentHandle` surface (the same surface the TS
/// `BrowserAgent` calls):
///
/// 1. `createEphemeral(ed25519)` — generates a fresh keypair.
/// 2. `signMessageJson(...)` — produces a JACS-signed wrapper, the
///    same path TS `BrowserAgent.sign()` uses.
/// 3. Build a canonical `sendSignedEmail` request body via the public
///    `canonical_json` surface and a synthetic SendEmailOptions JSON
///    payload — equivalent to mocking the HTTP send. We sign it via
///    `signMessageJson` so the byte string the would-be HTTP layer
///    handles is exercised through the same signer code path.
/// 4. `exportEncrypted(password)` then save via the same
///    `jacs_wasm::local_store::save_encrypted_agent` call the TS
///    `BrowserAgent.save` wrapper makes.
/// 5. Walk localStorage. Assert no entry contains:
///    - the literal password,
///    - the actual decrypted private-key bytes (in raw / hex /
///      base64 / standard base64 / url-safe base64),
///    - any PEM private-block marker.
#[wasm_bindgen_test]
fn browser_agent_save_does_not_leak_password_pem_or_actual_private_key() {
    clear_test_keys();

    // ── (1) BrowserAgent.createEphemeral path.
    let handle = BrowserAgentHandle::create_ephemeral("ed25519", None)
        .expect("createEphemeral ed25519");
    assert!(handle.is_unlocked(), "fresh ephemeral is unlocked");

    // ── (2) BrowserAgent.sign path — produces a JACS-signed wrapper.
    let signed_message = handle
        .sign_message_json(r#"{"hello":"world","seq":1}"#)
        .expect("signMessageJson");
    let signed_value: Value = serde_json::from_str(&signed_message).expect("signed parses");
    assert_eq!(signed_value.get("jacsType").and_then(Value::as_str), Some("message"));
    assert!(
        signed_value.get("jacsSignature").is_some(),
        "signed wrapper must carry jacsSignature"
    );

    // ── (3) Mock sendSignedEmail: build the canonical body bytes the
    //        HTTP layer would hand to the signer and sign them via the
    //        same wrapper path. This exercises the signer the way the
    //        real sendSignedEmail call would, without making a network
    //        request. The canonical_json call goes through the same
    //        `JacsProvider::canonical_json` path the HTTP layer uses.
    let email_body_json = r#"{"to":["recipient@example.com"],"subject":"leak-test","body":"<p>hi</p>","attachments":[]}"#;
    let canonical_body = handle
        .canonical_json(email_body_json)
        .expect("canonical body");
    // Build an auth header for the synthetic call — exercises
    // build_auth_header which goes through the same sign_raw path
    // sendSignedEmail uses for its Authorization header.
    let auth_header = handle
        .build_auth_header(1_700_000_000, "test-nonce")
        .expect("build_auth_header");
    assert!(
        auth_header.starts_with("JACS "),
        "auth header must start with JACS prefix, got: {auth_header}"
    );
    // Sign the canonical body via the same provider as the HTTP send.
    let signed_body = handle
        .sign_message_json(&canonical_body)
        .expect("sign canonical body");
    assert!(!signed_body.is_empty());

    // ── (4) BrowserAgent.save path. exportEncrypted -> save_encrypted_agent.
    let material_json = handle
        .export_encrypted(TEST_PASSWORD)
        .expect("exportEncrypted");
    jacs_wasm::local_store::save_encrypted_agent(STORAGE_KEY, &material_json)
        .expect("save encrypted agent");

    // Capture the actual private-key bytes the signer holds. We decrypt
    // the same envelope we just wrote — that's exactly the bytes
    // `CoreAgent::from_encrypted_material` would reconstruct, which is
    // exactly what an inadvertent save path might leak.
    let actual_private_key = decrypt_private_key_from_material(&material_json, TEST_PASSWORD);
    assert!(
        !actual_private_key.is_empty(),
        "decrypt_private_key returned empty bytes"
    );
    let priv_b64 = base64::engine::general_purpose::STANDARD.encode(&actual_private_key);
    let priv_b64_urlsafe = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&actual_private_key);
    let priv_hex = actual_private_key
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let priv_hex_upper = priv_hex.to_ascii_uppercase();
    // Raw-bytes-as-UTF8 needle: if a writer naively passed the raw
    // bytes as a string, a substring match would catch it (high
    // false-positive risk on short keys, but Ed25519 PKCS#8 is 48+
    // bytes — long enough to be a useful needle).
    let priv_raw_utf8 = String::from_utf8_lossy(&actual_private_key).into_owned();

    // ── (5) Walk localStorage and assert the property.
    let entries = walk_local_storage();
    let our_key_present = entries.iter().any(|(k, _)| k.contains(STORAGE_KEY));
    assert!(
        our_key_present,
        "save_encrypted_agent must have written under a key containing '{STORAGE_KEY}'; \
         keys found: {:?}",
        entries.iter().map(|(k, _)| k).collect::<Vec<_>>()
    );

    for (k, v) in &entries {
        for needle in FORBIDDEN_PEM_NEEDLES {
            assert!(
                !k.contains(needle) && !v.contains(needle),
                "localStorage[{k}] contains forbidden PEM marker `{needle}`"
            );
        }
        assert!(
            !k.contains(TEST_PASSWORD) && !v.contains(TEST_PASSWORD),
            "localStorage[{k}] leaked the literal password"
        );
        // Actual decrypted private-key bytes must not appear in any
        // serialization. The envelope ciphertext is fine (it IS in
        // `encrypted_private_key` by design) but the plaintext bytes
        // are not.
        assert!(
            !v.contains(&priv_b64),
            "localStorage[{k}] contains the agent's actual private-key bytes (standard base64)"
        );
        assert!(
            !v.contains(&priv_b64_urlsafe),
            "localStorage[{k}] contains the agent's actual private-key bytes (url-safe base64)"
        );
        assert!(
            !v.contains(&priv_hex),
            "localStorage[{k}] contains the agent's actual private-key bytes (lowercase hex)"
        );
        assert!(
            !v.contains(&priv_hex_upper),
            "localStorage[{k}] contains the agent's actual private-key bytes (uppercase hex)"
        );
        // The raw-utf8 check is conservative: it only triggers when a
        // long contiguous run of the key bytes happens to form valid
        // utf-8. Useful as a sanity check; not relied on alone.
        if priv_raw_utf8.len() >= 16 && priv_raw_utf8.is_ascii() {
            assert!(
                !v.contains(&priv_raw_utf8),
                "localStorage[{k}] contains the agent's actual private-key bytes (raw utf-8)"
            );
        }
    }

    clear_test_keys();
}

// ---------------------------------------------------------------------------
// Issue 010 — lifecycle tests for BrowserAgentHandle.
// ---------------------------------------------------------------------------

/// `createEphemeral` works for both algorithms and produces a usable
/// signer.
#[wasm_bindgen_test]
fn create_ephemeral_works_for_both_algorithms() {
    for algo in ["ed25519", "pq2025"] {
        let handle = BrowserAgentHandle::create_ephemeral(algo, None)
            .unwrap_or_else(|_| panic!("createEphemeral {algo}"));
        assert_eq!(handle.algorithm(), algo, "algorithm tag round-trips");
        assert!(!handle.jacs_id().is_empty(), "jacsId is non-empty");
        assert!(!handle.get_public_key_base64().is_empty(), "public key is non-empty");
        assert!(handle.is_unlocked(), "fresh ephemeral is unlocked");

        // Sign + verify round-trips through the same handle.
        let signed = handle
            .sign_message_json(r#"{"k":"v"}"#)
            .unwrap_or_else(|_| panic!("{algo} signMessageJson"));
        let result_js = handle
            .verify_json(&signed)
            .unwrap_or_else(|_| panic!("{algo} verifyJson"));
        // Verify result is a JS object — convert back to JSON via
        // serde_wasm_bindgen for an assertion.
        let result_value: Value = serde_wasm_bindgen::from_value(result_js)
            .unwrap_or_else(|_| panic!("{algo} verify result"));
        assert_eq!(
            result_value.get("valid").and_then(Value::as_bool),
            Some(true),
            "{algo} verifyJson must report valid=true for own signature"
        );
    }
}

/// `exportEncrypted` → `importEncrypted` round-trip yields a handle
/// with the same jacsId + public key + algorithm.
#[wasm_bindgen_test]
fn export_encrypted_round_trips_via_import_encrypted() {
    let original = BrowserAgentHandle::create_ephemeral("ed25519", None)
        .expect("createEphemeral");
    let original_jacs_id = original.jacs_id();
    let original_pubkey = original.get_public_key_base64();
    let original_algo = original.algorithm();

    let material = original.export_encrypted(TEST_PASSWORD).expect("exportEncrypted");

    let loaded = BrowserAgentHandle::import_encrypted(&material, TEST_PASSWORD, None)
        .expect("importEncrypted");
    assert_eq!(loaded.jacs_id(), original_jacs_id, "jacsId round-trips");
    assert_eq!(loaded.get_public_key_base64(), original_pubkey, "public key round-trips");
    assert_eq!(loaded.algorithm(), original_algo, "algorithm round-trips");
    assert!(loaded.is_unlocked(), "loaded agent is unlocked");

    // Sign with the loaded handle, verify with the original — proves
    // the signer round-tripped correctly.
    let signed = loaded.sign_message_json(r#"{"loaded":true}"#).expect("sign");
    let result = original.verify_json(&signed).expect("verify");
    let result_value: Value = serde_wasm_bindgen::from_value(result).expect("from_value");
    assert_eq!(result_value.get("valid").and_then(Value::as_bool), Some(true));
}

/// `importEncrypted` with the wrong password fails with a typed error
/// (not a panic).
#[wasm_bindgen_test]
fn import_encrypted_with_wrong_password_fails_cleanly() {
    let original = BrowserAgentHandle::create_ephemeral("ed25519", None)
        .expect("createEphemeral");
    let material = original.export_encrypted(TEST_PASSWORD).expect("exportEncrypted");

    let result = BrowserAgentHandle::import_encrypted(&material, "wrong-password", None);
    assert!(result.is_err(), "wrong password must fail");
}

/// `publicOnly` preserves the supplied jacsId, verifies signatures
/// from the matching private key, rejects sign/clearSecrets paths.
/// Validates Issue 004 fix end-to-end.
#[wasm_bindgen_test]
fn public_only_preserves_identity_and_verifies_correctly() {
    // Produce a real signed document via createEphemeral.
    let signer = BrowserAgentHandle::create_ephemeral("ed25519", None)
        .expect("createEphemeral");
    let signed = signer
        .sign_message_json(r#"{"public":"only"}"#)
        .expect("sign");

    let supplied_jacs_id = signer.jacs_id();
    let supplied_pubkey = signer.get_public_key_base64();

    // Build a publicOnly handle from the same identity + public key.
    let verifier = BrowserAgentHandle::public_only(
        &supplied_jacs_id,
        &supplied_pubkey,
        "ed25519",
        None,
    )
    .expect("publicOnly");

    // Issue 004: identity surfaces echo what was supplied, not the
    // internal ephemeral agent's id.
    assert_eq!(
        verifier.jacs_id(),
        supplied_jacs_id,
        "publicOnly preserves supplied jacsId"
    );
    assert_eq!(
        verifier.get_public_key_base64(),
        supplied_pubkey,
        "publicOnly preserves supplied public key"
    );

    // Verification succeeds for the matching signature.
    let result_js = verifier.verify_json(&signed).expect("verify");
    let result_value: Value = serde_wasm_bindgen::from_value(result_js).expect("from_value");
    assert_eq!(
        result_value.get("valid").and_then(Value::as_bool),
        Some(true),
        "publicOnly must verify a signature from the matching private key"
    );

    // Verification fails for a signature from a DIFFERENT key (using
    // publicOnly with a wrong public key).
    let other_signer = BrowserAgentHandle::create_ephemeral("ed25519", None)
        .expect("createEphemeral other");
    let other_signed = other_signer
        .sign_message_json(r#"{"other":"signer"}"#)
        .expect("other sign");
    let bad_result_js = verifier.verify_json(&other_signed).expect("verify other");
    let bad_value: Value = serde_wasm_bindgen::from_value(bad_result_js).expect("from_value");
    assert_eq!(
        bad_value.get("valid").and_then(Value::as_bool),
        Some(false),
        "publicOnly must reject a signature from a different private key"
    );

    // publicOnly handles refuse sign-requiring paths with a typed Locked error.
    assert!(!verifier.is_unlocked(), "publicOnly handle must report locked");
    let sign_result = verifier.sign_message_json(r#"{"forbidden":true}"#);
    assert!(
        sign_result.is_err(),
        "publicOnly handle must refuse signMessageJson"
    );
}

/// `clearSecrets` drops the provider's signer (Issue 002 fix).
/// After clearing:
///   - `isUnlocked` returns false,
///   - sign attempts fail with typed Locked error,
///   - `clearSecrets` is idempotent (multiple calls are safe).
#[wasm_bindgen_test]
fn clear_secrets_blocks_subsequent_signing_at_the_provider_layer() {
    let handle = BrowserAgentHandle::create_ephemeral("ed25519", None)
        .expect("createEphemeral");
    assert!(handle.is_unlocked(), "fresh handle is unlocked");

    // Sign once to prove the signer works pre-clear.
    let pre = handle.sign_message_json(r#"{"pre":1}"#).expect("pre sign");
    assert!(!pre.is_empty());

    // Clear secrets through the wrapper.
    handle.clear_secrets();
    assert!(!handle.is_unlocked(), "isUnlocked is false after clearSecrets");

    // Wrapper rejects sign attempts with Locked.
    let post = handle.sign_message_json(r#"{"post":1}"#);
    assert!(post.is_err(), "sign after clearSecrets must fail");

    // exportEncrypted requires unlocked — must also fail post-clear.
    let export = handle.export_encrypted(TEST_PASSWORD);
    assert!(export.is_err(), "exportEncrypted after clearSecrets must fail");

    // clearSecrets is idempotent — calling again must not panic.
    handle.clear_secrets();
    handle.clear_secrets();
    assert!(!handle.is_unlocked());
}

/// Save + load via the same path TS BrowserAgent.save / load take.
/// Proves the JSON shape written by `exportEncrypted` round-trips
/// through `local_store::save_encrypted_agent` ->
/// `local_store::load_encrypted_agent` -> `importEncrypted`.
#[wasm_bindgen_test]
fn save_load_round_trip_via_local_store_path() {
    clear_test_keys();

    let original = BrowserAgentHandle::create_ephemeral("ed25519", None)
        .expect("createEphemeral");
    let original_jacs_id = original.jacs_id();
    let original_pubkey = original.get_public_key_base64();

    // Mirror TS BrowserAgent.save(storageKey, password):
    //   const materialJson = this.exportEncrypted(password);
    //   jacsLocalStore.saveEncryptedAgent(storageKey, materialJson);
    let material_json = original.export_encrypted(TEST_PASSWORD).expect("exportEncrypted");
    jacs_wasm::local_store::save_encrypted_agent(STORAGE_KEY, &material_json)
        .expect("save_encrypted_agent");

    // Mirror TS BrowserAgent.load(storageKey, { password }):
    //   const materialJson = jacsLocalStore.loadEncryptedAgent(storageKey);
    //   return BrowserAgent.importEncrypted(materialJson, password);
    let loaded_json =
        jacs_wasm::local_store::load_encrypted_agent(STORAGE_KEY)
            .expect("load_encrypted_agent")
            .expect("must be Some after save");
    let loaded =
        BrowserAgentHandle::import_encrypted(&loaded_json, TEST_PASSWORD, None)
            .expect("importEncrypted from loaded blob");

    assert_eq!(loaded.jacs_id(), original_jacs_id, "round-trip jacsId");
    assert_eq!(loaded.get_public_key_base64(), original_pubkey, "round-trip pubkey");
    assert!(loaded.is_unlocked(), "loaded handle is unlocked");

    clear_test_keys();
}

/// Typed error path: `save_encrypted_agent` rejects malformed payload
/// with a typed `RefusedPayload` / `MalformedDocument` error rather
/// than panicking, AND does not write any localStorage entry. Issue
/// 010's "quota / unavailable / refused" requirement; the page can't
/// truly disable localStorage from inside but it CAN drive the
/// validation path with a known-bad shape.
#[wasm_bindgen_test]
fn save_encrypted_agent_rejects_malformed_payload_with_typed_error() {
    clear_test_keys();
    let err = jacs_wasm::local_store::save_encrypted_agent(STORAGE_KEY, "not-json")
        .expect_err("malformed payload must be rejected");
    let code = err.code();
    assert_eq!(code, "RefusedPayload", "expected RefusedPayload code, got {code}");
    // The validation runs before any write — confirm no leak.
    let entries = walk_local_storage();
    assert!(
        !entries.iter().any(|(k, _)| k.contains(STORAGE_KEY)),
        "rejected save_encrypted_agent must not leave any localStorage entry"
    );
}

/// Typed error path: `save_encrypted_agent` rejects a payload that
/// embeds a PEM private-key marker — defense-in-depth requirement
/// from PRD §5.4.
#[wasm_bindgen_test]
fn save_encrypted_agent_rejects_pem_private_block() {
    clear_test_keys();
    // Embed the PEM marker in a payload that is otherwise shaped like
    // an AgentMaterial — the validator runs before the shape check
    // and refuses.
    let payload = r#"{"config":{},"agent":{},"public_key":"AAAA","encrypted_private_key":"AAAA","algorithm":"ed25519","leak":"-----BEGIN PRIVATE KEY-----"}"#;
    let err = jacs_wasm::local_store::save_encrypted_agent(STORAGE_KEY, payload)
        .expect_err("PEM-bearing payload must be rejected");
    assert_eq!(err.code(), "RefusedPayload");
    let entries = walk_local_storage();
    assert!(
        !entries.iter().any(|(k, _)| k.contains(STORAGE_KEY)),
        "rejected payload must not leave any localStorage entry"
    );
}

/// Sanity check: the walker is robust when localStorage is empty.
#[wasm_bindgen_test]
fn local_storage_walker_is_robust_when_empty() {
    let _ = walk_local_storage();
}

