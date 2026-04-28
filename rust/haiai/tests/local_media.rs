//! TASK_001 unit tests: `JacsMediaProvider` impl on `LocalJacsProvider`.
//!
//! Each test creates an isolated agent under `target/local-media-<uuid>/`,
//! signs a tempfile, and verifies the round-trip end-to-end. WebP fixtures
//! are hand-built RIFF containers (no external WebP encoder).

#![cfg(feature = "jacs-crate")]

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use haiai::{
    CreateAgentOptions, JacsMediaProvider, LocalJacsProvider, MediaVerifyStatus, SignImageOptions,
    SignTextOptions, VerifyImageOptions, VerifyTextOptions, VerifyTextResult,
};
use uuid::Uuid;

static MEDIA_TEST_LOCK: Mutex<()> = Mutex::new(());

struct AgentEnv {
    base: PathBuf,
    original_cwd: PathBuf,
    config_path: PathBuf,
}

impl AgentEnv {
    fn new() -> Self {
        let original_cwd = std::env::current_dir().expect("cwd");
        let base = original_cwd.join(format!("target/local-media-{}", Uuid::new_v4()));
        fs::create_dir_all(&base).expect("create base");
        std::env::set_current_dir(&base).expect("cd to base");

        let options = CreateAgentOptions {
            name: "local-media-agent".to_string(),
            password: "TestPass!123".to_string(),
            algorithm: Some("pq2025".to_string()),
            data_directory: Some("data".to_string()),
            key_directory: Some("keys".to_string()),
            config_path: Some("jacs.config.json".to_string()),
            agent_type: Some("ai".to_string()),
            description: Some("Local media test agent".to_string()),
            domain: None,
            default_storage: Some("fs".to_string()),
        };
        LocalJacsProvider::create_agent_with_options(&options).expect("create agent");
        std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", "TestPass!123");

        let config_path = base.join("jacs.config.json");
        Self {
            base,
            original_cwd,
            config_path,
        }
    }

    fn provider(&self) -> LocalJacsProvider {
        LocalJacsProvider::from_config_path(Some(&self.config_path), None).expect("provider")
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let p = self.base.join(name);
        fs::write(&p, bytes).expect("write fixture");
        p
    }
}

impl Drop for AgentEnv {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.original_cwd);
    }
}

// ---------------------------------------------------------------------------
// Image fixtures
// ---------------------------------------------------------------------------

fn make_png(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(width, height, image::Rgba([32, 64, 128, 255]));
    let mut buf = Vec::new();
    let mut cur = std::io::Cursor::new(&mut buf);
    img.write_to(&mut cur, image::ImageFormat::Png).expect("png encode");
    buf
}

fn make_jpeg(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(width, height, image::Rgb([200, 150, 100]));
    let mut buf = Vec::new();
    let mut cur = std::io::Cursor::new(&mut buf);
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cur, 95);
    img.write_with_encoder(encoder).expect("jpeg encode");
    buf
}

/// Minimal valid WebP RIFF container — chunk-level only. Matches the JACS
/// reference fixture in `jacs/tests/image_signature_tests.rs`.
fn make_webp() -> Vec<u8> {
    fn build_chunk(fourcc: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + body.len() + 1);
        out.extend_from_slice(fourcc);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        if body.len() % 2 == 1 {
            out.push(0);
        }
        out
    }
    let body = vec![0u8; 4];
    let mut chunks = Vec::new();
    chunks.extend_from_slice(b"WEBP");
    chunks.extend_from_slice(&build_chunk(b"VP8L", &body));
    let riff_size = chunks.len() as u32;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(&chunks);
    out
}

// ---------------------------------------------------------------------------
// Inline-text round-trip
// ---------------------------------------------------------------------------

#[test]
fn local_provider_sign_text_round_trip() {
    let _lock = MEDIA_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let env = AgentEnv::new();
    let path = env.write("hello.md", b"# Hello\n");
    let provider = env.provider();

    let outcome = provider
        .sign_text_file(path.to_str().unwrap(), SignTextOptions::default())
        .expect("sign_text_file");
    assert_eq!(outcome.signers_added, 1);

    let result = provider
        .verify_text_file(path.to_str().unwrap(), VerifyTextOptions::default())
        .expect("verify_text_file");
    match result {
        VerifyTextResult::Signed { signatures } => {
            assert_eq!(signatures.len(), 1);
            assert_eq!(
                signatures[0].status,
                haiai::TextSignatureStatus::Valid,
                "signature should be Valid"
            );
        }
        other => panic!("expected Signed variant, got {other:?}"),
    }
}

#[test]
fn local_provider_sign_text_tampered_returns_hash_mismatch() {
    let _lock = MEDIA_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let env = AgentEnv::new();
    let path = env.write("tamper.md", b"# Hello\n");
    let provider = env.provider();

    provider
        .sign_text_file(path.to_str().unwrap(), SignTextOptions::default())
        .expect("sign");

    // Mutate one byte of the body region (before the signature block).
    let raw = fs::read_to_string(&path).expect("read signed");
    let marker = "-----BEGIN JACS SIGNATURE-----";
    let idx = raw.find(marker).expect("marker present");
    let mut body: Vec<u8> = raw.as_bytes()[..idx].to_vec();
    if let Some(byte) = body.iter_mut().find(|b| **b == b'H') {
        *byte = b'h';
    } else {
        body[0] ^= 0x01;
    }
    let mut tampered = body;
    tampered.extend_from_slice(&raw.as_bytes()[idx..]);
    fs::write(&path, &tampered).expect("write tampered");

    let result = provider
        .verify_text_file(path.to_str().unwrap(), VerifyTextOptions::default())
        .expect("verify_text_file");
    match result {
        VerifyTextResult::Signed { signatures } => {
            assert_eq!(signatures.len(), 1);
            assert_eq!(
                signatures[0].status,
                haiai::TextSignatureStatus::HashMismatch,
                "expected HashMismatch after tampering, got {:?}",
                signatures[0].status
            );
        }
        other => panic!("expected Signed (with HashMismatch), got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Image round-trip — PNG / JPEG / WebP
// ---------------------------------------------------------------------------

fn assert_image_round_trip(env: &AgentEnv, in_name: &str, out_name: &str, bytes: &[u8]) {
    let in_path = env.write(in_name, bytes);
    let out_path = env.base.join(out_name);
    let provider = env.provider();

    let signed = provider
        .sign_image(
            in_path.to_str().unwrap(),
            out_path.to_str().unwrap(),
            SignImageOptions::default(),
        )
        .expect("sign_image");
    assert!(!signed.signer_id.is_empty());
    assert!(out_path.exists());

    let result = provider
        .verify_image(
            out_path.to_str().unwrap(),
            VerifyImageOptions::default(),
        )
        .expect("verify_image");
    assert_eq!(result.status, MediaVerifyStatus::Valid, "expected Valid, got {:?}", result.status);
    assert_eq!(result.signer_id.as_deref(), Some(signed.signer_id.as_str()));
}

#[test]
fn local_provider_sign_image_png_round_trip() {
    let _lock = MEDIA_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let env = AgentEnv::new();
    let png = make_png(32, 32);
    assert_image_round_trip(&env, "in.png", "out.png", &png);
}

#[test]
fn local_provider_sign_image_jpeg_round_trip() {
    let _lock = MEDIA_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let env = AgentEnv::new();
    let jpg = make_jpeg(32, 32);
    assert_image_round_trip(&env, "in.jpg", "out.jpg", &jpg);
}

#[test]
fn local_provider_sign_image_webp_round_trip() {
    let _lock = MEDIA_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let env = AgentEnv::new();
    let webp = make_webp();
    assert_image_round_trip(&env, "in.webp", "out.webp", &webp);
}

#[test]
fn local_provider_sign_image_robust_round_trip() {
    let _lock = MEDIA_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let env = AgentEnv::new();
    // Robust LSB embedding uses 1 bit per pixel (alpha channel for PNG). The
    // JACS payload for pq2025 keys is ≈10 KB ≈ 80,000 bits. 320×320 carries
    // 102,400 bits — safe headroom.
    let in_path = env.write("in_robust.png", &make_png(320, 320));
    let out_path = env.base.join("out_robust.png");
    let provider = env.provider();

    let opts = SignImageOptions {
        robust: true,
        ..SignImageOptions::default()
    };
    let signed = provider
        .sign_image(
            in_path.to_str().unwrap(),
            out_path.to_str().unwrap(),
            opts,
        )
        .expect("sign_image robust");
    assert!(signed.robust);

    let verify_opts = VerifyImageOptions {
        scan_robust: true,
        ..VerifyImageOptions::default()
    };
    let result = provider
        .verify_image(out_path.to_str().unwrap(), verify_opts)
        .expect("verify_image robust");
    assert_eq!(result.status, MediaVerifyStatus::Valid);
    assert_eq!(
        result.embedding_channels.as_deref(),
        Some("metadata+lsb"),
        "robust mode must report metadata+lsb"
    );
}

#[test]
fn local_provider_sign_image_webp_robust_returns_unsupported() {
    let _lock = MEDIA_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let env = AgentEnv::new();
    let in_path = env.write("in_robust.webp", &make_webp());
    let out_path = env.base.join("out_robust.webp");
    let provider = env.provider();

    let opts = SignImageOptions {
        robust: true,
        ..SignImageOptions::default()
    };
    let err = provider
        .sign_image(
            in_path.to_str().unwrap(),
            out_path.to_str().unwrap(),
            opts,
        )
        .expect_err("WebP + robust must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("webp robust mode deferred") || msg.to_ascii_lowercase().contains("unsupported"),
        "expected JACS verbatim 'webp robust mode deferred' or unsupported error, got: {msg}"
    );
}

// ---------------------------------------------------------------------------
// extract_media_signature
// ---------------------------------------------------------------------------

#[test]
fn local_provider_extract_media_signature_returns_decoded_json() {
    let _lock = MEDIA_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let env = AgentEnv::new();
    let in_path = env.write("ex.png", &make_png(32, 32));
    let out_path = env.base.join("ex_signed.png");
    let provider = env.provider();

    provider
        .sign_image(
            in_path.to_str().unwrap(),
            out_path.to_str().unwrap(),
            SignImageOptions::default(),
        )
        .expect("sign");
    let payload = provider
        .extract_media_signature(out_path.to_str().unwrap(), false)
        .expect("extract")
        .expect("payload present");
    let parsed: serde_json::Value =
        serde_json::from_str(&payload).expect("decoded payload must be JSON");
    assert!(parsed.is_object(), "decoded payload should be a JSON object");
}

#[test]
fn local_provider_extract_media_signature_raw_returns_base64url() {
    use base64::Engine;
    let _lock = MEDIA_TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let env = AgentEnv::new();
    let in_path = env.write("raw.png", &make_png(32, 32));
    let out_path = env.base.join("raw_signed.png");
    let provider = env.provider();

    provider
        .sign_image(
            in_path.to_str().unwrap(),
            out_path.to_str().unwrap(),
            SignImageOptions::default(),
        )
        .expect("sign");
    let payload = provider
        .extract_media_signature(out_path.to_str().unwrap(), true)
        .expect("extract raw")
        .expect("payload present");
    let trimmed = payload.trim();
    assert!(!trimmed.is_empty());
    // Must decode under URL_SAFE_NO_PAD per JACS contract.
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(trimmed.as_bytes())
        .expect("raw payload must decode as base64url-no-pad");
}
