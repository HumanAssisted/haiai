//! Shared test helpers for integration tests (haiai_bin, fixture prep, image factories).
//! Extracted from `cli_integration.rs` for re-use by `cli_image_tests.rs` and
//! `cli_text_tests.rs` (TASK_004). Functions are pub(crate) at the test crate
//! level — each test file `mod common;` to bring them in scope.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub fn haiai_bin() -> PathBuf {
    let current_exe = std::env::current_exe().expect("current_exe");
    let target_dir = current_exe
        .parent()
        .and_then(Path::parent)
        .expect("target dir for integration test binary");
    let candidate = target_dir.join(format!("haiai{}", std::env::consts::EXE_SUFFIX));
    assert!(
        candidate.exists(),
        "expected haiai binary at {}. Run `cargo build -p haiai-cli` first.",
        candidate.display()
    );
    candidate
}

/// Bootstrap a NEW JACS agent in a tempdir using the `haiai init` CLI
/// command. Returns (TempDir, config_path). The agent password is
/// `TestPass!123`.
///
/// This avoids the storage-backend path-resolution headaches that the
/// fixture-copy approach hit when JACS's `calculate_storage_root_and_normalize`
/// rebases external absolute paths to `/`. A freshly-`init`-ed agent has its
/// data layout consistent with what JACS expects out-of-box.
pub const NEW_AGENT_PASSWORD: &str = "TestPass!123";

pub fn prepare_jacs_fixture() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let config_path = temp.path().join("jacs.config.json");

    // Use relative key/data dirs and run init with cwd=tempdir so JACS
    // resolves them against the same directory the CLI subcommand will
    // later resolve against (since the CLI looks for ./jacs.config.json).
    // This matches the working pattern in `tests/init_suite.rs` and
    // `tests/local_media.rs`.
    let agent_name = format!("test-agent-{}", std::process::id());
    let output = std::process::Command::new(haiai_bin())
        .args([
            "init",
            "--name",
            &agent_name,
            "--register",
            "false",
            "--algorithm",
            "ring-Ed25519",
            "--config-path",
            "jacs.config.json",
            "--key-dir",
            "keys",
            "--data-dir",
            "data",
        ])
        .current_dir(temp.path())
        .env("JACS_PRIVATE_KEY_PASSWORD", NEW_AGENT_PASSWORD)
        .output()
        .expect("run init");

    assert!(
        output.status.success(),
        "init failed.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(config_path.exists(), "config not created");

    (temp, config_path)
}

// ---------------------------------------------------------------------------
// Image fixtures (also used by cli_image_tests.rs)
// ---------------------------------------------------------------------------

pub fn make_png(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(width, height, image::Rgba([32, 64, 128, 255]));
    let mut buf = Vec::new();
    let mut cur = std::io::Cursor::new(&mut buf);
    img.write_to(&mut cur, image::ImageFormat::Png)
        .expect("png encode");
    buf
}

pub fn make_jpeg(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(width, height, image::Rgb([200, 150, 100]));
    let mut buf = Vec::new();
    let mut cur = std::io::Cursor::new(&mut buf);
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cur, 95);
    img.write_with_encoder(encoder).expect("jpeg encode");
    buf
}

/// Run the haiai binary with `cwd` set to the fixture tempdir and the
/// `init`-generated agent password env var set. Returns the captured Output.
pub fn run_haiai_in_fixture(fixture_dir: &Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(haiai_bin())
        .args(args)
        .current_dir(fixture_dir)
        .env("JACS_PRIVATE_KEY_PASSWORD", NEW_AGENT_PASSWORD)
        .output()
        .expect("run haiai")
}
