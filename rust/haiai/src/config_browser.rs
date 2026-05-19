//! In-memory `AgentConfig` deserializer for browser builds.
//!
//! HAIAI_WASM_PRD §4.7: in the browser, no `jacs.config.json` on disk.
//! Persistence is the TypeScript wrapper's job — it calls into
//! `@jacs/wasm`'s `localStore` to read the encrypted material from
//! `localStorage`, then hands the resulting JSON string to this module
//! to materialize an [`AgentConfig`].
//!
//! Pure logic — no `std::fs`, no `tokio::*`, no browser API calls. The
//! module compiles on both targets so the native side (e.g.
//! `cargo test` against in-memory JSON fixtures) can reuse the same
//! deserializer.

use crate::config::AgentConfig;
use crate::error::{HaiError, Result};

/// Materialize an [`AgentConfig`] from a JSON string.
///
/// Returns [`HaiError::ConfigInvalid`] (the existing error variant —
/// PRD §10 forbids adding new ones) with a message that names the
/// underlying serde error so the caller can surface a useful message
/// to the browser developer.
pub fn from_json_string(s: &str) -> Result<AgentConfig> {
    serde_json::from_str::<AgentConfig>(s).map_err(|e| HaiError::ConfigInvalid {
        message: format!("config_browser::from_json_string: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_config() -> AgentConfig {
        AgentConfig {
            jacs_agent_name: "browser-agent".to_string(),
            jacs_agent_version: "1.0.0".to_string(),
            jacs_key_dir: PathBuf::from("/wasm-virtual/keys"),
            jacs_id: Some("agent-123".to_string()),
            jacs_private_key_path: Some(PathBuf::from("/wasm-virtual/keys/private.pem")),
            source_path: PathBuf::from("/wasm-virtual/jacs.config.json"),
            agent_email: Some("agent@hai.ai".to_string()),
        }
    }

    #[test]
    fn roundtrip_serialize_deserialize() {
        let cfg = sample_config();
        let json = serde_json::to_string(&cfg).expect("serialize");
        let parsed = from_json_string(&json).expect("parse");
        assert_eq!(parsed.jacs_agent_name, cfg.jacs_agent_name);
        assert_eq!(parsed.jacs_agent_version, cfg.jacs_agent_version);
        assert_eq!(parsed.jacs_id, cfg.jacs_id);
        assert_eq!(parsed.agent_email, cfg.agent_email);
    }

    #[test]
    fn from_json_string_rejects_malformed() {
        let err = from_json_string("{").expect_err("should reject malformed json");
        match err {
            HaiError::ConfigInvalid { message } => {
                assert!(message.contains("config_browser::from_json_string"));
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }

    #[test]
    fn from_json_string_rejects_missing_required_field() {
        let err = from_json_string("{}").expect_err("should reject empty object");
        // serde reports the missing-field error; the variant routing is the
        // load-bearing assertion (callers programmatically match on ConfigInvalid).
        match err {
            HaiError::ConfigInvalid { message } => {
                assert!(
                    message.contains("missing field"),
                    "expected serde missing-field error in: {message}"
                );
            }
            other => panic!("unexpected error variant: {other:?}"),
        }
    }
}
