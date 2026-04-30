//! Regenerator for `fixtures/media/signed.{png,jpg,webp,md}` plus the
//! `_source/` byte inputs and `CHECKSUMS.txt`/`SIGNER.json` watchdogs.
//!
//! Run:
//!
//! ```text
//! cargo test -p haiai --test regen_media_fixtures -- --ignored
//! ```
//!
//! The single `regenerate_media_fixtures` test is `#[ignore]` so CI never
//! re-signs the fixtures (each signing run produces different bytes due to
//! the embedded `jacsVersionDate` timestamp). The committed pre-signed
//! bytes are what `cross_lang_contract.rs` (Rust), `test_cross_lang_media.py`
//! (Python), `cross-lang-media.test.ts` (Node), and `cross_lang_media_test.go`
//! (Go) all verify against — so they MUST stay byte-stable until someone
//! intentionally regenerates and commits.
//!
//! Reuses the shared `fixtures/jacs-agent/` agent (password
//! `secretpassord`) so cross-language tests don't have to materialize new
//! keys.

#![cfg(feature = "jacs-crate")]

use std::fs;
use std::path::{Path, PathBuf};

use haiai::{
    JacsMediaProvider, JacsProvider, LocalJacsProvider, SignImageOptions, SignTextOptions,
};
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// Deterministic source-byte generators (mirror local_media.rs).
// ---------------------------------------------------------------------------

fn make_png(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(width, height, image::Rgba([32, 64, 128, 255]));
    let mut buf = Vec::new();
    let mut cur = std::io::Cursor::new(&mut buf);
    img.write_to(&mut cur, image::ImageFormat::Png)
        .expect("png encode");
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

/// Minimal RIFF/WebP container. Same byte sequence used by
/// `tests/local_media.rs::local_provider_sign_image_webp_round_trip`.
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

const SOURCE_MARKDOWN: &[u8] = b"# Cross-language verify parity fixture\n\n\
This markdown is signed by the JACS test agent in fixtures/jacs-agent/.\n\
Its signed counterpart at fixtures/media/signed.md MUST verify under\n\
status \"valid\" in Rust, Python, Node, and Go.\n";

// ---------------------------------------------------------------------------
// Repo path + fixture-agent staging.
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

/// Copy a directory recursively, converting `_` → `:` in filenames so that
/// JACS `{id}:{version}.json` data files (illegal on Windows in the repo)
/// land at their runtime names. Mirrors `prepare_jacs_fixture` in
/// `cli_integration.rs` and `embedded_provider.rs::write_temp_fixture_config`.
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

fn stage_fixture_agent() -> (tempfile::TempDir, PathBuf) {
    // Password matches commit 39ff664 ("Regenerate jacs-agent fixture with
    // pq2025 signing"); also set in `embedded_provider.rs` test harness.
    std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", "secretpassord");

    let source = repo_root().join("fixtures/jacs-agent/jacs.config.json");
    assert!(
        source.exists(),
        "fixtures/jacs-agent/jacs.config.json missing at {}",
        source.display()
    );
    let source_dir = source.parent().unwrap();

    let mut value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&source).expect("read config"))
            .expect("parse config");

    let temp = tempfile::tempdir().expect("tempdir");

    let src_keys = source_dir.join(value["jacs_key_directory"].as_str().unwrap_or("keys"));
    let tmp_keys = temp.path().join("keys");
    fs::create_dir_all(&tmp_keys).expect("mkdir keys");
    for entry in fs::read_dir(&src_keys).expect("read keys") {
        let e = entry.expect("key entry");
        fs::copy(e.path(), tmp_keys.join(e.file_name())).expect("copy key");
    }

    let src_data = source_dir.join(value["jacs_data_directory"].as_str().unwrap_or("."));
    let tmp_data = temp.path().join("data");
    copy_fixture_dir(&src_data, &tmp_data);

    value["jacs_data_directory"] =
        serde_json::Value::String(tmp_data.to_string_lossy().into_owned());
    value["jacs_key_directory"] =
        serde_json::Value::String(tmp_keys.to_string_lossy().into_owned());

    let config = temp.path().join("jacs.config.json");
    fs::write(
        &config,
        serde_json::to_vec_pretty(&value).expect("encode config"),
    )
    .expect("write config");
    (temp, config)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

fn sign_image_to(provider: &LocalJacsProvider, source_bytes: &[u8], src_name: &str, dest: &Path) {
    let temp = tempfile::tempdir().expect("sign tempdir");
    let in_path = temp.path().join(src_name);
    let out_path = temp.path().join(format!("signed_{src_name}"));
    fs::write(&in_path, source_bytes).expect("stage source");
    let opts = SignImageOptions {
        backup: false,
        ..SignImageOptions::default()
    };
    provider
        .sign_image(in_path.to_str().unwrap(), out_path.to_str().unwrap(), opts)
        .expect("sign_image");
    let signed = fs::read(&out_path).expect("read signed");
    fs::write(dest, &signed).expect("write dest");
}

fn sign_markdown_to(provider: &LocalJacsProvider, source_bytes: &[u8], dest: &Path) {
    let temp = tempfile::tempdir().expect("md tempdir");
    let staged = temp.path().join("signed.md");
    fs::write(&staged, source_bytes).expect("stage source md");
    let opts = SignTextOptions {
        backup: false,
        ..SignTextOptions::default()
    };
    let outcome = provider
        .sign_text_file(staged.to_str().unwrap(), opts)
        .expect("sign_text_file");
    assert_eq!(
        outcome.signers_added, 1,
        "markdown signing must add a block"
    );
    fs::copy(&staged, dest).expect("write dest md");
}

#[test]
#[ignore = "regenerator: refresh fixtures/media/* with `cargo test -p haiai --test regen_media_fixtures -- --ignored`"]
fn regenerate_media_fixtures() {
    let media_dir = repo_root().join("fixtures/media");
    let source_dir = media_dir.join("_source");
    fs::create_dir_all(&source_dir).expect("mkdir _source");

    // 1. Materialize deterministic _source/* byte inputs.
    let png_bytes = make_png(64, 64);
    let jpg_bytes = make_jpeg(64, 64);
    let webp_bytes = make_webp();
    fs::write(source_dir.join("source.png"), &png_bytes).expect("write source.png");
    fs::write(source_dir.join("source.jpg"), &jpg_bytes).expect("write source.jpg");
    fs::write(source_dir.join("source.webp"), &webp_bytes).expect("write source.webp");
    fs::write(source_dir.join("source.md"), SOURCE_MARKDOWN).expect("write source.md");

    // 2. Stage the fixture agent and load its provider.
    let (_temp_agent, config_path) = stage_fixture_agent();
    let provider =
        LocalJacsProvider::from_config_path(Some(&config_path), None).expect("load fixture agent");
    let signer_id = provider.jacs_id().to_string();
    let algorithm = provider.algorithm().to_string();

    // 3. Sign each input → fixtures/media/signed.*
    sign_image_to(
        &provider,
        &png_bytes,
        "source.png",
        &media_dir.join("signed.png"),
    );
    sign_image_to(
        &provider,
        &jpg_bytes,
        "source.jpg",
        &media_dir.join("signed.jpg"),
    );
    sign_image_to(
        &provider,
        &webp_bytes,
        "source.webp",
        &media_dir.join("signed.webp"),
    );
    sign_markdown_to(&provider, SOURCE_MARKDOWN, &media_dir.join("signed.md"));

    // 4. Write CHECKSUMS.txt watchdog.
    let mut checksums = String::new();
    for name in ["signed.png", "signed.jpg", "signed.webp", "signed.md"] {
        let bytes = fs::read(media_dir.join(name)).expect("read signed");
        let hex = sha256_hex(&bytes);
        checksums.push_str(&format!("{hex}  {name}\n"));
    }
    fs::write(media_dir.join("CHECKSUMS.txt"), checksums).expect("write CHECKSUMS.txt");

    // 5. Write SIGNER.json so cross-language tests can read the expected
    //    signer identity instead of hardcoding it.
    let signer = serde_json::json!({
        "signer_id": signer_id,
        "algorithm": algorithm,
        "fixture_agent_dir": "fixtures/jacs-agent",
        "regenerator": "rust/haiai/tests/regen_media_fixtures.rs",
    });
    fs::write(
        media_dir.join("SIGNER.json"),
        serde_json::to_string_pretty(&signer).expect("encode SIGNER") + "\n",
    )
    .expect("write SIGNER.json");

    println!("regenerated fixtures/media/ — signer_id={signer_id} algorithm={algorithm}");
}
