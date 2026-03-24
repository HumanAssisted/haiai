//! Node.js bindings for HAI SDK via napi-rs.
//!
//! Every `HaiClientWrapper` method is exposed as a JavaScript `Promise<string>`-
//! returning function. No business logic lives here -- each method is thin
//! delegation to `hai-binding-core`.

use std::sync::Arc;

use hai_binding_core::{HaiBindingError, HaiClientWrapper, RT};
use haiai::jacs::StaticJacsProvider;
use napi::bindgen_prelude::*;
use napi_derive::napi;

// =============================================================================
// Error Conversion
// =============================================================================

fn to_napi_err(e: HaiBindingError) -> Error {
    Error::new(Status::GenericFailure, format!("{}: {}", e.kind, e.message))
}

// =============================================================================
// HaiClient class
// =============================================================================

/// JavaScript-facing HAI client.
///
/// Constructor accepts a JSON config string. All methods return `Promise<string>`
/// containing JSON responses.
#[napi]
pub struct HaiClient {
    inner: Arc<HaiClientWrapper>,
}

#[napi]
impl HaiClient {
    /// Create a new HaiClient from a config JSON string.
    ///
    /// Config format: `{"base_url": "...", "jacs_id": "...", "timeout_secs": 30, "max_retries": 3}`
    #[napi(constructor)]
    pub fn new(config_json: String) -> Result<Self> {
        // Parse config to extract jacs_id for the provider
        let config: serde_json::Value = serde_json::from_str(&config_json)
            .map_err(|e| Error::new(Status::InvalidArg, format!("invalid config JSON: {e}")))?;

        let jacs_id = config
            .get("jacs_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // For now, use StaticJacsProvider. In production, this would be replaced
        // with the real JACS binding provider passed from JavaScript.
        let provider = StaticJacsProvider::new(jacs_id);

        let wrapper = HaiClientWrapper::from_config_json(&config_json, Box::new(provider))
            .map_err(to_napi_err)?;

        Ok(Self {
            inner: Arc::new(wrapper),
        })
    }

    // =========================================================================
    // Registration & Identity
    // =========================================================================

    #[napi]
    pub async fn hello(&self, include_test: bool) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.hello(include_test).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn check_username(&self, username: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.check_username(&username).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn register(&self, options_json: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.register(&options_json).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn rotate_keys(&self, options_json: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.rotate_keys(&options_json).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn update_agent(&self, new_agent_data: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.update_agent(&new_agent_data).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn submit_response(&self, params_json: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.submit_response(&params_json).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn verify_status(&self, agent_id: Option<String>) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.verify_status(agent_id.as_deref()).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    // =========================================================================
    // Username
    // =========================================================================

    #[napi]
    pub async fn claim_username(&self, agent_id: String, username: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.claim_username(&agent_id, &username).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn update_username(&self, agent_id: String, username: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.update_username(&agent_id, &username).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn delete_username(&self, agent_id: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.delete_username(&agent_id).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    // =========================================================================
    // Email Core
    // =========================================================================

    #[napi]
    pub async fn send_email(&self, options_json: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.send_email(&options_json).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn send_signed_email(&self, options_json: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.send_signed_email(&options_json).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn list_messages(&self, options_json: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.list_messages(&options_json).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn update_labels(&self, params_json: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.update_labels(&params_json).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn get_email_status(&self) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.get_email_status().await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn get_message(&self, message_id: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.get_message(&message_id).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn get_unread_count(&self) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.get_unread_count().await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    // =========================================================================
    // Email Actions
    // =========================================================================

    #[napi]
    pub async fn mark_read(&self, message_id: String) -> Result<()> {
        let client = self.inner.clone();
        RT.spawn(async move { client.mark_read(&message_id).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn mark_unread(&self, message_id: String) -> Result<()> {
        let client = self.inner.clone();
        RT.spawn(async move { client.mark_unread(&message_id).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn delete_message(&self, message_id: String) -> Result<()> {
        let client = self.inner.clone();
        RT.spawn(async move { client.delete_message(&message_id).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn archive(&self, message_id: String) -> Result<()> {
        let client = self.inner.clone();
        RT.spawn(async move { client.archive(&message_id).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn unarchive(&self, message_id: String) -> Result<()> {
        let client = self.inner.clone();
        RT.spawn(async move { client.unarchive(&message_id).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn reply_with_options(&self, params_json: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.reply_with_options(&params_json).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn forward(&self, params_json: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.forward(&params_json).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    // =========================================================================
    // Search & Contacts
    // =========================================================================

    #[napi]
    pub async fn search_messages(&self, options_json: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.search_messages(&options_json).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn contacts(&self) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.contacts().await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    // =========================================================================
    // Key Operations
    // =========================================================================

    #[napi]
    pub async fn fetch_remote_key(&self, jacs_id: String, version: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.fetch_remote_key(&jacs_id, &version).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn fetch_key_by_hash(&self, hash: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.fetch_key_by_hash(&hash).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn fetch_key_by_email(&self, email: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.fetch_key_by_email(&email).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn fetch_key_by_domain(&self, domain: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.fetch_key_by_domain(&domain).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn fetch_all_keys(&self, jacs_id: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.fetch_all_keys(&jacs_id).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    // =========================================================================
    // Verification
    // =========================================================================

    #[napi]
    pub async fn verify_document(&self, document: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.verify_document(&document).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn get_verification(&self, agent_id: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.get_verification(&agent_id).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn verify_agent_document(&self, request_json: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.verify_agent_document(&request_json).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    // =========================================================================
    // Benchmarks
    // =========================================================================

    #[napi]
    pub async fn benchmark(&self, name: Option<String>, tier: Option<String>) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move {
            client.benchmark(name.as_deref(), tier.as_deref()).await
        })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn free_run(&self, transport: Option<String>) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.free_run(transport.as_deref()).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn pro_run(&self, options_json: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.pro_run(&options_json).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn enterprise_run(&self) -> Result<()> {
        let client = self.inner.clone();
        RT.spawn(async move { client.enterprise_run().await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    // =========================================================================
    // Sync JACS Delegation
    // =========================================================================

    #[napi]
    pub async fn build_auth_header(&self) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.build_auth_header().await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn sign_message(&self, message: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.sign_message(&message).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn canonical_json(&self, value_json: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.canonical_json(&value_json).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn verify_a2a_artifact(&self, wrapped_json: String) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.verify_a2a_artifact(&wrapped_json).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn export_agent_json(&self) -> Result<String> {
        let client = self.inner.clone();
        RT.spawn(async move { client.export_agent_json().await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?
            .map_err(to_napi_err)
    }

    // =========================================================================
    // Client State
    // =========================================================================

    #[napi]
    pub async fn jacs_id(&self) -> Result<String> {
        let client = self.inner.clone();
        Ok(RT.spawn(async move { client.jacs_id().await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))?)
    }

    #[napi]
    pub async fn set_hai_agent_id(&self, id: String) -> Result<()> {
        let client = self.inner.clone();
        RT.spawn(async move { client.set_hai_agent_id(id).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))
    }

    #[napi]
    pub async fn set_agent_email(&self, email: String) -> Result<()> {
        let client = self.inner.clone();
        RT.spawn(async move { client.set_agent_email(email).await })
            .await
            .map_err(|e| Error::new(Status::GenericFailure, e.to_string()))
    }
}
