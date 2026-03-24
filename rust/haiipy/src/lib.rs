//! Python bindings for HAI SDK via PyO3.
//!
//! Every `HaiClientWrapper` method is exposed as both:
//! - An async method (returns Python coroutine for asyncio users)
//! - A sync method with `_sync` suffix (blocks, releases GIL)
//!
//! No business logic lives here -- each method is thin delegation
//! to `hai-binding-core`.

use std::sync::Arc;

use hai_binding_core::{HaiBindingError, HaiClientWrapper, RT};
use haiai::jacs::StaticJacsProvider;
use pyo3::prelude::*;

// =============================================================================
// Error Conversion
// =============================================================================

fn to_py_err(e: HaiBindingError) -> PyErr {
    PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("{}: {}", e.kind, e.message))
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
        let config: serde_json::Value = serde_json::from_str(&config_json)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(format!("invalid config JSON: {e}")))?;

        let jacs_id = config
            .get("jacs_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let provider = StaticJacsProvider::new(jacs_id);

        let wrapper = HaiClientWrapper::from_config_json(&config_json, Box::new(provider))
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
        let client = self.inner.clone();
        py.allow_threads(|| {
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
        let client = self.inner.clone();
        py.allow_threads(|| {
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
        let client = self.inner.clone();
        py.allow_threads(|| {
            RT.block_on(async { client.register(&options_json).await })
        }).map_err(to_py_err)
    }

    fn rotate_keys<'py>(&self, py: Python<'py>, options_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.rotate_keys(&options_json).await.map_err(to_py_err)
        })
    }

    fn update_agent<'py>(&self, py: Python<'py>, new_agent_data: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.update_agent(&new_agent_data).await.map_err(to_py_err)
        })
    }

    fn submit_response<'py>(&self, py: Python<'py>, params_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.submit_response(&params_json).await.map_err(to_py_err)
        })
    }

    #[pyo3(signature = (agent_id=None))]
    fn verify_status<'py>(&self, py: Python<'py>, agent_id: Option<String>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.verify_status(agent_id.as_deref()).await.map_err(to_py_err)
        })
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

    fn update_username<'py>(&self, py: Python<'py>, agent_id: String, username: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.update_username(&agent_id, &username).await.map_err(to_py_err)
        })
    }

    fn delete_username<'py>(&self, py: Python<'py>, agent_id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.delete_username(&agent_id).await.map_err(to_py_err)
        })
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

    fn send_signed_email<'py>(&self, py: Python<'py>, options_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.send_signed_email(&options_json).await.map_err(to_py_err)
        })
    }

    fn list_messages<'py>(&self, py: Python<'py>, options_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.list_messages(&options_json).await.map_err(to_py_err)
        })
    }

    fn update_labels<'py>(&self, py: Python<'py>, params_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.update_labels(&params_json).await.map_err(to_py_err)
        })
    }

    fn get_email_status<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.get_email_status().await.map_err(to_py_err)
        })
    }

    fn get_message<'py>(&self, py: Python<'py>, message_id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.get_message(&message_id).await.map_err(to_py_err)
        })
    }

    fn get_message_sync(&self, py: Python, message_id: String) -> PyResult<String> {
        let client = self.inner.clone();
        py.allow_threads(|| {
            RT.block_on(async { client.get_message(&message_id).await })
        }).map_err(to_py_err)
    }

    fn get_unread_count<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.get_unread_count().await.map_err(to_py_err)
        })
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
        let client = self.inner.clone();
        py.allow_threads(|| {
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

    fn delete_message<'py>(&self, py: Python<'py>, message_id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.delete_message(&message_id).await.map_err(to_py_err)?;
            Ok(())
        })
    }

    fn archive<'py>(&self, py: Python<'py>, message_id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.archive(&message_id).await.map_err(to_py_err)?;
            Ok(())
        })
    }

    fn unarchive<'py>(&self, py: Python<'py>, message_id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.unarchive(&message_id).await.map_err(to_py_err)?;
            Ok(())
        })
    }

    fn reply_with_options<'py>(&self, py: Python<'py>, params_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.reply_with_options(&params_json).await.map_err(to_py_err)
        })
    }

    fn forward<'py>(&self, py: Python<'py>, params_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.forward(&params_json).await.map_err(to_py_err)
        })
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

    fn contacts<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.contacts().await.map_err(to_py_err)
        })
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

    fn fetch_key_by_hash<'py>(&self, py: Python<'py>, hash: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.fetch_key_by_hash(&hash).await.map_err(to_py_err)
        })
    }

    fn fetch_key_by_email<'py>(&self, py: Python<'py>, email: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.fetch_key_by_email(&email).await.map_err(to_py_err)
        })
    }

    fn fetch_key_by_domain<'py>(&self, py: Python<'py>, domain: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.fetch_key_by_domain(&domain).await.map_err(to_py_err)
        })
    }

    fn fetch_all_keys<'py>(&self, py: Python<'py>, jacs_id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.fetch_all_keys(&jacs_id).await.map_err(to_py_err)
        })
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

    fn get_verification<'py>(&self, py: Python<'py>, agent_id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.get_verification(&agent_id).await.map_err(to_py_err)
        })
    }

    fn verify_agent_document<'py>(&self, py: Python<'py>, request_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.verify_agent_document(&request_json).await.map_err(to_py_err)
        })
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

    #[pyo3(signature = (transport=None))]
    fn free_run<'py>(&self, py: Python<'py>, transport: Option<String>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.free_run(transport.as_deref()).await.map_err(to_py_err)
        })
    }

    fn pro_run<'py>(&self, py: Python<'py>, options_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.pro_run(&options_json).await.map_err(to_py_err)
        })
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
        let client = self.inner.clone();
        py.allow_threads(|| {
            RT.block_on(async { client.build_auth_header().await })
        }).map_err(to_py_err)
    }

    fn sign_message<'py>(&self, py: Python<'py>, message: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.sign_message(&message).await.map_err(to_py_err)
        })
    }

    fn canonical_json<'py>(&self, py: Python<'py>, value_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.canonical_json(&value_json).await.map_err(to_py_err)
        })
    }

    fn verify_a2a_artifact<'py>(&self, py: Python<'py>, wrapped_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.verify_a2a_artifact(&wrapped_json).await.map_err(to_py_err)
        })
    }

    // =========================================================================
    // Client State
    // =========================================================================

    fn jacs_id_sync(&self, py: Python) -> PyResult<String> {
        let client = self.inner.clone();
        Ok(py.allow_threads(|| {
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

    fn set_agent_email<'py>(&self, py: Python<'py>, email: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.set_agent_email(email).await;
            Ok(())
        })
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
