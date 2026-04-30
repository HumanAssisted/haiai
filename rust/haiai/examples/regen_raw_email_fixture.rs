//! One-off helper that regenerates the `raw_email_roundtrip` scenario in
//! `fixtures/email_conformance.json` by signing a crafted raw MIME with the
//! shared `fixtures/jacs-agent` fixture agent.
//!
//! The output carries everything per-SDK tests need to run verify end-to-end
//! against a mocked HAI registry:
//!   - `input_raw_b64` / `expected_raw_b64`: the real JACS-signed MIME bytes
//!   - `input_sha256`: SHA-256 of those bytes (byte-fidelity watchdog)
//!   - `expected_size_bytes`: exact byte count
//!   - `expected_verify_valid: true`
//!   - `verify_registry`: `{ email, jacs_id, algorithm, public_key, reputation_tier, agent_status }`
//!     so every SDK's conformance test can mock the key-lookup endpoint and
//!     feed those bytes through `verify_email` for a real valid=true result.
//!
//! Usage:
//!   cargo run --example regen_raw_email_fixture --features jacs-crate
//!
//! Overwrites only the `raw_email_roundtrip` top-level key; other keys (legacy
//! scenarios, content hash vectors, mock verify response, mime round-trip) are
//! preserved.

use base64::Engine;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// Inlined copy of `haiai::key_format::normalize_public_key_pem` (private module).
/// Keeps the fixture generator decoupled from the crate's internal layout.
fn normalize_public_key_pem(raw: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(raw) {
        let trimmed = text.trim();
        if trimmed.contains("BEGIN PUBLIC KEY") || trimmed.contains("BEGIN RSA PUBLIC KEY") {
            let mut normalized = trimmed.replace("\r\n", "\n").replace('\r', "\n");
            if !normalized.ends_with('\n') {
                normalized.push('\n');
            }
            return normalized;
        }
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
    let mut pem = String::with_capacity(encoded.len() + "PUBLIC KEY".len() * 2 + 64);
    pem.push_str("-----BEGIN PUBLIC KEY-----\n");
    for chunk in encoded.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).expect("base64 output is valid ascii"));
        pem.push('\n');
    }
    pem.push_str("-----END PUBLIC KEY-----\n");
    pem
}

/// Copy fixture dir, converting `_` in filenames back to `:` (windows-safe
/// mapping used by `prepare_jacs_fixture` in cli_integration.rs).
fn copy_fixture_dir(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).expect("mkdir");
    for entry in std::fs::read_dir(src).expect("readdir") {
        let entry = entry.expect("entry");
        let src_path = entry.path();
        let name = entry.file_name().to_string_lossy().replace('_', ":");
        let dst_path = dst.join(&name);
        if src_path.is_dir() {
            copy_fixture_dir(&src_path, &dst_path);
        } else {
            std::fs::copy(&src_path, &dst_path).expect("copy");
        }
    }
}

fn main() {
    // Repo root = rust/haiai/../../
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo = manifest.parent().unwrap().parent().unwrap();
    let fixtures = repo.join("fixtures");

    let source_agent_dir = fixtures.join("jacs-agent");
    let source_config = source_agent_dir.join("jacs.config.json");
    assert!(
        source_config.exists(),
        "fixtures/jacs-agent/jacs.config.json missing at {}",
        source_config.display()
    );

    // Stage the fixture in a temp dir with absolute paths + colon filenames,
    // mirroring `prepare_jacs_fixture` in cli_integration.rs.
    let mut config_value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&source_config).expect("read config"))
            .expect("parse config");

    let temp = tempfile::tempdir().expect("tempdir");
    let src_key_dir = source_agent_dir.join(
        config_value["jacs_key_directory"]
            .as_str()
            .unwrap_or("keys"),
    );
    let tmp_key_dir = temp.path().join("keys");
    std::fs::create_dir_all(&tmp_key_dir).expect("mkdir keys");
    for entry in std::fs::read_dir(&src_key_dir).expect("read keys") {
        let entry = entry.expect("entry");
        std::fs::copy(entry.path(), tmp_key_dir.join(entry.file_name())).expect("copy key");
    }

    let src_data_dir =
        source_agent_dir.join(config_value["jacs_data_directory"].as_str().unwrap_or("."));
    let tmp_data_dir = temp.path().join("data");
    copy_fixture_dir(&src_data_dir, &tmp_data_dir);

    config_value["jacs_data_directory"] =
        serde_json::Value::String(tmp_data_dir.to_string_lossy().into_owned());
    config_value["jacs_key_directory"] =
        serde_json::Value::String(tmp_key_dir.to_string_lossy().into_owned());

    let tmp_config = temp.path().join("jacs.config.json");
    std::fs::write(
        &tmp_config,
        serde_json::to_vec_pretty(&config_value).expect("encode"),
    )
    .expect("write tmp config");

    // SAFETY: single-threaded helper, no other code sets env vars concurrently.
    // Password matches the one used to regenerate the fixture (see commit 39ff664
    // "Regenerate jacs-agent fixture with pq2025 signing"). It is also what the
    // integration harness in hai-mcp/embedded_provider.rs sets.
    unsafe {
        std::env::set_var("JACS_PRIVATE_KEY_PASSWORD", "secretpassord");
    }

    let agent =
        jacs::simple::SimpleAgent::load(Some(&tmp_config.display().to_string()), Some(false))
            .expect("load fixture SimpleAgent");

    let public_key_pem_bytes = agent.get_public_key().expect("get public key bytes");
    // Normalise to PEM. For raw-byte keys (pq2025 DER), ASCII-armor into a
    // PEM block. Same logic as `haiai::key_format::normalize_public_key_pem`
    // (inlined here because key_format is a private module).
    let public_key_pem = normalize_public_key_pem(&public_key_pem_bytes);

    // Agent id: compose `{jacsId}:{jacsVersion}` — JACS convention.
    let agent_json_str = agent.export_agent().expect("export_agent");
    let agent_json: serde_json::Value =
        serde_json::from_str(&agent_json_str).expect("parse agent json");
    let jacs_id = agent_json["jacsId"].as_str().expect("jacsId").to_string();
    let _jacs_version = agent_json["jacsVersion"]
        .as_str()
        .expect("jacsVersion")
        .to_string();
    // Use the bare agent id (no version suffix). `verify_email` checks
    // `jacsSignature.agentID == registry.jacs_id`, and `agentID` is the bare
    // `jacsId`, not the composite `jacsId:jacsVersion`. See
    // `rust/haiai/src/email.rs:180` identity-binding block.
    let composite_jacs_id = jacs_id.clone();

    // Build a raw MIME with CRLF line endings, an embedded NUL, and non-ASCII
    // bytes — exactly what R2 mandates must survive unchanged through the
    // download pipeline. The From header MUST match the registry email
    // the conformance consumers will mock, because `verify_email` enforces
    // identity binding (`From` == registry `email`).
    let from_email = "sender@example.com";
    let mut raw_mime: Vec<u8> = Vec::new();
    raw_mime.extend_from_slice(
        format!(
            "From: {from_email}\r\n\
             To: recipient@example.com\r\n\
             Subject: Raw Email Retrieval Conformance\r\n\
             Date: Thu, 23 Apr 2026 12:00:00 +0000\r\n\
             Message-ID: <raw-conformance-001@hai.ai>\r\n\
             MIME-Version: 1.0\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\r\n"
        )
        .as_bytes(),
    );
    raw_mime.extend_from_slice(
        b"Hello, this body contains CRLF line endings, a non-ASCII char (\xc3\xa9), and\r\n",
    );
    raw_mime.extend_from_slice(b"a NUL byte: \x00 between words. Byte-fidelity is mandatory.\r\n");

    // Sign. This produces a new multipart/mixed with a hai.ai.signature.jacs.json attachment.
    let signed = jacs::email::sign_email(&raw_mime, &agent).expect("sign_email");

    // Determine signingAlgorithm from the JACS attachment (so the fixture
    // registry entry stays in sync with whatever the agent actually used).
    let jacs_bytes = jacs::email::get_jacs_attachment(&signed).expect("get_jacs_attachment");
    let jacs_value: serde_json::Value =
        serde_json::from_slice(&jacs_bytes).expect("parse jacs attachment");
    let signing_algorithm = jacs_value["jacsSignature"]["signingAlgorithm"]
        .as_str()
        .expect("signingAlgorithm")
        .to_string();

    // Compute sha256 + b64 on the exact signed bytes.
    let mut hasher = Sha256::new();
    hasher.update(&signed);
    let sha = format!("{:x}", hasher.finalize());
    let b64 = base64::engine::general_purpose::STANDARD.encode(&signed);
    let size_bytes = signed.len();

    // Load existing fixture, replace only `raw_email_roundtrip`.
    let fixture_path = fixtures.join("email_conformance.json");
    let src = std::fs::read_to_string(&fixture_path).expect("read email_conformance.json");
    let mut doc: serde_json::Value =
        serde_json::from_str(&src).expect("parse email_conformance.json");

    doc["raw_email_roundtrip"] = serde_json::json!({
        "description": "Sign → persist → fetch → decode → verify chain. Bytes MUST byte-equal the signed input (R2), AND verify_email(bytes) MUST return valid=true when the HAI registry is mocked with the verify_registry data below. Re-generate with: cargo run --example regen_raw_email_fixture --features jacs-crate.",
        "input_raw_b64": b64,
        "input_sha256": sha,
        "expected_available": true,
        "expected_omitted_reason": null,
        "expected_raw_b64": b64,
        "expected_size_bytes": size_bytes,
        "expected_verify_valid": true,
        "verify_registry": {
            "email": from_email,
            "jacs_id": composite_jacs_id,
            "algorithm": signing_algorithm,
            "public_key": public_key_pem,
            "reputation_tier": "free_chaotic",
            "agent_status": "active",
            "registered_at": "2026-04-23T12:00:00Z",
            "benchmarks_completed": []
        }
    });

    let out = serde_json::to_string_pretty(&doc).expect("serialize");
    std::fs::write(&fixture_path, format!("{out}\n")).expect("write fixture");

    println!(
        "wrote {} ({size_bytes} bytes, sha256={sha})",
        fixture_path.display()
    );
    println!("jacs_id = {composite_jacs_id}");
    println!("algorithm = {signing_algorithm}");
}
