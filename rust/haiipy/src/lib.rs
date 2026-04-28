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

use hai_binding_core::{HaiBindingError, HaiClientWrapper};
use pyo3::prelude::*;

/// Own static tokio runtime for haiipy sync methods (`_sync` variants).
/// Each FFI binding manages its own runtime:
/// - haiinpm: napi-rs built-in async runtime
/// - haiigo: own static RT for spawn+channel pattern
/// - haiipy: this RT for `block_on()` in sync wrappers
static RT: std::sync::LazyLock<tokio::runtime::Runtime> = std::sync::LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create haiipy tokio runtime")
});

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
            "Cannot call sync methods from within an async context; use the async variant instead",
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
        let wrapper = HaiClientWrapper::from_config_json_auto(&config_json).map_err(to_py_err)?;

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
        py.detach(|| RT.block_on(async { client.hello(include_test).await }))
            .map_err(to_py_err)
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
        py.detach(|| RT.block_on(async { client.register(&options_json).await }))
            .map_err(to_py_err)
    }

    fn register_new_agent<'py>(
        &self,
        py: Python<'py>,
        options_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .register_new_agent(&options_json)
                .await
                .map_err(to_py_err)
        })
    }

    fn register_new_agent_sync(&self, py: Python, options_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.register_new_agent(&options_json).await }))
            .map_err(to_py_err)
    }

    fn rotate_keys<'py>(
        &self,
        py: Python<'py>,
        options_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.rotate_keys(&options_json).await.map_err(to_py_err)
        })
    }

    fn rotate_keys_sync(&self, py: Python, options_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.rotate_keys(&options_json).await }))
            .map_err(to_py_err)
    }

    fn update_agent<'py>(
        &self,
        py: Python<'py>,
        new_agent_data: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .update_agent(&new_agent_data)
                .await
                .map_err(to_py_err)
        })
    }

    fn update_agent_sync(&self, py: Python, new_agent_data: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.update_agent(&new_agent_data).await }))
            .map_err(to_py_err)
    }

    fn submit_response<'py>(
        &self,
        py: Python<'py>,
        params_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .submit_response(&params_json)
                .await
                .map_err(to_py_err)
        })
    }

    fn submit_response_sync(&self, py: Python, params_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.submit_response(&params_json).await }))
            .map_err(to_py_err)
    }

    #[pyo3(signature = (agent_id=None))]
    fn verify_status<'py>(
        &self,
        py: Python<'py>,
        agent_id: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .verify_status(agent_id.as_deref())
                .await
                .map_err(to_py_err)
        })
    }

    #[pyo3(signature = (agent_id=None))]
    fn verify_status_sync(&self, py: Python, agent_id: Option<String>) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.verify_status(agent_id.as_deref()).await }))
            .map_err(to_py_err)
    }

    // =========================================================================
    // Username
    // =========================================================================

    fn update_username<'py>(
        &self,
        py: Python<'py>,
        agent_id: String,
        username: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .update_username(&agent_id, &username)
                .await
                .map_err(to_py_err)
        })
    }

    fn update_username_sync(
        &self,
        py: Python,
        agent_id: String,
        username: String,
    ) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.update_username(&agent_id, &username).await }))
            .map_err(to_py_err)
    }

    fn delete_username<'py>(
        &self,
        py: Python<'py>,
        agent_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.delete_username(&agent_id).await.map_err(to_py_err)
        })
    }

    fn delete_username_sync(&self, py: Python, agent_id: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.delete_username(&agent_id).await }))
            .map_err(to_py_err)
    }

    // =========================================================================
    // Email Core
    // =========================================================================

    fn send_email<'py>(
        &self,
        py: Python<'py>,
        options_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.send_email(&options_json).await.map_err(to_py_err)
        })
    }

    fn send_email_sync(&self, py: Python, options_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.send_email(&options_json).await }))
            .map_err(to_py_err)
    }

    fn send_signed_email<'py>(
        &self,
        py: Python<'py>,
        options_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .send_signed_email(&options_json)
                .await
                .map_err(to_py_err)
        })
    }

    fn send_signed_email_sync(&self, py: Python, options_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.send_signed_email(&options_json).await }))
            .map_err(to_py_err)
    }

    fn list_messages<'py>(
        &self,
        py: Python<'py>,
        options_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.list_messages(&options_json).await.map_err(to_py_err)
        })
    }

    fn list_messages_sync(&self, py: Python, options_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.list_messages(&options_json).await }))
            .map_err(to_py_err)
    }

    fn update_labels<'py>(
        &self,
        py: Python<'py>,
        params_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.update_labels(&params_json).await.map_err(to_py_err)
        })
    }

    fn update_labels_sync(&self, py: Python, params_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.update_labels(&params_json).await }))
            .map_err(to_py_err)
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
        py.detach(|| RT.block_on(async { client.get_email_status().await }))
            .map_err(to_py_err)
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
        py.detach(|| RT.block_on(async { client.get_message(&message_id).await }))
            .map_err(to_py_err)
    }

    fn get_raw_email<'py>(
        &self,
        py: Python<'py>,
        message_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.get_raw_email(&message_id).await.map_err(to_py_err)
        })
    }

    fn get_raw_email_sync(&self, py: Python, message_id: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.get_raw_email(&message_id).await }))
            .map_err(to_py_err)
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
        py.detach(|| RT.block_on(async { client.get_unread_count().await }))
            .map_err(to_py_err)
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
        py.detach(|| RT.block_on(async { client.mark_read(&message_id).await }))
            .map_err(to_py_err)
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
        py.detach(|| RT.block_on(async { client.mark_unread(&message_id).await }))
            .map_err(to_py_err)
    }

    fn delete_message<'py>(
        &self,
        py: Python<'py>,
        message_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .delete_message(&message_id)
                .await
                .map_err(to_py_err)?;
            Ok(())
        })
    }

    fn delete_message_sync(&self, py: Python, message_id: String) -> PyResult<()> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.delete_message(&message_id).await }))
            .map_err(to_py_err)
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
        py.detach(|| RT.block_on(async { client.archive(&message_id).await }))
            .map_err(to_py_err)
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
        py.detach(|| RT.block_on(async { client.unarchive(&message_id).await }))
            .map_err(to_py_err)
    }

    fn reply_with_options<'py>(
        &self,
        py: Python<'py>,
        params_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .reply_with_options(&params_json)
                .await
                .map_err(to_py_err)
        })
    }

    fn reply_with_options_sync(&self, py: Python, params_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.reply_with_options(&params_json).await }))
            .map_err(to_py_err)
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
        py.detach(|| RT.block_on(async { client.forward(&params_json).await }))
            .map_err(to_py_err)
    }

    // =========================================================================
    // Search & Contacts
    // =========================================================================

    fn search_messages<'py>(
        &self,
        py: Python<'py>,
        options_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .search_messages(&options_json)
                .await
                .map_err(to_py_err)
        })
    }

    fn search_messages_sync(&self, py: Python, options_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.search_messages(&options_json).await }))
            .map_err(to_py_err)
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
        py.detach(|| RT.block_on(async { client.contacts().await }))
            .map_err(to_py_err)
    }

    // =========================================================================
    // Server Keys
    // =========================================================================

    fn fetch_server_keys<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.fetch_server_keys().await.map_err(to_py_err)
        })
    }

    fn fetch_server_keys_sync(&self, py: Python) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.fetch_server_keys().await }))
            .map_err(to_py_err)
    }

    // =========================================================================
    // Raw Email Sign/Verify
    // =========================================================================

    fn sign_email_raw<'py>(
        &self,
        py: Python<'py>,
        raw_email_b64: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .sign_email_raw(&raw_email_b64)
                .await
                .map_err(to_py_err)
        })
    }

    fn sign_email_raw_sync(&self, py: Python, raw_email_b64: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.sign_email_raw(&raw_email_b64).await }))
            .map_err(to_py_err)
    }

    fn verify_email_raw<'py>(
        &self,
        py: Python<'py>,
        raw_email_b64: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .verify_email_raw(&raw_email_b64)
                .await
                .map_err(to_py_err)
        })
    }

    fn verify_email_raw_sync(&self, py: Python, raw_email_b64: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.verify_email_raw(&raw_email_b64).await }))
            .map_err(to_py_err)
    }

    // =========================================================================
    // Local Media Sign/Verify (Layer 8 / TASK_007)
    // =========================================================================

    fn sign_text<'py>(
        &self,
        py: Python<'py>,
        path: String,
        opts_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.sign_text(&path, &opts_json).await.map_err(to_py_err)
        })
    }

    fn sign_text_sync(&self, py: Python, path: String, opts_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.sign_text(&path, &opts_json).await }))
            .map_err(to_py_err)
    }

    fn verify_text<'py>(
        &self,
        py: Python<'py>,
        path: String,
        opts_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .verify_text(&path, &opts_json)
                .await
                .map_err(to_py_err)
        })
    }

    fn verify_text_sync(&self, py: Python, path: String, opts_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.verify_text(&path, &opts_json).await }))
            .map_err(to_py_err)
    }

    fn sign_image<'py>(
        &self,
        py: Python<'py>,
        in_path: String,
        out_path: String,
        opts_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .sign_image(&in_path, &out_path, &opts_json)
                .await
                .map_err(to_py_err)
        })
    }

    fn sign_image_sync(
        &self,
        py: Python,
        in_path: String,
        out_path: String,
        opts_json: String,
    ) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.sign_image(&in_path, &out_path, &opts_json).await })
        })
        .map_err(to_py_err)
    }

    fn verify_image<'py>(
        &self,
        py: Python<'py>,
        path: String,
        opts_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .verify_image(&path, &opts_json)
                .await
                .map_err(to_py_err)
        })
    }

    fn verify_image_sync(&self, py: Python, path: String, opts_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.verify_image(&path, &opts_json).await }))
            .map_err(to_py_err)
    }

    fn extract_media_signature<'py>(
        &self,
        py: Python<'py>,
        path: String,
        opts_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .extract_media_signature(&path, &opts_json)
                .await
                .map_err(to_py_err)
        })
    }

    fn extract_media_signature_sync(
        &self,
        py: Python,
        path: String,
        opts_json: String,
    ) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.extract_media_signature(&path, &opts_json).await }))
            .map_err(to_py_err)
    }

    // =========================================================================
    // Attestations
    // =========================================================================

    fn create_attestation<'py>(
        &self,
        py: Python<'py>,
        params_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .create_attestation(&params_json)
                .await
                .map_err(to_py_err)
        })
    }

    fn create_attestation_sync(&self, py: Python, params_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.create_attestation(&params_json).await }))
            .map_err(to_py_err)
    }

    fn list_attestations<'py>(
        &self,
        py: Python<'py>,
        params_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .list_attestations(&params_json)
                .await
                .map_err(to_py_err)
        })
    }

    fn list_attestations_sync(&self, py: Python, params_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.list_attestations(&params_json).await }))
            .map_err(to_py_err)
    }

    fn get_attestation<'py>(
        &self,
        py: Python<'py>,
        agent_id: String,
        doc_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_attestation(&agent_id, &doc_id)
                .await
                .map_err(to_py_err)
        })
    }

    fn get_attestation_sync(
        &self,
        py: Python,
        agent_id: String,
        doc_id: String,
    ) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.get_attestation(&agent_id, &doc_id).await }))
            .map_err(to_py_err)
    }

    fn verify_attestation<'py>(
        &self,
        py: Python<'py>,
        document: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .verify_attestation(&document)
                .await
                .map_err(to_py_err)
        })
    }

    fn verify_attestation_sync(&self, py: Python, document: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.verify_attestation(&document).await }))
            .map_err(to_py_err)
    }

    // =========================================================================
    // Email Templates
    // =========================================================================

    fn create_email_template<'py>(
        &self,
        py: Python<'py>,
        options_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .create_email_template(&options_json)
                .await
                .map_err(to_py_err)
        })
    }

    fn create_email_template_sync(&self, py: Python, options_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.create_email_template(&options_json).await }))
            .map_err(to_py_err)
    }

    fn list_email_templates<'py>(
        &self,
        py: Python<'py>,
        options_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .list_email_templates(&options_json)
                .await
                .map_err(to_py_err)
        })
    }

    fn list_email_templates_sync(&self, py: Python, options_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.list_email_templates(&options_json).await }))
            .map_err(to_py_err)
    }

    fn get_email_template<'py>(
        &self,
        py: Python<'py>,
        template_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_email_template(&template_id)
                .await
                .map_err(to_py_err)
        })
    }

    fn get_email_template_sync(&self, py: Python, template_id: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.get_email_template(&template_id).await }))
            .map_err(to_py_err)
    }

    fn update_email_template<'py>(
        &self,
        py: Python<'py>,
        template_id: String,
        options_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .update_email_template(&template_id, &options_json)
                .await
                .map_err(to_py_err)
        })
    }

    fn update_email_template_sync(
        &self,
        py: Python,
        template_id: String,
        options_json: String,
    ) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async {
                client
                    .update_email_template(&template_id, &options_json)
                    .await
            })
        })
        .map_err(to_py_err)
    }

    fn delete_email_template<'py>(
        &self,
        py: Python<'py>,
        template_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .delete_email_template(&template_id)
                .await
                .map_err(to_py_err)?;
            Ok(())
        })
    }

    fn delete_email_template_sync(&self, py: Python, template_id: String) -> PyResult<()> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.delete_email_template(&template_id).await }))
            .map_err(to_py_err)
    }

    // =========================================================================
    // Key Operations
    // =========================================================================

    fn fetch_remote_key<'py>(
        &self,
        py: Python<'py>,
        jacs_id: String,
        version: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .fetch_remote_key(&jacs_id, &version)
                .await
                .map_err(to_py_err)
        })
    }

    fn fetch_remote_key_sync(
        &self,
        py: Python,
        jacs_id: String,
        version: String,
    ) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.fetch_remote_key(&jacs_id, &version).await }))
            .map_err(to_py_err)
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
        py.detach(|| RT.block_on(async { client.fetch_key_by_hash(&hash).await }))
            .map_err(to_py_err)
    }

    fn fetch_key_by_email<'py>(
        &self,
        py: Python<'py>,
        email: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.fetch_key_by_email(&email).await.map_err(to_py_err)
        })
    }

    fn fetch_key_by_email_sync(&self, py: Python, email: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.fetch_key_by_email(&email).await }))
            .map_err(to_py_err)
    }

    fn fetch_key_by_domain<'py>(
        &self,
        py: Python<'py>,
        domain: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.fetch_key_by_domain(&domain).await.map_err(to_py_err)
        })
    }

    fn fetch_key_by_domain_sync(&self, py: Python, domain: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.fetch_key_by_domain(&domain).await }))
            .map_err(to_py_err)
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
        py.detach(|| RT.block_on(async { client.fetch_all_keys(&jacs_id).await }))
            .map_err(to_py_err)
    }

    // =========================================================================
    // Verification
    // =========================================================================

    fn verify_document<'py>(
        &self,
        py: Python<'py>,
        document: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.verify_document(&document).await.map_err(to_py_err)
        })
    }

    fn verify_document_sync(&self, py: Python, document: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.verify_document(&document).await }))
            .map_err(to_py_err)
    }

    fn get_verification<'py>(
        &self,
        py: Python<'py>,
        agent_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.get_verification(&agent_id).await.map_err(to_py_err)
        })
    }

    fn get_verification_sync(&self, py: Python, agent_id: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.get_verification(&agent_id).await }))
            .map_err(to_py_err)
    }

    fn verify_agent_document<'py>(
        &self,
        py: Python<'py>,
        request_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .verify_agent_document(&request_json)
                .await
                .map_err(to_py_err)
        })
    }

    fn verify_agent_document_sync(&self, py: Python, request_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.verify_agent_document(&request_json).await }))
            .map_err(to_py_err)
    }

    // =========================================================================
    // Benchmarks
    // =========================================================================

    #[pyo3(signature = (name=None, tier=None))]
    fn benchmark<'py>(
        &self,
        py: Python<'py>,
        name: Option<String>,
        tier: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .benchmark(name.as_deref(), tier.as_deref())
                .await
                .map_err(to_py_err)
        })
    }

    #[pyo3(signature = (name=None, tier=None))]
    fn benchmark_sync(
        &self,
        py: Python,
        name: Option<String>,
        tier: Option<String>,
    ) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.benchmark(name.as_deref(), tier.as_deref()).await })
        })
        .map_err(to_py_err)
    }

    #[pyo3(signature = (transport=None))]
    fn free_run<'py>(
        &self,
        py: Python<'py>,
        transport: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .free_run(transport.as_deref())
                .await
                .map_err(to_py_err)
        })
    }

    #[pyo3(signature = (transport=None))]
    fn free_run_sync(&self, py: Python, transport: Option<String>) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.free_run(transport.as_deref()).await }))
            .map_err(to_py_err)
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
        py.detach(|| RT.block_on(async { client.pro_run(&options_json).await }))
            .map_err(to_py_err)
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
        py.detach(|| RT.block_on(async { client.enterprise_run().await }))
            .map_err(to_py_err)
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
        py.detach(|| RT.block_on(async { client.build_auth_header().await }))
            .map_err(to_py_err)
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
        py.detach(|| RT.block_on(async { client.sign_message(&message).await }))
            .map_err(to_py_err)
    }

    fn canonical_json<'py>(
        &self,
        py: Python<'py>,
        value_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.canonical_json(&value_json).await.map_err(to_py_err)
        })
    }

    fn canonical_json_sync(&self, py: Python, value_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.canonical_json(&value_json).await }))
            .map_err(to_py_err)
    }

    fn verify_a2a_artifact<'py>(
        &self,
        py: Python<'py>,
        wrapped_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .verify_a2a_artifact(&wrapped_json)
                .await
                .map_err(to_py_err)
        })
    }

    fn verify_a2a_artifact_sync(&self, py: Python, wrapped_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.verify_a2a_artifact(&wrapped_json).await }))
            .map_err(to_py_err)
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
        py.detach(|| RT.block_on(async { client.export_agent_json().await }))
            .map_err(to_py_err)
    }

    // =========================================================================
    // Client State
    // =========================================================================

    fn jacs_id<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move { Ok(client.jacs_id().await) })
    }

    fn jacs_id_sync(&self, py: Python) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        Ok(py.detach(|| RT.block_on(async { client.jacs_id().await })))
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
        py.detach(|| RT.block_on(async { client.set_hai_agent_id(id).await }));
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
        py.detach(|| RT.block_on(async { client.set_agent_email(email).await }));
        Ok(())
    }

    // =========================================================================
    // SSE Streaming
    // =========================================================================

    fn connect_sse<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect_sse().await.map_err(to_py_err)
        })
    }

    fn connect_sse_sync(&self, py: Python) -> PyResult<u64> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.connect_sse().await }))
            .map_err(to_py_err)
    }

    fn sse_next_event<'py>(&self, py: Python<'py>, handle: u64) -> PyResult<Bound<'py, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            hai_binding_core::sse_next_event(handle)
                .await
                .map_err(to_py_err)
        })
    }

    fn sse_next_event_sync(&self, py: Python, handle: u64) -> PyResult<Option<String>> {
        check_not_async()?;
        py.detach(|| RT.block_on(async { hai_binding_core::sse_next_event(handle).await }))
            .map_err(to_py_err)
    }

    fn sse_close<'py>(&self, py: Python<'py>, handle: u64) -> PyResult<Bound<'py, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            hai_binding_core::sse_close(handle)
                .await
                .map_err(to_py_err)?;
            Ok(())
        })
    }

    fn sse_close_sync(&self, py: Python, handle: u64) -> PyResult<()> {
        check_not_async()?;
        py.detach(|| RT.block_on(async { hai_binding_core::sse_close(handle).await }))
            .map_err(to_py_err)
    }

    // =========================================================================
    // WebSocket Streaming
    // =========================================================================

    fn connect_ws<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect_ws().await.map_err(to_py_err)
        })
    }

    fn connect_ws_sync(&self, py: Python) -> PyResult<u64> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.connect_ws().await }))
            .map_err(to_py_err)
    }

    fn ws_next_event<'py>(&self, py: Python<'py>, handle: u64) -> PyResult<Bound<'py, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            hai_binding_core::ws_next_event(handle)
                .await
                .map_err(to_py_err)
        })
    }

    fn ws_next_event_sync(&self, py: Python, handle: u64) -> PyResult<Option<String>> {
        check_not_async()?;
        py.detach(|| RT.block_on(async { hai_binding_core::ws_next_event(handle).await }))
            .map_err(to_py_err)
    }

    fn ws_close<'py>(&self, py: Python<'py>, handle: u64) -> PyResult<Bound<'py, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            hai_binding_core::ws_close(handle)
                .await
                .map_err(to_py_err)?;
            Ok(())
        })
    }

    fn ws_close_sync(&self, py: Python, handle: u64) -> PyResult<()> {
        check_not_async()?;
        py.detach(|| RT.block_on(async { hai_binding_core::ws_close(handle).await }))
            .map_err(to_py_err)
    }

    // =========================================================================
    // JACS Document Store (40 entries: 20 async + 20 _sync)
    //
    // Each method is a thin delegation to `HaiClientWrapper`. Vec<u8> returns
    // map to Python `bytes` automatically via pyo3 0.28's specialised
    // `IntoPyObject for Vec<u8>` impl. Option<String> returns map to
    // Python `Optional[str]` natively.
    // =========================================================================

    // ---- store_document ----
    fn store_document<'py>(
        &self,
        py: Python<'py>,
        signed_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.store_document(signed_json).await.map_err(to_py_err)
        })
    }

    fn store_document_sync(&self, py: Python, signed_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.store_document(signed_json).await }))
            .map_err(to_py_err)
    }

    // ---- sign_and_store ----
    fn sign_and_store<'py>(
        &self,
        py: Python<'py>,
        data_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.sign_and_store(data_json).await.map_err(to_py_err)
        })
    }

    fn sign_and_store_sync(&self, py: Python, data_json: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.sign_and_store(data_json).await }))
            .map_err(to_py_err)
    }

    // ---- get_document ----
    fn get_document<'py>(&self, py: Python<'py>, key: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.get_document(key).await.map_err(to_py_err)
        })
    }

    fn get_document_sync(&self, py: Python, key: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.get_document(key).await }))
            .map_err(to_py_err)
    }

    // ---- get_latest_document ----
    fn get_latest_document<'py>(
        &self,
        py: Python<'py>,
        doc_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.get_latest_document(doc_id).await.map_err(to_py_err)
        })
    }

    fn get_latest_document_sync(&self, py: Python, doc_id: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.get_latest_document(doc_id).await }))
            .map_err(to_py_err)
    }

    // ---- get_document_versions ----
    fn get_document_versions<'py>(
        &self,
        py: Python<'py>,
        doc_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_document_versions(doc_id)
                .await
                .map_err(to_py_err)
        })
    }

    fn get_document_versions_sync(&self, py: Python, doc_id: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.get_document_versions(doc_id).await }))
            .map_err(to_py_err)
    }

    // ---- list_documents ----
    #[pyo3(signature = (jacs_type=None))]
    fn list_documents<'py>(
        &self,
        py: Python<'py>,
        jacs_type: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.list_documents(jacs_type).await.map_err(to_py_err)
        })
    }

    #[pyo3(signature = (jacs_type=None))]
    fn list_documents_sync(&self, py: Python, jacs_type: Option<String>) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.list_documents(jacs_type).await }))
            .map_err(to_py_err)
    }

    // ---- remove_document ----
    fn remove_document<'py>(&self, py: Python<'py>, key: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.remove_document(key).await.map_err(to_py_err)?;
            Ok(())
        })
    }

    fn remove_document_sync(&self, py: Python, key: String) -> PyResult<()> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.remove_document(key).await }))
            .map_err(to_py_err)
    }

    // ---- update_document ----
    fn update_document<'py>(
        &self,
        py: Python<'py>,
        doc_id: String,
        signed_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .update_document(doc_id, signed_json)
                .await
                .map_err(to_py_err)
        })
    }

    fn update_document_sync(
        &self,
        py: Python,
        doc_id: String,
        signed_json: String,
    ) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.update_document(doc_id, signed_json).await }))
            .map_err(to_py_err)
    }

    // ---- search_documents ----
    fn search_documents<'py>(
        &self,
        py: Python<'py>,
        query: String,
        limit: usize,
        offset: usize,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .search_documents(query, limit, offset)
                .await
                .map_err(to_py_err)
        })
    }

    fn search_documents_sync(
        &self,
        py: Python,
        query: String,
        limit: usize,
        offset: usize,
    ) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.search_documents(query, limit, offset).await }))
            .map_err(to_py_err)
    }

    // ---- query_by_type ----
    fn query_by_type<'py>(
        &self,
        py: Python<'py>,
        doc_type: String,
        limit: usize,
        offset: usize,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .query_by_type(doc_type, limit, offset)
                .await
                .map_err(to_py_err)
        })
    }

    fn query_by_type_sync(
        &self,
        py: Python,
        doc_type: String,
        limit: usize,
        offset: usize,
    ) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.query_by_type(doc_type, limit, offset).await }))
            .map_err(to_py_err)
    }

    // ---- query_by_field ----
    fn query_by_field<'py>(
        &self,
        py: Python<'py>,
        field: String,
        value: String,
        limit: usize,
        offset: usize,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .query_by_field(field, value, limit, offset)
                .await
                .map_err(to_py_err)
        })
    }

    fn query_by_field_sync(
        &self,
        py: Python,
        field: String,
        value: String,
        limit: usize,
        offset: usize,
    ) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| {
            RT.block_on(async { client.query_by_field(field, value, limit, offset).await })
        })
        .map_err(to_py_err)
    }

    // ---- query_by_agent ----
    fn query_by_agent<'py>(
        &self,
        py: Python<'py>,
        agent_id: String,
        limit: usize,
        offset: usize,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .query_by_agent(agent_id, limit, offset)
                .await
                .map_err(to_py_err)
        })
    }

    fn query_by_agent_sync(
        &self,
        py: Python,
        agent_id: String,
        limit: usize,
        offset: usize,
    ) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.query_by_agent(agent_id, limit, offset).await }))
            .map_err(to_py_err)
    }

    // ---- storage_capabilities ----
    fn storage_capabilities<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.storage_capabilities().await.map_err(to_py_err)
        })
    }

    fn storage_capabilities_sync(&self, py: Python) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.storage_capabilities().await }))
            .map_err(to_py_err)
    }

    // ---- save_memory ----
    #[pyo3(signature = (content=None))]
    fn save_memory<'py>(
        &self,
        py: Python<'py>,
        content: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.save_memory(content).await.map_err(to_py_err)
        })
    }

    #[pyo3(signature = (content=None))]
    fn save_memory_sync(&self, py: Python, content: Option<String>) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.save_memory(content).await }))
            .map_err(to_py_err)
    }

    // ---- save_soul ----
    #[pyo3(signature = (content=None))]
    fn save_soul<'py>(
        &self,
        py: Python<'py>,
        content: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.save_soul(content).await.map_err(to_py_err)
        })
    }

    #[pyo3(signature = (content=None))]
    fn save_soul_sync(&self, py: Python, content: Option<String>) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.save_soul(content).await }))
            .map_err(to_py_err)
    }

    // ---- get_memory ----
    fn get_memory<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.get_memory().await.map_err(to_py_err)
        })
    }

    fn get_memory_sync(&self, py: Python) -> PyResult<Option<String>> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.get_memory().await }))
            .map_err(to_py_err)
    }

    // ---- get_soul ----
    fn get_soul<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.get_soul().await.map_err(to_py_err)
        })
    }

    fn get_soul_sync(&self, py: Python) -> PyResult<Option<String>> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.get_soul().await }))
            .map_err(to_py_err)
    }

    // ---- store_text_file ----
    fn store_text_file<'py>(&self, py: Python<'py>, path: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.store_text_file(path).await.map_err(to_py_err)
        })
    }

    fn store_text_file_sync(&self, py: Python, path: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.store_text_file(path).await }))
            .map_err(to_py_err)
    }

    // ---- store_image_file ----
    fn store_image_file<'py>(&self, py: Python<'py>, path: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.store_image_file(path).await.map_err(to_py_err)
        })
    }

    fn store_image_file_sync(&self, py: Python, path: String) -> PyResult<String> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.store_image_file(path).await }))
            .map_err(to_py_err)
    }

    // ---- get_record_bytes ----
    // Vec<u8> -> Python `bytes` via pyo3 0.28's specialised
    // `IntoPyObject for Vec<u8>` impl (`pyo3-0.28.3/src/conversions/std/vec.rs:25`).
    fn get_record_bytes<'py>(&self, py: Python<'py>, key: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.get_record_bytes(key).await.map_err(to_py_err)
        })
    }

    fn get_record_bytes_sync(&self, py: Python, key: String) -> PyResult<Vec<u8>> {
        check_not_async()?;
        let client = self.inner.clone();
        py.detach(|| RT.block_on(async { client.get_record_bytes(key).await }))
            .map_err(to_py_err)
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

// Note: Full integration tests for haiipy (PyO3 method dispatch, GIL release,
// async bridging) require a Python environment and are run via:
//   pip install -e ".[dev]" && pytest
// or CI smoke tests (see .github/workflows/test.yml).
//
// Rust-side unit tests below are compiled via `cargo test` only when the
// "test-haiipy" feature is NOT active (cdylib linking requires Python symbols).
// These tests cover error conversion logic and the deadlock guard independently
// of the PyO3 cdylib by testing the functions as pure Rust.

// The to_py_err and check_not_async functions are tested indirectly:
// - to_py_err: format matches HaiBindingError Display output, which is tested
//   in hai-binding-core's test suite
// - check_not_async: uses tokio::runtime::Handle::try_current() which is
//   deterministic -- covered by Python-level pytest in CI
