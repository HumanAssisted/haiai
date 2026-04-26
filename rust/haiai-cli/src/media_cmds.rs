//! TASK_004: CLI handlers for the five Layer-8 (`JacsMediaProvider`) commands.
//!
//! Every handler loads a `LocalJacsProvider` from `./jacs.config.json` and
//! delegates to the trait method. Output mirrors JACS's reference CLI:
//! human-readable by default, JSON when `--json`. Verify commands return an
//! exit code (0/1/2) for the caller to forward via `std::process::exit`.

use anyhow::Context as _;
use haiai::{
    media_verify_status_to_str, text_signature_status_to_str, JacsMediaProvider, LocalJacsProvider,
    MediaVerifyStatus, SignImageOptions, SignTextOptions, TextSignatureStatus, VerifyImageOptions,
    VerifyTextOptions, VerifyTextResult,
};
use serde_json::json;

fn load_provider() -> anyhow::Result<LocalJacsProvider> {
    LocalJacsProvider::from_config_path(None, None)
        .context("failed to load JACS agent from jacs.config.json")
}

// ---------------------------------------------------------------------------
// sign-text
// ---------------------------------------------------------------------------

pub fn handle_sign_text(
    file: &str,
    no_backup: bool,
    allow_duplicate: bool,
    json_out: bool,
) -> anyhow::Result<()> {
    let provider = load_provider()?;
    // Issue 010: use ..Default::default() so future JACS field additions
    // (e.g., a new SignTextOptions field) don't break this call site.
    let opts = SignTextOptions {
        backup: !no_backup,
        allow_duplicate,
        ..SignTextOptions::default()
    };
    let outcome = provider
        .sign_text_file(file, opts)
        .context("sign_text_file failed")?;

    if json_out {
        println!("{}", serde_json::to_string(&outcome)?);
    } else if outcome.signers_added == 0 {
        println!("{} unchanged (signature already present)", outcome.path);
    } else {
        println!(
            "Signed {}{}",
            outcome.path,
            outcome
                .backup_path
                .as_ref()
                .map(|b| format!("\nBackup: {b}"))
                .unwrap_or_default()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// verify-text  (returns exit code: 0 valid, 1 bad-sig/strict-missing, 2 missing permissive)
// ---------------------------------------------------------------------------

pub fn handle_verify_text(
    file: &str,
    key_dir: Option<&str>,
    strict: bool,
    json_out: bool,
) -> anyhow::Result<i32> {
    let provider = load_provider()?;
    let opts = VerifyTextOptions {
        strict,
        key_dir: key_dir.map(std::path::PathBuf::from),
    };

    match provider.verify_text_file(file, opts) {
        Ok(VerifyTextResult::Signed { signatures }) => {
            let all_valid = signatures
                .iter()
                .all(|s| s.status == TextSignatureStatus::Valid);
            if json_out {
                let entries: Vec<_> = signatures
                    .iter()
                    .map(|sig| {
                        json!({
                            "signer_id": sig.signer_id,
                            "algorithm": sig.algorithm,
                            "timestamp": sig.timestamp,
                            "status": text_signature_status_to_str(&sig.status),
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string(&json!({
                        "status": if all_valid { "valid" } else { "invalid" },
                        "signatures": entries,
                    }))?
                );
            } else if all_valid {
                println!(
                    "Signed by {} signer(s); all valid.",
                    signatures.len()
                );
                for sig in &signatures {
                    println!("  ✓ {} ({})", sig.signer_id, sig.algorithm);
                }
            } else {
                println!("Signature verification FAILED:");
                for sig in &signatures {
                    println!(
                        "  {} {} ({}) — {}",
                        if sig.status == TextSignatureStatus::Valid {
                            "✓"
                        } else {
                            "✗"
                        },
                        sig.signer_id,
                        sig.algorithm,
                        text_signature_status_to_str(&sig.status)
                    );
                }
            }
            Ok(if all_valid { 0 } else { 1 })
        }
        Ok(VerifyTextResult::MissingSignature) => {
            if json_out {
                println!(
                    "{}",
                    serde_json::to_string(&json!({
                        "status": "missing_signature",
                        "signatures": [],
                    }))?
                );
            } else {
                eprintln!("No JACS signature found in {file}");
            }
            // Permissive default: exit 2; strict: exit 1
            Ok(if strict { 1 } else { 2 })
        }
        Ok(VerifyTextResult::Malformed(detail)) => {
            if json_out {
                println!(
                    "{}",
                    serde_json::to_string(&json!({
                        "status": "malformed",
                        "malformed_detail": detail,
                        "signatures": [],
                    }))?
                );
            } else {
                eprintln!("Malformed signature: {detail}");
            }
            Ok(1)
        }
        Err(e) if strict => {
            eprintln!("Verification failed: {e}");
            Ok(1)
        }
        Err(e) => {
            eprintln!("Verification failed: {e}");
            Ok(2)
        }
    }
}

// Issue 011: status-string helpers were hoisted into `haiai::jacs`. The
// local `sig_status_str` was dropped — call `text_signature_status_to_str`
// directly so all surfaces (binding-core/MCP/CLI) share one source.

// ---------------------------------------------------------------------------
// sign-image
// ---------------------------------------------------------------------------

pub fn handle_sign_image(
    input: &str,
    out: &str,
    robust: bool,
    format: Option<&str>,
    refuse_overwrite: bool,
    json_out: bool,
) -> anyhow::Result<()> {
    let provider = load_provider()?;
    // Issue 010: use ..Default::default() so future JACS field additions
    // (e.g., a new SignImageOptions field) don't break this call site.
    let opts = SignImageOptions {
        robust,
        format_hint: format.map(str::to_string),
        refuse_overwrite,
        ..SignImageOptions::default()
    };
    let signed = provider
        .sign_image(input, out, opts)
        .context("sign_image failed")?;

    if json_out {
        println!("{}", serde_json::to_string(&signed)?);
    } else {
        println!("Signed {} -> {}", input, signed.out_path);
        println!("  format: {}", signed.format);
        println!("  signer: {}", signed.signer_id);
        println!("  robust: {}", signed.robust);
        if let Some(bak) = &signed.backup_path {
            println!("  backup: {}", bak);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// verify-image
// ---------------------------------------------------------------------------

pub fn handle_verify_image(
    file: &str,
    key_dir: Option<&str>,
    strict: bool,
    robust: bool,
    json_out: bool,
) -> anyhow::Result<i32> {
    let provider = load_provider()?;
    let base = VerifyTextOptions {
        strict,
        key_dir: key_dir.map(std::path::PathBuf::from),
    };
    let opts = VerifyImageOptions {
        base,
        scan_robust: robust,
    };

    let result = provider
        .verify_image(file, opts)
        .context("verify_image failed")?;

    let exit_code = match &result.status {
        MediaVerifyStatus::Valid => 0,
        MediaVerifyStatus::MissingSignature => {
            if strict {
                1
            } else {
                2
            }
        }
        MediaVerifyStatus::InvalidSignature
        | MediaVerifyStatus::HashMismatch
        | MediaVerifyStatus::KeyNotFound
        | MediaVerifyStatus::UnsupportedFormat
        | MediaVerifyStatus::Malformed(_) => 1,
    };

    if json_out {
        println!("{}", serde_json::to_string(&result)?);
    } else {
        // Issue 011: derive the wire label from the shared helper, then
        // append the malformed detail (which is CLI-specific human formatting)
        // — keeps the wire string drift-free while preserving the existing
        // human-readable detail.
        let status_label = match &result.status {
            MediaVerifyStatus::Malformed(d) => {
                format!("{} ({d})", media_verify_status_to_str(&result.status))
            }
            other => media_verify_status_to_str(other).to_string(),
        };
        println!("status: {}", status_label);
        if let Some(s) = &result.signer_id {
            println!("signer: {s}");
        }
        if let Some(a) = &result.algorithm {
            println!("algorithm: {a}");
        }
        if let Some(c) = &result.embedding_channels {
            println!("embedding: {c}");
        }
    }
    Ok(exit_code)
}

// ---------------------------------------------------------------------------
// extract-media-signature
// ---------------------------------------------------------------------------

pub fn handle_extract_media_signature(file: &str, raw_payload: bool) -> anyhow::Result<()> {
    let provider = load_provider()?;
    match provider
        .extract_media_signature(file, raw_payload)
        .context("extract_media_signature failed")?
    {
        Some(payload) => {
            println!("{payload}");
            Ok(())
        }
        None => {
            eprintln!("No JACS signature found in {file}");
            std::process::exit(2);
        }
    }
}
