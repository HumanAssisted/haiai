//! Python bindings for HAI SDK via PyO3.
//!
//! Every `HaiClientWrapper` method is exposed as both:
//! - An async method (returns Python coroutine for asyncio users)
//! - A sync method with `_sync` suffix (blocks, releases GIL)
//!
//! All `_sync` methods include a deadlock guard that detects calls from within
//! an async context and raises `RuntimeError` instead of panicking.
//!
//! No business logic lives here -- each method is thin delegation
//! to `hai-binding-core`.

use std::sync::Arc;

use hai_binding_core::{HaiBindingError, HaiClientWrapper, RT};
use pyo3::prelude::*;

// =============================================================================
// Error Conversion
// =============================================================================

fn to_py_err(e: HaiBindingError) -> PyErr {
    PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}: {}", e.kind, e.message))
}

/// Guard against calling sync methods from within an async context.
/// `RT.block_on()` will panic if called from within a tokio runtime context.
fn check_not_async() -> PyResult<()> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(pyo3::exceptions::PyRuntimeError::new_err(
            "Cannot call sync methods from within an async context; use the async variant instead"
        ));
    }
    Ok(())
}

// =============================================================================
// HaiClient class
// =============================================================================

/// Python-facing HAI client.
///
/// Constructor accepts a JSON config string. Methods come in async/sync pairs.
#[pyclass]
pub struct HaiClient {
    inner: Arc<HaiClientWrapper>,
}

#[pymethods]
impl HaiClient {
    /// Create a new HaiClient from a config JSON string.
    #[new]
    fn new(config_json: String) -> PyResult<Self> {
        let wrapper = HaiClientWrapper::from_config_json_auto(&config_json)
            .map_err(to_py_err)?;

        Ok(Self {
            inner: Arc::new(wrapper),
        })
    }

    // =========================================================================
    // Registration & Identity
    // =========================================================================

    fn hello<'py>(&self, py: Python<'py>, include_test: bool) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.hello(include_test).await.map_err(to_py_err)
        })
    }

    fn hello_sync(&self, py: Python, include_test: bool) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.hello(include_test).await })
        }).map_err(to_py_err)
    }

    fn check_username<'py>(&self, py: Python<'py>, username: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.check_username(&username).await.map_err(to_py_err)
        })
    }

    fn check_username_sync(&self, py: Python, username: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.check_username(&username).await })
        }).map_err(to_py_err)
    }

    fn register<'py>(&self, py: Python<'py>, options_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.register(&options_json).await.map_err(to_py_err)
        })
    }

    fn register_sync(&self, py: Python, options_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.register(&options_json).await })
        }).map_err(to_py_err)
    }

    fn rotate_keys<'py>(&self, py: Python<'py>, options_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.rotate_keys(&options_json).await.map_err(to_py_err)
        })
    }

    fn rotate_keys_sync(&self, py: Python, options_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.rotate_keys(&options_json).await })
        }).map_err(to_py_err)
    }

    fn update_agent<'py>(&self, py: Python<'py>, new_agent_data: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.update_agent(&new_agent_data).await.map_err(to_py_err)
        })
    }

    fn update_agent_sync(&self, py: Python, new_agent_data: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.update_agent(&new_agent_data).await })
        }).map_err(to_py_err)
    }

    fn submit_response<'py>(&self, py: Python<'py>, params_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.submit_response(&params_json).await.map_err(to_py_err)
        })
    }

    fn submit_response_sync(&self, py: Python, params_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.submit_response(&params_json).await })
        }).map_err(to_py_err)
    }

    #[pyo3(signature = (agent_id=None))]
    fn verify_status<'py>(&self, py: Python<'py>, agent_id: Option<String>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.verify_status(agent_id.as_deref()).await.map_err(to_py_err)
        })
    }

    #[pyo3(signature = (agent_id=None))]
    fn verify_status_sync(&self, py: Python, agent_id: Option<String>) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.verify_status(agent_id.as_deref()).await })
        }).map_err(to_py_err)
    }

    // =========================================================================
    // Username
    // =========================================================================

    fn claim_username<'py>(&self, py: Python<'py>, agent_id: String, username: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.claim_username(&agent_id, &username).await.map_err(to_py_err)
        })
    }

    fn claim_username_sync(&self, py: Python, agent_id: String, username: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.claim_username(&agent_id, &username).await })
        }).map_err(to_py_err)
    }

    fn update_username<'py>(&self, py: Python<'py>, agent_id: String, username: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.update_username(&agent_id, &username).await.map_err(to_py_err)
        })
    }

    fn update_username_sync(&self, py: Python, agent_id: String, username: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.update_username(&agent_id, &username).await })
        }).map_err(to_py_err)
    }

    fn delete_username<'py>(&self, py: Python<'py>, agent_id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.delete_username(&agent_id).await.map_err(to_py_err)
        })
    }

    fn delete_username_sync(&self, py: Python, agent_id: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.delete_username(&agent_id).await })
        }).map_err(to_py_err)
    }

    // =========================================================================
    // Email Core
    // =========================================================================

    fn send_email<'py>(&self, py: Python<'py>, options_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.send_email(&options_json).await.map_err(to_py_err)
        })
    }

    fn send_email_sync(&self, py: Python, options_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.send_email(&options_json).await })
        }).map_err(to_py_err)
    }

    fn send_signed_email<'py>(&self, py: Python<'py>, options_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.send_signed_email(&options_json).await.map_err(to_py_err)
        })
    }

    fn send_signed_email_sync(&self, py: Python, options_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.send_signed_email(&options_json).await })
        }).map_err(to_py_err)
    }

    fn list_messages<'py>(&self, py: Python<'py>, options_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.list_messages(&options_json).await.map_err(to_py_err)
        })
    }

    fn list_messages_sync(&self, py: Python, options_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.list_messages(&options_json).await })
        }).map_err(to_py_err)
    }

    fn update_labels<'py>(&self, py: Python<'py>, params_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.update_labels(&params_json).await.map_err(to_py_err)
        })
    }

    fn update_labels_sync(&self, py: Python, params_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.update_labels(&params_json).await })
        }).map_err(to_py_err)
    }

    fn get_email_status<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.get_email_status().await.map_err(to_py_err)
        })
    }

    fn get_email_status_sync(&self, py: Python) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.get_email_status().await })
        }).map_err(to_py_err)
    }

    fn get_message<'py>(&self, py: Python<'py>, message_id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.get_message(&message_id).await.map_err(to_py_err)
        })
    }

    fn get_message_sync(&self, py: Python, message_id: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.get_message(&message_id).await })
        }).map_err(to_py_err)
    }

    fn get_unread_count<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.get_unread_count().await.map_err(to_py_err)
        })
    }

    fn get_unread_count_sync(&self, py: Python) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.get_unread_count().await })
        }).map_err(to_py_err)
    }

    // =========================================================================
    // Email Actions
    // =========================================================================

    fn mark_read<'py>(&self, py: Python<'py>, message_id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.mark_read(&message_id).await.map_err(to_py_err)?;
            Ok(())
        })
    }

    fn mark_read_sync(&self, py: Python, message_id: String) -> PyResult<()> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.mark_read(&message_id).await })
        }).map_err(to_py_err)
    }

    fn mark_unread<'py>(&self, py: Python<'py>, message_id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.mark_unread(&message_id).await.map_err(to_py_err)?;
            Ok(())
        })
    }

    fn mark_unread_sync(&self, py: Python, message_id: String) -> PyResult<()> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.mark_unread(&message_id).await })
        }).map_err(to_py_err)
    }

    fn delete_message<'py>(&self, py: Python<'py>, message_id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.delete_message(&message_id).await.map_err(to_py_err)?;
            Ok(())
        })
    }

    fn delete_message_sync(&self, py: Python, message_id: String) -> PyResult<()> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.delete_message(&message_id).await })
        }).map_err(to_py_err)
    }

    fn archive<'py>(&self, py: Python<'py>, message_id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.archive(&message_id).await.map_err(to_py_err)?;
            Ok(())
        })
    }

    fn archive_sync(&self, py: Python, message_id: String) -> PyResult<()> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.archive(&message_id).await })
        }).map_err(to_py_err)
    }

    fn unarchive<'py>(&self, py: Python<'py>, message_id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.unarchive(&message_id).await.map_err(to_py_err)?;
            Ok(())
        })
    }

    fn unarchive_sync(&self, py: Python, message_id: String) -> PyResult<()> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.unarchive(&message_id).await })
        }).map_err(to_py_err)
    }

    fn reply_with_options<'py>(&self, py: Python<'py>, params_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.reply_with_options(&params_json).await.map_err(to_py_err)
        })
    }

    fn reply_with_options_sync(&self, py: Python, params_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.reply_with_options(&params_json).await })
        }).map_err(to_py_err)
    }

    fn forward<'py>(&self, py: Python<'py>, params_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.forward(&params_json).await.map_err(to_py_err)
        })
    }

    fn forward_sync(&self, py: Python, params_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.forward(&params_json).await })
        }).map_err(to_py_err)
    }

    // =========================================================================
    // Search & Contacts
    // =========================================================================

    fn search_messages<'py>(&self, py: Python<'py>, options_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.search_messages(&options_json).await.map_err(to_py_err)
        })
    }

    fn search_messages_sync(&self, py: Python, options_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.search_messages(&options_json).await })
        }).map_err(to_py_err)
    }

    fn contacts<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.contacts().await.map_err(to_py_err)
        })
    }

    fn contacts_sync(&self, py: Python) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.contacts().await })
        }).map_err(to_py_err)
    }

    // =========================================================================
    // Key Operations
    // =========================================================================

    fn fetch_remote_key<'py>(&self, py: Python<'py>, jacs_id: String, version: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.fetch_remote_key(&jacs_id, &version).await.map_err(to_py_err)
        })
    }

    fn fetch_remote_key_sync(&self, py: Python, jacs_id: String, version: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.fetch_remote_key(&jacs_id, &version).await })
        }).map_err(to_py_err)
    }

    fn fetch_key_by_hash<'py>(&self, py: Python<'py>, hash: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.fetch_key_by_hash(&hash).await.map_err(to_py_err)
        })
    }

    fn fetch_key_by_hash_sync(&self, py: Python, hash: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.fetch_key_by_hash(&hash).await })
        }).map_err(to_py_err)
    }

    fn fetch_key_by_email<'py>(&self, py: Python<'py>, email: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.fetch_key_by_email(&email).await.map_err(to_py_err)
        })
    }

    fn fetch_key_by_email_sync(&self, py: Python, email: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.fetch_key_by_email(&email).await })
        }).map_err(to_py_err)
    }

    fn fetch_key_by_domain<'py>(&self, py: Python<'py>, domain: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.fetch_key_by_domain(&domain).await.map_err(to_py_err)
        })
    }

    fn fetch_key_by_domain_sync(&self, py: Python, domain: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.fetch_key_by_domain(&domain).await })
        }).map_err(to_py_err)
    }

    fn fetch_all_keys<'py>(&self, py: Python<'py>, jacs_id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.fetch_all_keys(&jacs_id).await.map_err(to_py_err)
        })
    }

    fn fetch_all_keys_sync(&self, py: Python, jacs_id: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.fetch_all_keys(&jacs_id).await })
        }).map_err(to_py_err)
    }

    // =========================================================================
    // Verification
    // =========================================================================

    fn verify_document<'py>(&self, py: Python<'py>, document: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.verify_document(&document).await.map_err(to_py_err)
        })
    }

    fn verify_document_sync(&self, py: Python, document: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.verify_document(&document).await })
        }).map_err(to_py_err)
    }

    fn get_verification<'py>(&self, py: Python<'py>, agent_id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.get_verification(&agent_id).await.map_err(to_py_err)
        })
    }

    fn get_verification_sync(&self, py: Python, agent_id: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.get_verification(&agent_id).await })
        }).map_err(to_py_err)
    }

    fn verify_agent_document<'py>(&self, py: Python<'py>, request_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.verify_agent_document(&request_json).await.map_err(to_py_err)
        })
    }

    fn verify_agent_document_sync(&self, py: Python, request_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.verify_agent_document(&request_json).await })
        }).map_err(to_py_err)
    }

    // =========================================================================
    // Benchmarks
    // =========================================================================

    #[pyo3(signature = (name=None, tier=None))]
    fn benchmark<'py>(&self, py: Python<'py>, name: Option<String>, tier: Option<String>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.benchmark(name.as_deref(), tier.as_deref()).await.map_err(to_py_err)
        })
    }

    #[pyo3(signature = (name=None, tier=None))]
    fn benchmark_sync(&self, py: Python, name: Option<String>, tier: Option<String>) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.benchmark(name.as_deref(), tier.as_deref()).await })
        }).map_err(to_py_err)
    }

    #[pyo3(signature = (transport=None))]
    fn free_run<'py>(&self, py: Python<'py>, transport: Option<String>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.free_run(transport.as_deref()).await.map_err(to_py_err)
        })
    }

    #[pyo3(signature = (transport=None))]
    fn free_run_sync(&self, py: Python, transport: Option<String>) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.free_run(transport.as_deref()).await })
        }).map_err(to_py_err)
    }

    fn pro_run<'py>(&self, py: Python<'py>, options_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.pro_run(&options_json).await.map_err(to_py_err)
        })
    }

    fn pro_run_sync(&self, py: Python, options_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.pro_run(&options_json).await })
        }).map_err(to_py_err)
    }

    fn enterprise_run<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.enterprise_run().await.map_err(to_py_err)?;
            Ok(())
        })
    }

    fn enterprise_run_sync(&self, py: Python) -> PyResult<()> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.enterprise_run().await })
        }).map_err(to_py_err)
    }

    // =========================================================================
    // JACS Delegation
    // =========================================================================

    fn build_auth_header<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.build_auth_header().await.map_err(to_py_err)
        })
    }

    fn build_auth_header_sync(&self, py: Python) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.build_auth_header().await })
        }).map_err(to_py_err)
    }

    fn sign_message<'py>(&self, py: Python<'py>, message: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.sign_message(&message).await.map_err(to_py_err)
        })
    }

    fn sign_message_sync(&self, py: Python, message: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.sign_message(&message).await })
        }).map_err(to_py_err)
    }

    fn canonical_json<'py>(&self, py: Python<'py>, value_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.canonical_json(&value_json).await.map_err(to_py_err)
        })
    }

    fn canonical_json_sync(&self, py: Python, value_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.canonical_json(&value_json).await })
        }).map_err(to_py_err)
    }

    fn verify_a2a_artifact<'py>(&self, py: Python<'py>, wrapped_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.verify_a2a_artifact(&wrapped_json).await.map_err(to_py_err)
        })
    }

    fn verify_a2a_artifact_sync(&self, py: Python, wrapped_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.verify_a2a_artifact(&wrapped_json).await })
        }).map_err(to_py_err)
    }

    fn export_agent_json<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.export_agent_json().await.map_err(to_py_err)
        })
    }

    fn export_agent_json_sync(&self, py: Python) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.export_agent_json().await })
        }).map_err(to_py_err)
    }

    // =========================================================================
    // Client State
    // =========================================================================

    fn jacs_id<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            Ok(client.jacs_id().await)
        })
    }

    fn jacs_id_sync(&self, py: Python) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        Ok(py.detach(|| {
            RT.block_on(async { client.jacs_id().await })
        }))
    }

    fn set_hai_agent_id<'py>(&self, py: Python<'py>, id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.set_hai_agent_id(id).await;
            Ok(())
        })
    }

    fn set_hai_agent_id_sync(&self, py: Python, id: String) -> PyResult<()> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.set_hai_agent_id(id).await })
        });
        Ok(())
    }

    fn set_agent_email<'py>(&self, py: Python<'py>, email: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.set_agent_email(email).await;
            Ok(())
        })
    }

    fn set_agent_email_sync(&self, py: Python, email: String) -> PyResult<()> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.set_agent_email(email).await })
        });
        Ok(())
    }
}

// =============================================================================
// Module registration
// =============================================================================

#[pymodule]
fn haiipy(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<HaiClient>()?;
    Ok(())
}

// Note: Rust-side unit tests for haiipy cannot be compiled in the standard
// `cargo test` workflow because the cdylib links against Python symbols which
// are only available when building via maturin or inside a Python environment.
// Error conversion and deadlock guard logic is exercised via Python-level tests
// (e.g., `pip install -e ".[dev]" && pytest`) and covered indirectly by the
// hai-binding-core test suite which tests the shared logic.
