#![cfg(feature = "jacs-crate")]

use base64::Engine;
use haiai::{
    CreateAgentOptions, HaiClient, HaiClientOptions, JacsMediaProvider, LocalJacsProvider,
    MediaVerifyStatus, StaticJacsProvider, TextSignatureStatus, VerifyImageOptions,
    VerifyTextOptions, VerifyTextResult,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
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
    nonce: String,
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
    let parts: Vec<&str> = token.splitn(4, ':').collect();

    assert_eq!(fixture.auth_header.scheme, "JACS");
    assert_eq!(
        fixture.auth_header.parts,
        vec!["jacs_id", "timestamp", "nonce", "signature_base64"]
    );
    assert_eq!(parts.len(), 4);
    assert_eq!(parts[0], fixture.auth_header.example.jacs_id);
    assert_eq!(
        fixture.auth_header.signed_message_template,
        "{jacs_id}:{timestamp}:{nonce}"
    );

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(parts[3])
        .expect("decode static provider signature");
    let signed_message = String::from_utf8(decoded).expect("utf8 signature payload");
    assert_eq!(
        signed_message,
        format!("sig:{}:{}:{}", parts[0], parts[1], parts[2])
    );

    let parsed_timestamp = parts[1].parse::<i64>().expect("timestamp");
    assert!(
        parsed_timestamp >= fixture.auth_header.example.timestamp,
        "timestamp should be unix seconds"
    );
    assert!(
        !fixture.auth_header.example.nonce.is_empty(),
        "fixture should include an example nonce"
    );
    assert!(!parts[2].is_empty(), "nonce should be present");
}

// ===========================================================================
// TASK_011: cross-language verify-parity over `fixtures/media/signed.*`.
//
// Each test creates a fresh signed verifier agent, stages only the shared
// fixture signer's public key in an explicit key directory, reads the
// committed signed fixture from disk, and asserts that
// `LocalJacsProvider::{verify_image, verify_text_file}` returns Valid for
// the unmodified bytes and HashMismatch when one content byte is flipped.
//
// The same byte sequences are exercised by the Python / Node / Go
// counterparts (`test_cross_lang_media.py`, `cross-lang-media.test.ts`,
// `cross_lang_media_test.go`). Any drift between languages MUST be a
// parity bug, not a test-only quirk — that is the entire point of this
// suite (PRD §5.5).
// ===========================================================================

/// Serialise verifier creation and loads. JACS reads
/// `JACS_PRIVATE_KEY_PASSWORD` at load time and the test runner is
/// multi-threaded by default.
static MEDIA_PARITY_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Deserialize)]
struct SignerFixture {
    signer_id: String,
    algorithm: String,
    public_key_file: String,
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

/// Create a fresh signed verifier and give it only the committed fixture
/// signer's public key through JACS' explicit `key_dir` resolution contract.
/// The legacy fixture agent config remains unsigned and is never loaded.
fn stage_media_verifier(signer: &SignerFixture) -> (tempfile::TempDir, LocalJacsProvider, PathBuf) {
    const PASSWORD: &str = "MediaParityTestPass!123";
    std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", PASSWORD);

    let temp = tempfile::tempdir().expect("tempdir");
    let temp_root = temp.path().canonicalize().expect("canonical tempdir");
    let config = temp_root.join("jacs.config.json");
    let data_dir = temp_root.join("data");
    let agent_key_dir = temp_root.join("agent-keys");
    LocalJacsProvider::create_agent_with_options(&CreateAgentOptions {
        name: "cross-language-media-verifier".to_string(),
        password: PASSWORD.to_string(),
        algorithm: Some("ed25519".to_string()),
        data_directory: Some(data_dir.to_string_lossy().into_owned()),
        key_directory: Some(agent_key_dir.to_string_lossy().into_owned()),
        config_path: Some(config.to_string_lossy().into_owned()),
        agent_type: Some("ai".to_string()),
        description: Some("Cross-language media fixture verifier".to_string()),
        domain: None,
        default_storage: Some("fs".to_string()),
    })
    .expect("create signed verifier agent");

    let verification_key_dir = temp_root.join("verification-keys");
    fs::create_dir_all(&verification_key_dir).expect("create verification key dir");
    let source_public_key = repo_root().join(&signer.public_key_file);
    assert!(
        source_public_key.is_file(),
        "fixture public key must exist at {}",
        source_public_key.display()
    );
    let encoded_signer_id =
        jacs::simple::advanced::encode_signer_id_for_filename(&signer.signer_id);
    fs::copy(
        source_public_key,
        verification_key_dir.join(format!("{encoded_signer_id}.public.pem")),
    )
    .expect("stage fixture signer public key");

    let provider = LocalJacsProvider::from_config_path(Some(&config), None)
        .expect("load signed verifier agent");
    (temp, provider, verification_key_dir)
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
fn tamper_text_body(bytes: &mut [u8]) {
    const MARKER: &[u8] = b"-----BEGIN JACS SIGNATURE-----";
    let body_end = bytes
        .windows(MARKER.len())
        .position(|w| w == MARKER)
        .expect("BEGIN marker present in signed.md");
    // Walk back to a printable ASCII byte and toggle case.
    let i = (0..body_end)
        .rfind(|&i| bytes[i].is_ascii_alphabetic())
        .unwrap_or(0);
    bytes[i] ^= 0b0010_0000;
}

fn assert_image_valid(name: &str, signer: &SignerFixture) {
    let _lock = MEDIA_PARITY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let (_temp, provider, key_dir) = stage_media_verifier(signer);

    let work = tempfile::tempdir().expect("tempdir");
    let work_root = work.path().canonicalize().expect("canonical workdir");
    let staged = work_root.join(name);
    fs::write(&staged, read_fixture_with_checksum(name)).expect("stage signed image");

    let result = provider
        .verify_image(
            staged.to_str().unwrap(),
            VerifyImageOptions {
                base: VerifyTextOptions {
                    strict: false,
                    key_dir: Some(key_dir),
                },
                ..VerifyImageOptions::default()
            },
        )
        .expect("verify_image");
    assert_eq!(
        result.status,
        MediaVerifyStatus::Valid,
        "expected Valid for {name}, got {:?}",
        result.status
    );
    assert_eq!(
        result.signer_id.as_deref(),
        Some(signer.signer_id.as_str()),
        "signer mismatch for {name}",
    );
}

fn assert_image_tampered(name: &str, marker: &[u8], offset: usize) {
    let _lock = MEDIA_PARITY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let signer = load_signer_fixture();
    let (_temp, provider, key_dir) = stage_media_verifier(&signer);

    let work = tempfile::tempdir().expect("tempdir");
    let work_root = work.path().canonicalize().expect("canonical workdir");
    let staged = work_root.join(name);
    let mut bytes = read_fixture_with_checksum(name);
    tamper_after(&mut bytes, marker, offset);
    fs::write(&staged, &bytes).expect("write tampered");

    let result = provider
        .verify_image(
            staged.to_str().unwrap(),
            VerifyImageOptions {
                base: VerifyTextOptions {
                    strict: false,
                    key_dir: Some(key_dir),
                },
                ..VerifyImageOptions::default()
            },
        )
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
    assert_image_valid("signed.png", &signer);
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
    assert_image_valid("signed.jpg", &signer);
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
    assert_image_valid("signed.webp", &signer);
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
    let (_temp, provider, key_dir) = stage_media_verifier(&signer);

    let work = tempfile::tempdir().expect("tempdir");
    let work_root = work.path().canonicalize().expect("canonical workdir");
    let staged = work_root.join("signed.md");
    fs::write(&staged, read_fixture_with_checksum("signed.md")).expect("stage signed.md");

    let result = provider
        .verify_text_file(
            staged.to_str().unwrap(),
            VerifyTextOptions {
                strict: false,
                key_dir: Some(key_dir),
            },
        )
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
    let signer = load_signer_fixture();
    let (_temp, provider, key_dir) = stage_media_verifier(&signer);

    let work = tempfile::tempdir().expect("tempdir");
    let work_root = work.path().canonicalize().expect("canonical workdir");
    let staged = work_root.join("signed.md");
    let mut bytes = read_fixture_with_checksum("signed.md");
    tamper_text_body(&mut bytes);
    fs::write(&staged, &bytes).expect("write tampered");

    let result = provider
        .verify_text_file(
            staged.to_str().unwrap(),
            VerifyTextOptions {
                strict: false,
                key_dir: Some(key_dir),
            },
        )
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
