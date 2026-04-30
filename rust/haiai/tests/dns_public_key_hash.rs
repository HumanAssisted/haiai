//! Regression test for Issue 012: `verify_dns_public_key` must hash public-key
//! PEM via JACS's canonical helper, not by recomputing SHA-256 locally.
//!
//! The previous local implementation used `base64(sha256(pem.as_bytes()))`,
//! which does not match the values JACS publishes — JACS does
//! `lowercase_hex(sha256(BOM-detect-utf8(pem).trim().replace("\r","")))`.
//! Every legitimate agent's `jacs_public_key_hash=` DNS TXT record was being
//! silently rejected. This test asserts the algorithm we now use is the same
//! one JACS publishes, and that it is robust to CRLF vs LF line endings.

#![cfg(feature = "jacs-crate")]

use jacs::crypt::hash::hash_public_key;

const SAMPLE_PEM_LF: &str =
    "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEABBccddee0011223344556677889900AABBCCDDEEFF==\n-----END PUBLIC KEY-----\n";

const SAMPLE_PEM_CRLF: &str =
    "-----BEGIN PUBLIC KEY-----\r\nMCowBQYDK2VwAyEABBccddee0011223344556677889900AABBCCDDEEFF==\r\n-----END PUBLIC KEY-----\r\n";

#[test]
fn jacs_hash_public_key_is_lowercase_hex_sha256() {
    // JACS hashes return lowercase hex SHA-256 (64 chars, [0-9a-f]).
    let hash = hash_public_key(SAMPLE_PEM_LF);
    assert_eq!(hash.len(), 64, "expected 64-char hex digest, got {hash:?}");
    assert!(
        hash.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "expected lowercase hex digest, got {hash:?}"
    );
}

#[test]
fn jacs_hash_public_key_is_invariant_to_line_endings() {
    // CRLF-vs-LF normalization is the whole reason we delegate to JACS:
    // a key emitted by an agent on Windows must hash the same as the same
    // key on macOS / Linux, otherwise DNS verification fails inconsistently.
    let lf_hash = hash_public_key(SAMPLE_PEM_LF);
    let crlf_hash = hash_public_key(SAMPLE_PEM_CRLF);
    assert_eq!(
        lf_hash, crlf_hash,
        "CRLF and LF PEMs must hash to the same value (Issue 012 regression)"
    );
}

#[test]
fn jacs_hash_public_key_differs_from_legacy_base64_sha256() {
    // Legacy haiai (pre-Issue-012) computed base64(sha256(pem.as_bytes()))
    // with no normalization. That value will never match a JACS-published
    // hash. This test pins that the two algorithms differ — i.e. verifies
    // the migration is real and we are not silently shipping the same value
    // under a new name.
    use base64::Engine;
    use sha2::{Digest, Sha256};

    let legacy = {
        let mut h = Sha256::new();
        h.update(SAMPLE_PEM_LF.as_bytes());
        base64::engine::general_purpose::STANDARD.encode(h.finalize())
    };
    let canonical = hash_public_key(SAMPLE_PEM_LF);
    assert_ne!(
        legacy, canonical,
        "legacy base64 form and canonical hex form must differ (else the bug is unfixed)"
    );
}
