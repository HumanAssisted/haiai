//! Node.js bindings for HAI SDK via napi-rs.
//!
//! Every `HaiClientWrapper` method is exposed as a JavaScript `Promise<string>`-
//! returning function. No business logic lives here -- each method is thin
//! delegation to `hai-binding-core`.
//!
//! napi-rs manages its own tokio runtime for `#[napi] async fn` methods, so
//! we call wrapper methods directly instead of spawning on a separate runtime.
//! This avoids an unnecessary thread hop between the napi-rs runtime and the
//! binding-core runtime.

use std::sync::Arc;

use hai_binding_core::{HaiBindingError, HaiClientWrapper};
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
    /// If `jacs_config_path` is provided, loads a real JACS agent for
    /// cryptographic signing. Otherwise falls back to a test-only provider.
    ///
    /// Config format: `{"base_url": "...", "jacs_id": "...", "jacs_config_path": "/path/to/jacs.config.json", "timeout_secs": 30, "max_retries": 3}`
    #[napi(constructor)]
    pub fn new(config_json: String) -> Result<Self> {
        let wrapper = HaiClientWrapper::from_config_json_auto(&config_json).map_err(to_napi_err)?;

        Ok(Self {
            inner: Arc::new(wrapper),
        })
    }

    // =========================================================================
    // Registration & Identity
    // =========================================================================

    #[napi]
    pub async fn hello(&self, include_test: bool) -> Result<String> {
        self.inner.hello(include_test).await.map_err(to_napi_err)
    }

    #[napi]
    pub async fn register(&self, options_json: String) -> Result<String> {
        self.inner
            .register(&options_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn register_new_agent(&self, options_json: String) -> Result<String> {
        self.inner
            .register_new_agent(&options_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn rotate_keys(&self, options_json: String) -> Result<String> {
        self.inner
            .rotate_keys(&options_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn update_agent(&self, new_agent_data: String) -> Result<String> {
        self.inner
            .update_agent(&new_agent_data)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn submit_response(&self, params_json: String) -> Result<String> {
        self.inner
            .submit_response(&params_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn verify_status(&self, agent_id: Option<String>) -> Result<String> {
        self.inner
            .verify_status(agent_id.as_deref())
            .await
            .map_err(to_napi_err)
    }

    // =========================================================================
    // Username
    // =========================================================================

    #[napi]
    pub async fn update_username(&self, agent_id: String, username: String) -> Result<String> {
        self.inner
            .update_username(&agent_id, &username)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn delete_username(&self, agent_id: String) -> Result<String> {
        self.inner
            .delete_username(&agent_id)
            .await
            .map_err(to_napi_err)
    }

    // =========================================================================
    // Email Core
    // =========================================================================

    #[napi]
    pub async fn send_email(&self, options_json: String) -> Result<String> {
        self.inner
            .send_email(&options_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn send_signed_email(&self, options_json: String) -> Result<String> {
        self.inner
            .send_signed_email(&options_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn list_messages(&self, options_json: String) -> Result<String> {
        self.inner
            .list_messages(&options_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn update_labels(&self, params_json: String) -> Result<String> {
        self.inner
            .update_labels(&params_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn get_email_status(&self) -> Result<String> {
        self.inner.get_email_status().await.map_err(to_napi_err)
    }

    #[napi]
    pub async fn get_message(&self, message_id: String) -> Result<String> {
        self.inner
            .get_message(&message_id)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn get_raw_email(&self, message_id: String) -> Result<String> {
        self.inner
            .get_raw_email(&message_id)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn get_unread_count(&self) -> Result<String> {
        self.inner.get_unread_count().await.map_err(to_napi_err)
    }

    // =========================================================================
    // Email Actions
    // =========================================================================

    #[napi]
    pub async fn mark_read(&self, message_id: String) -> Result<()> {
        self.inner.mark_read(&message_id).await.map_err(to_napi_err)
    }

    #[napi]
    pub async fn mark_unread(&self, message_id: String) -> Result<()> {
        self.inner
            .mark_unread(&message_id)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn delete_message(&self, message_id: String) -> Result<()> {
        self.inner
            .delete_message(&message_id)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn archive(&self, message_id: String) -> Result<()> {
        self.inner.archive(&message_id).await.map_err(to_napi_err)
    }

    #[napi]
    pub async fn unarchive(&self, message_id: String) -> Result<()> {
        self.inner.unarchive(&message_id).await.map_err(to_napi_err)
    }

    #[napi]
    pub async fn reply_with_options(&self, params_json: String) -> Result<String> {
        self.inner
            .reply_with_options(&params_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn forward(&self, params_json: String) -> Result<String> {
        self.inner.forward(&params_json).await.map_err(to_napi_err)
    }

    // =========================================================================
    // Search & Contacts
    // =========================================================================

    #[napi]
    pub async fn search_messages(&self, options_json: String) -> Result<String> {
        self.inner
            .search_messages(&options_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn contacts(&self) -> Result<String> {
        self.inner.contacts().await.map_err(to_napi_err)
    }

    // =========================================================================
    // Server Keys
    // =========================================================================

    #[napi]
    pub async fn fetch_server_keys(&self) -> Result<String> {
        self.inner.fetch_server_keys().await.map_err(to_napi_err)
    }

    // =========================================================================
    // Raw Email Sign/Verify
    // =========================================================================

    #[napi]
    pub async fn sign_email_raw(&self, raw_email_b64: String) -> Result<String> {
        self.inner
            .sign_email_raw(&raw_email_b64)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn verify_email_raw(&self, raw_email_b64: String) -> Result<String> {
        self.inner
            .verify_email_raw(&raw_email_b64)
            .await
            .map_err(to_napi_err)
    }

    // =========================================================================
    // Local Media Sign/Verify (Layer 8 / TASK_008)
    // =========================================================================

    #[napi]
    pub async fn sign_text(&self, path: String, opts_json: String) -> Result<String> {
        self.inner
            .sign_text(&path, &opts_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn verify_text(&self, path: String, opts_json: String) -> Result<String> {
        self.inner
            .verify_text(&path, &opts_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn sign_image(
        &self,
        in_path: String,
        out_path: String,
        opts_json: String,
    ) -> Result<String> {
        self.inner
            .sign_image(&in_path, &out_path, &opts_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn verify_image(&self, path: String, opts_json: String) -> Result<String> {
        self.inner
            .verify_image(&path, &opts_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn extract_media_signature(&self, path: String, opts_json: String) -> Result<String> {
        self.inner
            .extract_media_signature(&path, &opts_json)
            .await
            .map_err(to_napi_err)
    }

    // =========================================================================
    // Attestations
    // =========================================================================

    #[napi]
    pub async fn create_attestation(&self, params_json: String) -> Result<String> {
        self.inner
            .create_attestation(&params_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn list_attestations(&self, params_json: String) -> Result<String> {
        self.inner
            .list_attestations(&params_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn get_attestation(&self, agent_id: String, doc_id: String) -> Result<String> {
        self.inner
            .get_attestation(&agent_id, &doc_id)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn verify_attestation(&self, document: String) -> Result<String> {
        self.inner
            .verify_attestation(&document)
            .await
            .map_err(to_napi_err)
    }

    // =========================================================================
    // Email Templates
    // =========================================================================

    #[napi]
    pub async fn create_email_template(&self, options_json: String) -> Result<String> {
        self.inner
            .create_email_template(&options_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn list_email_templates(&self, options_json: String) -> Result<String> {
        self.inner
            .list_email_templates(&options_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn get_email_template(&self, template_id: String) -> Result<String> {
        self.inner
            .get_email_template(&template_id)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn update_email_template(
        &self,
        template_id: String,
        options_json: String,
    ) -> Result<String> {
        self.inner
            .update_email_template(&template_id, &options_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn delete_email_template(&self, template_id: String) -> Result<()> {
        self.inner
            .delete_email_template(&template_id)
            .await
            .map_err(to_napi_err)
    }

    // =========================================================================
    // Key Operations
    // =========================================================================

    #[napi]
    pub async fn fetch_remote_key(&self, jacs_id: String, version: String) -> Result<String> {
        self.inner
            .fetch_remote_key(&jacs_id, &version)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn fetch_key_by_hash(&self, hash: String) -> Result<String> {
        self.inner
            .fetch_key_by_hash(&hash)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn fetch_key_by_email(&self, email: String) -> Result<String> {
        self.inner
            .fetch_key_by_email(&email)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn fetch_key_by_domain(&self, domain: String) -> Result<String> {
        self.inner
            .fetch_key_by_domain(&domain)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn fetch_all_keys(&self, jacs_id: String) -> Result<String> {
        self.inner
            .fetch_all_keys(&jacs_id)
            .await
            .map_err(to_napi_err)
    }

    // =========================================================================
    // Verification
    // =========================================================================

    #[napi]
    pub async fn verify_document(&self, document: String) -> Result<String> {
        self.inner
            .verify_document(&document)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn get_verification(&self, agent_id: String) -> Result<String> {
        self.inner
            .get_verification(&agent_id)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn verify_agent_document(&self, request_json: String) -> Result<String> {
        self.inner
            .verify_agent_document(&request_json)
            .await
            .map_err(to_napi_err)
    }

    // =========================================================================
    // Benchmarks
    // =========================================================================

    #[napi]
    pub async fn benchmark(&self, name: Option<String>, tier: Option<String>) -> Result<String> {
        self.inner
            .benchmark(name.as_deref(), tier.as_deref())
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn free_run(&self, transport: Option<String>) -> Result<String> {
        self.inner
            .free_run(transport.as_deref())
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn pro_run(&self, options_json: String) -> Result<String> {
        self.inner.pro_run(&options_json).await.map_err(to_napi_err)
    }

    #[napi]
    pub async fn enterprise_run(&self) -> Result<()> {
        self.inner.enterprise_run().await.map_err(to_napi_err)
    }

    // =========================================================================
    // JACS Delegation
    // =========================================================================

    #[napi]
    pub async fn build_auth_header(&self) -> Result<String> {
        self.inner.build_auth_header().await.map_err(to_napi_err)
    }

    #[napi]
    pub async fn sign_message(&self, message: String) -> Result<String> {
        self.inner.sign_message(&message).await.map_err(to_napi_err)
    }

    #[napi]
    pub async fn sign_response(&self, payload_json: String) -> Result<String> {
        self.inner
            .sign_response(&payload_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn canonical_json(&self, value_json: String) -> Result<String> {
        self.inner
            .canonical_json(&value_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn verify_a2a_artifact(&self, wrapped_json: String) -> Result<String> {
        self.inner
            .verify_a2a_artifact(&wrapped_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn export_agent_json(&self) -> Result<String> {
        self.inner.export_agent_json().await.map_err(to_napi_err)
    }

    // =========================================================================
    // Client State
    // =========================================================================

    #[napi]
    pub async fn jacs_id(&self) -> Result<String> {
        Ok(self.inner.jacs_id().await)
    }

    #[napi]
    pub async fn base_url(&self) -> Result<String> {
        Ok(self.inner.base_url().await)
    }

    #[napi]
    pub async fn hai_agent_id(&self) -> Result<String> {
        Ok(self.inner.hai_agent_id().await)
    }

    #[napi]
    pub async fn agent_email(&self) -> Result<Option<String>> {
        Ok(self.inner.agent_email().await)
    }

    #[napi]
    pub async fn set_hai_agent_id(&self, id: String) -> Result<()> {
        self.inner.set_hai_agent_id(id).await;
        Ok(())
    }

    #[napi]
    pub async fn set_agent_email(&self, email: String) -> Result<()> {
        self.inner.set_agent_email(email).await;
        Ok(())
    }

    // =========================================================================
    // SSE Streaming
    // =========================================================================

    #[napi]
    pub async fn connect_sse(&self) -> Result<f64> {
        let handle = self.inner.connect_sse().await.map_err(to_napi_err)?;
        Ok(handle as f64)
    }

    #[napi]
    pub async fn sse_next_event(&self, handle: f64) -> Result<Option<String>> {
        hai_binding_core::sse_next_event(handle as u64)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn sse_close(&self, handle: f64) -> Result<()> {
        hai_binding_core::sse_close(handle as u64)
            .await
            .map_err(to_napi_err)
    }

    // =========================================================================
    // WebSocket Streaming
    // =========================================================================

    #[napi]
    pub async fn connect_ws(&self) -> Result<f64> {
        let handle = self.inner.connect_ws().await.map_err(to_napi_err)?;
        Ok(handle as f64)
    }

    #[napi]
    pub async fn ws_next_event(&self, handle: f64) -> Result<Option<String>> {
        hai_binding_core::ws_next_event(handle as u64)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn ws_close(&self, handle: f64) -> Result<()> {
        hai_binding_core::ws_close(handle as u64)
            .await
            .map_err(to_napi_err)
    }

    // =========================================================================
    // JACS Document Store (20 methods)
    //
    // napi-rs auto-converts Rust `snake_case` to JS `camelCase` at the JS
    // boundary, so `save_memory` -> `saveMemory` etc. `Vec<u8>` returns map
    // to a Node `Buffer`; `Option<String>` maps to `string | null`.
    // `Result<()>` maps to `Promise<void>`.
    // =========================================================================

    #[napi]
    pub async fn store_document(&self, signed_json: String) -> Result<String> {
        self.inner
            .store_document(signed_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn sign_and_store(&self, data_json: String) -> Result<String> {
        self.inner
            .sign_and_store(data_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn get_document(&self, key: String) -> Result<String> {
        self.inner.get_document(key).await.map_err(to_napi_err)
    }

    #[napi]
    pub async fn get_latest_document(&self, doc_id: String) -> Result<String> {
        self.inner
            .get_latest_document(doc_id)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn get_document_versions(&self, doc_id: String) -> Result<String> {
        self.inner
            .get_document_versions(doc_id)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn list_documents(&self, jacs_type: Option<String>) -> Result<String> {
        self.inner
            .list_documents(jacs_type)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn remove_document(&self, key: String) -> Result<()> {
        self.inner.remove_document(key).await.map_err(to_napi_err)
    }

    #[napi]
    pub async fn update_document(&self, doc_id: String, signed_json: String) -> Result<String> {
        self.inner
            .update_document(doc_id, signed_json)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn search_documents(&self, query: String, limit: u32, offset: u32) -> Result<String> {
        self.inner
            .search_documents(query, limit as usize, offset as usize)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn query_by_type(&self, doc_type: String, limit: u32, offset: u32) -> Result<String> {
        self.inner
            .query_by_type(doc_type, limit as usize, offset as usize)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn query_by_field(
        &self,
        field: String,
        value: String,
        limit: u32,
        offset: u32,
    ) -> Result<String> {
        self.inner
            .query_by_field(field, value, limit as usize, offset as usize)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn query_by_agent(
        &self,
        agent_id: String,
        limit: u32,
        offset: u32,
    ) -> Result<String> {
        self.inner
            .query_by_agent(agent_id, limit as usize, offset as usize)
            .await
            .map_err(to_napi_err)
    }

    #[napi]
    pub async fn storage_capabilities(&self) -> Result<String> {
        self.inner.storage_capabilities().await.map_err(to_napi_err)
    }

    #[napi]
    pub async fn save_memory(&self, content: Option<String>) -> Result<String> {
        self.inner.save_memory(content).await.map_err(to_napi_err)
    }

    #[napi]
    pub async fn save_soul(&self, content: Option<String>) -> Result<String> {
        self.inner.save_soul(content).await.map_err(to_napi_err)
    }

    #[napi]
    pub async fn get_memory(&self) -> Result<Option<String>> {
        self.inner.get_memory().await.map_err(to_napi_err)
    }

    #[napi]
    pub async fn get_soul(&self) -> Result<Option<String>> {
        self.inner.get_soul().await.map_err(to_napi_err)
    }

    #[napi]
    pub async fn store_text_file(&self, path: String) -> Result<String> {
        self.inner.store_text_file(path).await.map_err(to_napi_err)
    }

    #[napi]
    pub async fn store_image_file(&self, path: String) -> Result<String> {
        self.inner.store_image_file(path).await.map_err(to_napi_err)
    }

    /// Fetch raw record bytes (no UTF-8 decode, no JSON parse). Returns a
    /// Node `Buffer` (assignable to `Uint8Array`).
    #[napi]
    pub async fn get_record_bytes(&self, key: String) -> Result<Buffer> {
        let bytes = self
            .inner
            .get_record_bytes(key)
            .await
            .map_err(to_napi_err)?;
        Ok(Buffer::from(bytes))
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use hai_binding_core::{ErrorKind, HaiBindingError};

    #[test]
    fn to_napi_err_formats_kind_and_message() {
        let err = HaiBindingError::new(ErrorKind::AuthFailed, "token expired");
        let napi_err = to_napi_err(err);
        let msg = format!("{}", napi_err);
        assert!(msg.contains("AuthFailed"));
        assert!(msg.contains("token expired"));
    }

    #[test]
    fn to_napi_err_handles_all_kinds() {
        let kinds = vec![
            ErrorKind::ConfigFailed,
            ErrorKind::AuthFailed,
            ErrorKind::RateLimited,
            ErrorKind::NotFound,
            ErrorKind::ApiError,
            ErrorKind::NetworkFailed,
            ErrorKind::SerializationFailed,
            ErrorKind::InvalidArgument,
            ErrorKind::ProviderError,
            ErrorKind::Generic,
        ];
        for kind in kinds {
            let err = HaiBindingError::new(kind, "test message");
            let napi_err = to_napi_err(err);
            // Should not panic for any error kind
            let msg = format!("{}", napi_err);
            assert!(msg.contains("test message"));
        }
    }
}
