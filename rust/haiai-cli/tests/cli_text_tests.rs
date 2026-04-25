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
