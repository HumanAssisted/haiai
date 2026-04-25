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

    let out = run_haiai_in_fixture(temp.path(), &["sign-image", "in.png", "--out", "signed.png"]);
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

    let out = run_haiai_in_fixture(temp.path(), &["sign-image", "in.jpg", "--out", "signed.jpg"]);
    assert!(out.status.success(), "sign-image failed: {}", String::from_utf8_lossy(&out.stderr));

    let out = run_haiai_in_fixture(temp.path(), &["verify-image", "signed.jpg"]);
    assert!(out.status.success(), "verify-image failed: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn cli_verify_image_strict_missing_signature_exits_1() {
    let (temp, _config_path) = prepare_jacs_fixture();
    write_to_fixture(temp.path(), "unsigned.png", &make_png(32, 32));

    let out = run_haiai_in_fixture(temp.path(), &["verify-image", "unsigned.png", "--strict"]);
    assert_eq!(out.status.code(), Some(1), "expected exit 1 (strict missing signature)");
}

#[test]
fn cli_verify_image_permissive_missing_signature_exits_2() {
    let (temp, _config_path) = prepare_jacs_fixture();
    write_to_fixture(temp.path(), "unsigned.png", &make_png(32, 32));

    let out = run_haiai_in_fixture(temp.path(), &["verify-image", "unsigned.png"]);
    assert_eq!(out.status.code(), Some(2), "expected exit 2 (permissive missing)");
}

#[test]
fn cli_verify_image_tampered_exits_1() {
    let (temp, _config_path) = prepare_jacs_fixture();
    write_to_fixture(temp.path(), "in.png", &make_png(32, 32));

    let out = run_haiai_in_fixture(temp.path(), &["sign-image", "in.png", "--out", "signed.png"]);
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
fn cli_extract_media_signature_returns_decoded_json() {
    let (temp, _config_path) = prepare_jacs_fixture();
    write_to_fixture(temp.path(), "in.png", &make_png(32, 32));

    let _ = run_haiai_in_fixture(
        temp.path(),
        &["sign-image", "in.png", "--out", "signed.png"],
    );

    let out = run_haiai_in_fixture(temp.path(), &["extract-media-signature", "signed.png"]);
    assert!(out.status.success(), "extract failed: {}", String::from_utf8_lossy(&out.stderr));
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
