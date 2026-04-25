#![cfg(feature = "jacs-crate")]

use base64::Engine;
use haiai::{
    HaiClient, HaiClientOptions, JacsMediaProvider, LocalJacsProvider, MediaVerifyStatus,
    StaticJacsProvider, TextSignatureStatus, VerifyImageOptions, VerifyTextOptions,
    VerifyTextResult,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Deserialize)]
struct CrossLangFixture {
    auth_header: AuthHeaderFixture,
    canonical_json_cases: Vec<CanonicalJsonCase>,
}

#[derive(Debug, Deserialize)]
struct AuthHeaderFixture {
    scheme: String,
    parts: Vec<String>,
    signed_message_template: String,
    example: AuthHeaderExample,
}

#[derive(Debug, Deserialize)]
struct AuthHeaderExample {
    jacs_id: String,
    timestamp: i64,
}

#[derive(Debug, Deserialize)]
struct CanonicalJsonCase {
    name: String,
    input: Value,
    expected: String,
}

fn load_fixture() -> CrossLangFixture {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/cross_lang_test.json");
    let raw = fs::read_to_string(path).expect("read cross_lang_test fixture");
    serde_json::from_str(&raw).expect("decode cross_lang_test fixture")
}

#[test]
fn canonical_json_matches_shared_cases() {
    let fixture = load_fixture();
    let client = HaiClient::new(
        StaticJacsProvider::new("fixture-agent"),
        HaiClientOptions::default(),
    )
    .expect("client");

    for case in fixture.canonical_json_cases {
        let got = client.canonical_json(&case.input).expect("canonical json");
        assert_eq!(got, case.expected, "case {}", case.name);
    }
}

#[test]
fn auth_header_matches_shared_shape() {
    let fixture = load_fixture();
    let client = HaiClient::new(
        StaticJacsProvider::new(fixture.auth_header.example.jacs_id.clone()),
        HaiClientOptions::default(),
    )
    .expect("client");

    let header = client.build_auth_header().expect("auth header");
    let token = header.strip_prefix("JACS ").expect("auth header prefix");
    let parts: Vec<&str> = token.splitn(3, ':').collect();

    assert_eq!(fixture.auth_header.scheme, "JACS");
    assert_eq!(
        fixture.auth_header.parts,
        vec!["jacs_id", "timestamp", "signature_base64"]
    );
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], fixture.auth_header.example.jacs_id);
    assert_eq!(
        fixture.auth_header.signed_message_template,
        "{jacs_id}:{timestamp}"
    );

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(parts[2])
        .expect("decode static provider signature");
    let signed_message = String::from_utf8(decoded).expect("utf8 signature payload");
    assert_eq!(signed_message, format!("sig:{}:{}", parts[0], parts[1]));

    let parsed_timestamp = parts[1].parse::<i64>().expect("timestamp");
    assert!(
        parsed_timestamp >= fixture.auth_header.example.timestamp,
        "timestamp should be unix seconds"
    );
}

// ===========================================================================
// TASK_011: cross-language verify-parity over `fixtures/media/signed.*`.
//
// Each test stages the shared `fixtures/jacs-agent/` agent in a tempdir,
// reads the committed signed fixture from disk, and asserts that
// `LocalJacsProvider::{verify_image, verify_text_file}` returns Valid for
// the unmodified bytes and HashMismatch when one content byte is flipped.
//
// The same byte sequences are exercised by the Python / Node / Go
// counterparts (`test_cross_lang_media.py`, `cross-lang-media.test.ts`,
// `cross_lang_media_test.go`). Any drift between languages MUST be a
// parity bug, not a test-only quirk — that is the entire point of this
// suite (PRD §5.5).
// ===========================================================================

/// Serialise fixture-agent loads. JACS reads `JACS_PRIVATE_KEY_PASSWORD`
/// at load time and the test runner is multi-threaded by default.
static MEDIA_PARITY_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Deserialize)]
struct SignerFixture {
    signer_id: String,
    algorithm: String,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn load_signer_fixture() -> SignerFixture {
    let path = repo_root().join("fixtures/media/SIGNER.json");
    let raw = fs::read_to_string(&path).expect("fixtures/media/SIGNER.json must exist");
    serde_json::from_str(&raw).expect("decode SIGNER.json")
}

fn copy_fixture_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("create dst dir");
    for entry in fs::read_dir(src).expect("read src dir") {
        let entry = entry.expect("dir entry");
        let src_path = entry.path();
        let name = entry.file_name().to_string_lossy().replace('_', ":");
        let dst_path = dst.join(&name);
        if src_path.is_dir() {
            copy_fixture_dir(&src_path, &dst_path);
        } else {
            fs::copy(&src_path, &dst_path).expect("copy file");
        }
    }
}

/// Stage `fixtures/jacs-agent/` in a tempdir with `_` → `:` filename mapping.
/// Returns (TempDir, staged_config_path). Sets `JACS_PRIVATE_KEY_PASSWORD`.
fn stage_fixture_agent() -> (tempfile::TempDir, PathBuf) {
    std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", "secretpassord");

    let source = repo_root().join("fixtures/jacs-agent/jacs.config.json");
    assert!(
        source.exists(),
        "fixtures/jacs-agent/jacs.config.json must exist"
    );
    let source_dir = source.parent().expect("fixture dir");
    let mut value: Value =
        serde_json::from_str(&fs::read_to_string(&source).expect("read fixture config"))
            .expect("parse fixture config");

    let temp = tempfile::tempdir().expect("tempdir");

    let src_keys =
        source_dir.join(value["jacs_key_directory"].as_str().unwrap_or("keys"));
    let tmp_keys = temp.path().join("keys");
    fs::create_dir_all(&tmp_keys).expect("mkdir keys");
    for entry in fs::read_dir(&src_keys).expect("read keys") {
        let entry = entry.expect("key entry");
        fs::copy(entry.path(), tmp_keys.join(entry.file_name())).expect("copy key");
    }

    let src_data = source_dir.join(value["jacs_data_directory"].as_str().unwrap_or("."));
    let tmp_data = temp.path().join("data");
    copy_fixture_dir(&src_data, &tmp_data);

    value["jacs_data_directory"] = Value::String(tmp_data.to_string_lossy().into_owned());
    value["jacs_key_directory"] = Value::String(tmp_keys.to_string_lossy().into_owned());

    let config = temp.path().join("jacs.config.json");
    fs::write(
        &config,
        serde_json::to_vec_pretty(&value).expect("encode config"),
    )
    .expect("write config");

    (temp, config)
}

fn checksum_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

fn read_fixture_with_checksum(name: &str) -> Vec<u8> {
    let bytes = fs::read(repo_root().join("fixtures/media").join(name))
        .unwrap_or_else(|e| panic!("read fixtures/media/{name}: {e}"));

    // Cross-check against fixtures/media/CHECKSUMS.txt so any byte drift
    // (e.g. accidental regenerator run) trips immediately.
    let manifest = fs::read_to_string(repo_root().join("fixtures/media/CHECKSUMS.txt"))
        .expect("read CHECKSUMS.txt");
    let expected = manifest
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let hex = parts.next()?;
            let file = parts.next()?;
            (file == name).then(|| hex.to_string())
        })
        .unwrap_or_else(|| panic!("no checksum for {name} in CHECKSUMS.txt"));
    let got = checksum_hex(&bytes);
    assert_eq!(got, expected, "checksum drift on fixtures/media/{name}");
    bytes
}

/// Locate a marker in `bytes` and flip one bit at `marker_idx + offset`.
/// Used to mutate image content past the JACS metadata chunk.
fn tamper_after(bytes: &mut [u8], marker: &[u8], offset: usize) {
    let idx = bytes
        .windows(marker.len())
        .position(|w| w == marker)
        .unwrap_or_else(|| panic!("marker {marker:?} not found"));
    let target = idx + marker.len() + offset;
    bytes[target] ^= 0x01;
}

/// For a signed markdown file, mutate one body byte BEFORE the
/// `-----BEGIN JACS SIGNATURE-----` block so verify reports HashMismatch
/// rather than Malformed.
fn tamper_text_body(bytes: &mut Vec<u8>) {
    const MARKER: &[u8] = b"-----BEGIN JACS SIGNATURE-----";
    let body_end = bytes
        .windows(MARKER.len())
        .position(|w| w == MARKER)
        .expect("BEGIN marker present in signed.md");
    // Walk back to a printable ASCII byte and toggle case.
    let i = (0..body_end).rfind(|&i| bytes[i].is_ascii_alphabetic()).unwrap_or(0);
    bytes[i] ^= 0b0010_0000;
}

fn assert_image_valid(name: &str, expected_signer: &str) {
    let _lock = MEDIA_PARITY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (_temp, config) = stage_fixture_agent();
    let provider =
        LocalJacsProvider::from_config_path(Some(&config), None).expect("load fixture agent");

    let work = tempfile::tempdir().expect("tempdir");
    let staged = work.path().join(name);
    fs::write(&staged, read_fixture_with_checksum(name)).expect("stage signed image");

    let result = provider
        .verify_image(staged.to_str().unwrap(), VerifyImageOptions::default())
        .expect("verify_image");
    assert_eq!(
        result.status,
        MediaVerifyStatus::Valid,
        "expected Valid for {name}, got {:?}",
        result.status
    );
    assert_eq!(
        result.signer_id.as_deref(),
        Some(expected_signer),
        "signer mismatch for {name}",
    );
}

fn assert_image_tampered(name: &str, marker: &[u8], offset: usize) {
    let _lock = MEDIA_PARITY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (_temp, config) = stage_fixture_agent();
    let provider =
        LocalJacsProvider::from_config_path(Some(&config), None).expect("load fixture agent");

    let work = tempfile::tempdir().expect("tempdir");
    let staged = work.path().join(name);
    let mut bytes = read_fixture_with_checksum(name);
    tamper_after(&mut bytes, marker, offset);
    fs::write(&staged, &bytes).expect("write tampered");

    let result = provider
        .verify_image(staged.to_str().unwrap(), VerifyImageOptions::default())
        .expect("verify_image");
    assert_eq!(
        result.status,
        MediaVerifyStatus::HashMismatch,
        "expected HashMismatch after tampering {name}, got {:?}",
        result.status
    );
}

#[test]
fn cross_lang_signed_image_png_verifies() {
    let signer = load_signer_fixture();
    assert_eq!(signer.algorithm, "pq2025", "fixture algorithm baseline");
    assert_image_valid("signed.png", &signer.signer_id);
}

#[test]
fn cross_lang_signed_image_png_tampered_returns_hash_mismatch() {
    // PNG: flip a byte well inside the IDAT chunk's compressed data
    // (well past the chunk length + chunk-type field, definitely not in
    // the iTXt/jacsSignature metadata chunk).
    assert_image_tampered("signed.png", b"IDAT", 6);
}

#[test]
fn cross_lang_signed_image_jpeg_verifies() {
    let signer = load_signer_fixture();
    assert_image_valid("signed.jpg", &signer.signer_id);
}

#[test]
fn cross_lang_signed_image_jpeg_tampered_returns_hash_mismatch() {
    // JPEG: flip a byte just after the SOS (Start-Of-Scan) marker. SOS is
    // 0xFFDA, which marks the beginning of compressed entropy data —
    // unrelated to APP11 (where JACS embeds).
    assert_image_tampered("signed.jpg", &[0xFF, 0xDA], 4);
}

#[test]
fn cross_lang_signed_image_webp_verifies() {
    let signer = load_signer_fixture();
    assert_image_valid("signed.webp", &signer.signer_id);
}

#[test]
fn cross_lang_signed_image_webp_tampered_returns_hash_mismatch() {
    // WebP: flip a byte inside the VP8L chunk body (past the 4-byte chunk
    // size). The XMP chunk holding the JACS payload is appended after the
    // VP8L chunk, so we are not corrupting metadata.
    assert_image_tampered("signed.webp", b"VP8L", 4);
}

#[test]
fn cross_lang_signed_text_md_verifies() {
    let signer = load_signer_fixture();
    let _lock = MEDIA_PARITY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (_temp, config) = stage_fixture_agent();
    let provider =
        LocalJacsProvider::from_config_path(Some(&config), None).expect("load fixture agent");

    let work = tempfile::tempdir().expect("tempdir");
    let staged = work.path().join("signed.md");
    fs::write(&staged, read_fixture_with_checksum("signed.md")).expect("stage signed.md");

    let result = provider
        .verify_text_file(staged.to_str().unwrap(), VerifyTextOptions::default())
        .expect("verify_text_file");
    match result {
        VerifyTextResult::Signed { signatures } => {
            assert_eq!(signatures.len(), 1, "exactly one signature block");
            assert_eq!(
                signatures[0].status,
                TextSignatureStatus::Valid,
                "signature status must be Valid"
            );
            assert_eq!(
                signatures[0].signer_id, signer.signer_id,
                "signer mismatch on signed.md"
            );
        }
        other => panic!("expected Signed variant, got {other:?}"),
    }
}

#[test]
fn cross_lang_signed_text_md_tampered_returns_hash_mismatch() {
    let _lock = MEDIA_PARITY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (_temp, config) = stage_fixture_agent();
    let provider =
        LocalJacsProvider::from_config_path(Some(&config), None).expect("load fixture agent");

    let work = tempfile::tempdir().expect("tempdir");
    let staged = work.path().join("signed.md");
    let mut bytes = read_fixture_with_checksum("signed.md");
    tamper_text_body(&mut bytes);
    fs::write(&staged, &bytes).expect("write tampered");

    let result = provider
        .verify_text_file(staged.to_str().unwrap(), VerifyTextOptions::default())
        .expect("verify_text_file");
    match result {
        VerifyTextResult::Signed { signatures } => {
            assert_eq!(signatures.len(), 1);
            assert_eq!(
                signatures[0].status,
                TextSignatureStatus::HashMismatch,
                "expected HashMismatch after body tampering, got {:?}",
                signatures[0].status,
            );
        }
        other => panic!("expected Signed (with HashMismatch), got {other:?}"),
    }
}

