//! Rust HAISDK.
//!
//! This crate is intentionally a thin HAI-platform wrapper around JACS.
//! Runtime signing/canonicalization should be delegated via [`JacsProvider`].

pub mod client;
pub mod config;
pub mod error;
pub mod jacs;
#[cfg(feature = "jacs-local")]
pub mod jacs_local;
pub mod types;
pub mod verify;

pub use client::{HaiClient, HaiClientOptions};
pub use config::{load_config, resolve_private_key_candidates, AgentConfig};
pub use error::{HaiError, Result};
pub use jacs::{JacsProvider, NoopJacsProvider, StaticJacsProvider};
#[cfg(feature = "jacs-local")]
pub use jacs_local::LocalJacsProvider;
pub use types::*;
pub use verify::{
    generate_verify_link, generate_verify_link_hosted, MAX_VERIFY_DOCUMENT_BYTES,
    MAX_VERIFY_URL_LEN,
};
