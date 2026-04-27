//! TASK_004 integration tests for `haiai sign-text` / `verify-text`.

mod common;
use common::{prepare_jacs_fixture, run_haiai_in_fixture};

#[test]
fn cli_sign_text_round_trip() {
    let (temp, _config_path) = prepare_jacs_fixture();
    let path = temp.path().join("hello.md");
    std::fs::write(&path, b"# Hello\n").expect("write");

    let out = run_haiai_in_fixture(temp.path(), &["sign-text", "hello.md"]);
    assert!(
        out.status.success(),
        "sign-text failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let out = run_haiai_in_fixture(temp.path(), &["verify-text", "hello.md"]);
    assert!(
        out.status.success(),
        "verify-text failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn cli_verify_text_strict_missing_signature_exits_1() {
    let (temp, _config_path) = prepare_jacs_fixture();
    let path = temp.path().join("unsigned.md");
    std::fs::write(&path, b"# unsigned\n").expect("write");

    let out = run_haiai_in_fixture(temp.path(), &["verify-text", "unsigned.md", "--strict"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "strict missing signature should exit 1; got code={:?} stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn cli_verify_text_permissive_missing_signature_exits_2() {
    let (temp, _config_path) = prepare_jacs_fixture();
    let path = temp.path().join("unsigned.md");
    std::fs::write(&path, b"# unsigned\n").expect("write");

    let out = run_haiai_in_fixture(temp.path(), &["verify-text", "unsigned.md"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "permissive missing signature should exit 2; got code={:?}",
        out.status.code()
    );
}

#[test]
fn cli_verify_text_json_signed_uses_signed_discriminator() {
    // Regression for Issue 013.2: the Signed branch previously emitted
    // `"status": "valid" | "invalid"` (per-signature vocabulary) at the
    // file-level layer. Documented contract — matching JACS reference CLI,
    // binding-core, MCP, and Python/Node/Go SDKs — is `"status": "signed"`,
    // with valid-vs-invalid living inside per-signature `signatures[].status`.
    let (temp, _config_path) = prepare_jacs_fixture();
    let path = temp.path().join("hello.md");
    std::fs::write(&path, b"# Hello\n").expect("write");

    let out = run_haiai_in_fixture(temp.path(), &["sign-text", "hello.md"]);
    assert!(
        out.status.success(),
        "sign-text failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let out = run_haiai_in_fixture(temp.path(), &["verify-text", "hello.md", "--json"]);
    assert!(
        out.status.success(),
        "verify-text --json failed: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("verify-text --json must emit valid JSON");
    assert_eq!(
        parsed["status"].as_str(),
        Some("signed"),
        "verify-text --json must use the documented file-level discriminator (Issue 013.2); got: {}",
        parsed
    );
    let signatures = parsed["signatures"]
        .as_array()
        .expect("signatures field must be present and an array");
    assert!(
        !signatures.is_empty(),
        "Signed envelope must include at least one per-signature entry"
    );
    // Per-signature entries carry the valid/invalid vocabulary.
    let first_status = signatures[0]["status"].as_str().expect("per-sig status");
    assert_eq!(
        first_status, "valid",
        "round-tripped signature should be per-sig valid; got: {}",
        signatures[0]
    );
}

#[test]
fn cli_verify_text_json_missing_signature_uses_documented_discriminator() {
    // Pin the file-level vocabulary for the unsigned case too. binding-core
    // and SDKs document `"missing_signature"`. The CLI MissingSignature
    // branch was already correct; this test prevents future drift.
    //
    // Note: --strict makes JACS surface missing-sig as an `Err`, not the
    // structured `VerifyTextResult::MissingSignature` variant — so we use
    // permissive mode (default) to exercise the JSON envelope path. Exit 2
    // is expected; we only assert on stdout JSON shape.
    let (temp, _config_path) = prepare_jacs_fixture();
    let path = temp.path().join("unsigned.md");
    std::fs::write(&path, b"# unsigned\n").expect("write");

    let out = run_haiai_in_fixture(temp.path(), &["verify-text", "unsigned.md", "--json"]);
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout)
        .expect("verify-text --json must emit valid JSON for missing signatures");
    assert_eq!(
        parsed["status"].as_str(),
        Some("missing_signature"),
        "verify-text --json missing-sig discriminator drifted; got: {}",
        parsed
    );
    assert_eq!(
        parsed["signatures"].as_array().map(|a| a.len()),
        Some(0),
        "missing_signature envelope must carry empty signatures array"
    );
}
