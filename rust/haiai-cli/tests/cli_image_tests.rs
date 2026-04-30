//! TASK_004 integration tests for `haiai sign-image` / `verify-image` /
//! `extract-media-signature`.
//!
//! Pattern: hand-rolled `Command::new(haiai_bin())` (no assert_cmd or escargot;
//! see TASK_004 PRD note). cwd is set to the JACS fixture tempdir; password
//! is supplied via `JACS_PRIVATE_KEY_PASSWORD`.

mod common;
use common::{make_jpeg, make_png, prepare_jacs_fixture, run_haiai_in_fixture};

fn write_to_fixture(fixture_dir: &std::path::Path, name: &str, bytes: &[u8]) {
    std::fs::write(fixture_dir.join(name), bytes).expect("write fixture file");
}

#[test]
fn cli_sign_image_png_round_trip() {
    let (temp, _config_path) = prepare_jacs_fixture();
    write_to_fixture(temp.path(), "in.png", &make_png(32, 32));

    let out = run_haiai_in_fixture(
        temp.path(),
        &["sign-image", "in.png", "--out", "signed.png"],
    );
    assert!(
        out.status.success(),
        "sign-image failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(temp.path().join("signed.png").exists());

    let out = run_haiai_in_fixture(temp.path(), &["verify-image", "signed.png"]);
    assert!(
        out.status.success(),
        "verify-image failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn cli_sign_image_jpeg_round_trip() {
    let (temp, _config_path) = prepare_jacs_fixture();
    write_to_fixture(temp.path(), "in.jpg", &make_jpeg(32, 32));

    let out = run_haiai_in_fixture(
        temp.path(),
        &["sign-image", "in.jpg", "--out", "signed.jpg"],
    );
    assert!(
        out.status.success(),
        "sign-image failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out = run_haiai_in_fixture(temp.path(), &["verify-image", "signed.jpg"]);
    assert!(
        out.status.success(),
        "verify-image failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn cli_verify_image_strict_missing_signature_exits_1() {
    let (temp, _config_path) = prepare_jacs_fixture();
    write_to_fixture(temp.path(), "unsigned.png", &make_png(32, 32));

    let out = run_haiai_in_fixture(temp.path(), &["verify-image", "unsigned.png", "--strict"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "expected exit 1 (strict missing signature)"
    );
}

#[test]
fn cli_verify_image_permissive_missing_signature_exits_2() {
    let (temp, _config_path) = prepare_jacs_fixture();
    write_to_fixture(temp.path(), "unsigned.png", &make_png(32, 32));

    let out = run_haiai_in_fixture(temp.path(), &["verify-image", "unsigned.png"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit 2 (permissive missing)"
    );
}

#[test]
fn cli_verify_image_tampered_exits_1() {
    let (temp, _config_path) = prepare_jacs_fixture();
    write_to_fixture(temp.path(), "in.png", &make_png(32, 32));

    let out = run_haiai_in_fixture(
        temp.path(),
        &["sign-image", "in.png", "--out", "signed.png"],
    );
    assert!(out.status.success());

    // Mutate one byte inside the IDAT region (well past the iTXt signature
    // chunk). Using a brute-force scan for the IDAT marker.
    let path = temp.path().join("signed.png");
    let mut bytes = std::fs::read(&path).expect("read signed");
    let idat_idx = bytes
        .windows(4)
        .position(|w| w == b"IDAT")
        .expect("IDAT marker present");
    bytes[idat_idx + 6] ^= 0x01;
    std::fs::write(&path, &bytes).expect("write tampered");

    let out = run_haiai_in_fixture(temp.path(), &["verify-image", "signed.png"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "tampered image should exit 1; got: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn cli_verify_image_json_status_field_is_flat_string() {
    // Regression for Issue 013.1: `verify-image --json` previously serialized
    // the JACS struct directly, leaking the tagged `Malformed` shape. The
    // happy path must emit a flat snake_case `status` string matching the
    // binding-core/MCP/SDK envelope contract.
    let (temp, _config_path) = prepare_jacs_fixture();
    write_to_fixture(temp.path(), "in.png", &make_png(32, 32));
    let _ = run_haiai_in_fixture(
        temp.path(),
        &["sign-image", "in.png", "--out", "signed.png"],
    );

    let out = run_haiai_in_fixture(temp.path(), &["verify-image", "signed.png", "--json"]);
    assert!(
        out.status.success(),
        "verify-image --json failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("verify-image --json must emit valid JSON");
    assert!(
        parsed["status"].is_string(),
        "status must be a flat string, not a tagged object; got: {}",
        parsed
    );
    assert_eq!(parsed["status"].as_str(), Some("valid"));
    // Sibling fields documented by binding-core's `media_verify_result_to_json`.
    assert!(parsed.get("signer_id").is_some());
    assert!(parsed.get("algorithm").is_some());
    assert!(parsed.get("format").is_some());
}

#[test]
fn cli_verify_image_json_malformed_emits_flat_status_with_detail() {
    // Regression for Issue 013.1: when JACS returns
    // `MediaVerifyStatus::Malformed(detail)`, the CLI must NOT emit the
    // tagged `{"status": {"malformed": "<detail>"}}` shape. Instead the
    // envelope must be `{"status": "malformed", "malformed_detail": "<detail>", ...}`
    // — matching binding-core/MCP/SDKs.
    //
    // To trigger Malformed deterministically we tamper the iTXt chunk
    // payload (the JACS attachment) so it is no longer parseable as a
    // signed envelope.
    let (temp, _config_path) = prepare_jacs_fixture();
    write_to_fixture(temp.path(), "in.png", &make_png(32, 32));
    let _ = run_haiai_in_fixture(
        temp.path(),
        &["sign-image", "in.png", "--out", "signed.png"],
    );

    let path = temp.path().join("signed.png");
    let mut bytes = std::fs::read(&path).expect("read signed");
    // Find the iTXt chunk and corrupt its payload so JACS reports Malformed.
    let itxt_idx = bytes
        .windows(4)
        .position(|w| w == b"iTXt")
        .expect("iTXt marker present in signed PNG");
    // The 4 bytes preceding iTXt are the chunk length; the chunk data starts
    // 4 bytes after the type. Tamper bytes well into the payload (past the
    // keyword) so the parser sees garbage instead of valid JACS data.
    let tamper_offset = itxt_idx + 24;
    if tamper_offset < bytes.len() {
        bytes[tamper_offset] = 0xff;
        bytes[tamper_offset + 1] = 0xfe;
        bytes[tamper_offset + 2] = 0xfd;
    }
    std::fs::write(&path, &bytes).expect("write tampered");

    let out = run_haiai_in_fixture(temp.path(), &["verify-image", "signed.png", "--json"]);
    // verify-image returns exit 1 for Malformed (and other failures); JSON
    // must still be emitted on stdout, regardless of exit code.
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout)
        .expect("verify-image --json must emit valid JSON even on malformed/invalid signatures");
    assert!(
        parsed["status"].is_string(),
        "status must be a flat string; got tagged shape: {}",
        parsed
    );
    // The exact failure reason can vary depending on which byte JACS hits
    // first (Malformed vs InvalidSignature vs HashMismatch). The contract
    // pinned here is: status is always a flat snake_case string, NEVER a
    // tagged object. If status happens to be "malformed" we additionally
    // require malformed_detail.
    let status = parsed["status"].as_str().expect("status string");
    assert_ne!(
        status, "valid",
        "tampered iTXt should not verify as valid; got envelope: {}",
        parsed
    );
    if status == "malformed" {
        assert!(
            parsed["malformed_detail"].is_string(),
            "malformed status requires sibling malformed_detail field: {}",
            parsed
        );
    }
}

#[test]
fn cli_extract_media_signature_returns_decoded_json() {
    let (temp, _config_path) = prepare_jacs_fixture();
    write_to_fixture(temp.path(), "in.png", &make_png(32, 32));

    let _ = run_haiai_in_fixture(
        temp.path(),
        &["sign-image", "in.png", "--out", "signed.png"],
    );

    let out = run_haiai_in_fixture(temp.path(), &["extract-media-signature", "signed.png"]);
    assert!(
        out.status.success(),
        "extract failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("decoded payload should be JSON");
    assert!(parsed.is_object(), "expected JSON object, got: {stdout}");
}

#[test]
fn cli_extract_media_signature_raw_returns_base64url() {
    let (temp, _config_path) = prepare_jacs_fixture();
    write_to_fixture(temp.path(), "in.png", &make_png(32, 32));
    let _ = run_haiai_in_fixture(
        temp.path(),
        &["sign-image", "in.png", "--out", "signed.png"],
    );

    let out = run_haiai_in_fixture(
        temp.path(),
        &["extract-media-signature", "signed.png", "--raw-payload"],
    );
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(stdout.trim().as_bytes())
        .expect("raw payload must decode as base64url-no-pad");
}
