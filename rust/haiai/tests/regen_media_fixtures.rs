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
//! Creates a fresh ephemeral PQ signer and commits only its public key. It also
//! creates a separate portable signed Ed25519 verifier fixture for bindings
//! that cannot create agents (notably CGo). The verifier's encrypted test key
//! is not a media-signing key; every language resolves the PQ signer through
//! the explicit public-key directory contract.

#![cfg(feature = "jacs-crate")]

use std::fs;
use std::path::{Path, PathBuf};

use haiai::{
    CreateAgentOptions, JacsMediaProvider, JacsProvider, LocalJacsProvider, SignImageOptions,
    SignTextOptions,
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
This markdown is signed by the dedicated JACS media fixture signer.\n\
Its signed counterpart at fixtures/media/signed.md MUST verify under\n\
status \"valid\" in Rust, Python, Node, and Go.\n";

// ---------------------------------------------------------------------------
// Repo path + fixture-agent creation.
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rust/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn copy_fixture_tree_for_repo(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("create fixture destination");
    for entry in fs::read_dir(src).expect("read generated fixture") {
        let entry = entry.expect("fixture entry");
        let source = entry.path();
        let source_name = entry.file_name();
        if matches!(
            source_name.to_str(),
            Some(".haiai_resolved_jacs.config.json" | ".gitignore" | ".dockerignore")
        ) {
            continue;
        }
        let repo_name = entry.file_name().to_string_lossy().replace(':', "_");
        let target = dst.join(repo_name);
        if source.is_dir() {
            copy_fixture_tree_for_repo(&source, &target);
        } else {
            fs::copy(source, target).expect("copy generated fixture file");
        }
    }
}

fn create_portable_fixture_agent(
    name: &str,
    algorithm: &str,
    password: &str,
) -> (tempfile::TempDir, LocalJacsProvider, PathBuf, PathBuf) {
    std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", password);
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().canonicalize().expect("canonical tempdir");
    let config = root.join("jacs.config.json");

    // JACS signs path values into the config. An absolute config path anchors
    // filesystem creation at this temp root, while the relative data/key
    // values remain portable in the signed config copied to the repository.
    let create_result = LocalJacsProvider::create_agent_with_options(&CreateAgentOptions {
        name: name.to_string(),
        password: password.to_string(),
        algorithm: Some(algorithm.to_string()),
        data_directory: Some("data".to_string()),
        key_directory: Some("keys".to_string()),
        config_path: Some(config.to_string_lossy().into_owned()),
        agent_type: Some("ai".to_string()),
        description: Some(format!("Cross-language media fixture {name}")),
        domain: None,
        default_storage: Some("fs".to_string()),
    });
    let result = create_result.expect("create signed portable fixture agent");

    let provider = LocalJacsProvider::from_config_path(Some(&config), None)
        .expect("load portable fixture agent");
    let result_public_key = PathBuf::from(result.public_key_path);
    let public_key_path = if result_public_key.is_absolute() {
        result_public_key
    } else {
        root.join(result_public_key)
    };
    (temp, provider, public_key_path, root)
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

    // 2. Create the ephemeral PQ signer and preserve only its public key.
    let (_temp_agent, provider, public_key_path, _signer_root) = create_portable_fixture_agent(
        "cross-language-media-fixture-signer",
        "pq2025",
        "MediaFixtureSigningPass!123",
    );
    let signer_id = provider.jacs_id().to_string();
    let algorithm = provider.algorithm().to_string();
    let public_key_fixture = media_dir.join("signer.public.pem");
    fs::copy(&public_key_path, &public_key_fixture).expect("write signer public key fixture");

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

    // 4. Create a separate signed verifier fixture for bindings that cannot
    //    bootstrap JACS agents. It deliberately has a different identity and
    //    algorithm so successful verification proves explicit PQ key lookup.
    let (_verifier_temp, _verifier, _verifier_public_key, verifier_root) =
        create_portable_fixture_agent(
            "cross-language-media-fixture-verifier",
            "ed25519",
            "MediaFixtureVerifierPass!123",
        );
    let verifier_fixture = media_dir.join("verifier-agent");
    if verifier_fixture.exists() {
        fs::remove_dir_all(&verifier_fixture).expect("remove old verifier fixture");
    }
    copy_fixture_tree_for_repo(&verifier_root, &verifier_fixture);
    let obsolete_signer_fixture = media_dir.join("signer-agent");
    if obsolete_signer_fixture.exists() {
        fs::remove_dir_all(obsolete_signer_fixture).expect("remove obsolete signer fixture");
    }

    // 5. Write CHECKSUMS.txt watchdog.
    let mut checksums = String::new();
    for name in ["signed.png", "signed.jpg", "signed.webp", "signed.md"] {
        let bytes = fs::read(media_dir.join(name)).expect("read signed");
        let hex = sha256_hex(&bytes);
        checksums.push_str(&format!("{hex}  {name}\n"));
    }
    fs::write(media_dir.join("CHECKSUMS.txt"), checksums).expect("write CHECKSUMS.txt");

    // 6. Write SIGNER.json so cross-language tests can read the expected
    //    signer identity instead of hardcoding it.
    let signer = serde_json::json!({
        "signer_id": signer_id,
        "algorithm": algorithm,
        "verifier_agent_dir": "fixtures/media/verifier-agent",
        "public_key_file": "fixtures/media/signer.public.pem",
        "regenerator": "rust/haiai/tests/regen_media_fixtures.rs",
    });
    fs::write(
        media_dir.join("SIGNER.json"),
        serde_json::to_string_pretty(&signer).expect("encode SIGNER") + "\n",
    )
    .expect("write SIGNER.json");

    println!("regenerated fixtures/media/ — signer_id={signer_id} algorithm={algorithm}");
}
