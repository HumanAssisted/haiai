# Task 002: haiipy (PyO3) exposes 20 doc-store methods, async + `_sync`

## Context

PRD: `docs/haisdk/JACS_DOCUMENT_STORE_FFI_PRD.md` §4.4.

After TASK_001, `HaiClientWrapper` has 20 async methods on the doc-store surface. This task adds the 40 PyO3 entries (20 async + 20 `_sync` shims) on the `HaiClient` PyO3 class in `rust/haiipy/src/lib.rs`.

The Python FFI adapter `python/src/haiai/_ffi_adapter.py` (lines 548–691 sync, lines 1247–1386 async) **already** calls into `self._native.<method>_sync(...)` / `await self._native.<method>(...)` — so no Python-side changes are needed once haiipy ships the symbols.

## Goal

`from haiipy import HaiClient; client = HaiClient(config); client.save_memory_sync("...")` returns the record key from the mock server. The same call works from `asyncio` via `await client.save_memory("...")`.

## Research First

- [ ] Read `rust/haiipy/src/lib.rs` lines 1–200 to confirm the async + `_sync` pattern (e.g. `register` / `register_sync`).
- [ ] Read `rust/haiipy/src/lib.rs` lines 770–815 (or any existing `Option<String>` arg block — `verify_status` at lines 168–184). Confirm `#[pyo3(signature = (...))]` use for default `None`.
- [ ] Read `python/src/haiai/_ffi_adapter.py` lines 548–691 (sync surface) — every method call is already there. Confirm we don't need to touch this file.
- [ ] Read `python/src/haiai/_ffi_adapter.py` lines 1247–1386 (async surface) — same.
- [ ] **`Vec<u8>` IS auto-converted to Python `bytes` by pyo3 0.28** via the `IntoPyObject for Vec<T>` specialization at `pyo3-0.28.3/src/conversions/std/vec.rs:25` (verified — comment says "Turns `Vec<u8>` into `PyBytes`, all other `T`s will be turned into a `PyList`"). So a method signature `fn get_record_bytes_sync(&self, py: Python, key: String) -> PyResult<Vec<u8>>` returns Python `bytes` correctly. No explicit `PyBytes::new` wrap needed. The `python/tests/test_jacs_document_store_ffi.py:115` test asserts the `bytes` shape and is the source of truth.
- [ ] Confirm `Option<String>` returning method auto-translates to Python `Optional[str]` (pyo3 supports this via `IntoPyObject for Option<T>` since 0.20+).
- [ ] Re-read PRD §7 risk #3 (RemoteJacsProvider's blocking runtime under PyO3): TASK_001 was updated to call sync trait methods inline (matches MCP/CLI pattern, no `spawn_blocking`). The flow is haiipy `_sync` → `RT.block_on(async { wrapper.X().await })` → wrapper calls sync `JacsDocumentProvider::X(&store, ...)` → `RemoteJacsProvider::block_on` → `tokio::task::block_in_place(handle.block_on(...))` → safe on the multi-thread runtime worker. Confirm the haiipy RT uses `new_multi_thread` (`rust/haiipy/src/lib.rs:25`) — it does today, do NOT change it.

## TDD: Tests First (Red)

PyO3 has limited Rust-side test infra; the haiipy crate already documents that "Full integration tests for haiipy ... require a Python environment and are run via `pip install -e ".[dev]" && pytest`" (`rust/haiipy/src/lib.rs` line 1200). Prefer adding tests to `python/tests/test_ffi_adapter.py` rather than haiipy crate.

### Unit Tests (Python side, pytest)
- [ ] Test: `test_ffi_adapter_save_memory_calls_native_save_memory_sync` in `python/tests/test_ffi_adapter.py` — mock the `_native` object, assert `save_memory("hello")` calls `_native.save_memory_sync("hello")`.
- [ ] Test: `test_ffi_adapter_save_memory_passes_none_through` — assert `save_memory(None)` calls `_native.save_memory_sync(None)`.
- [ ] Test: `test_ffi_adapter_get_memory_returns_none_when_native_returns_none` — `_native.get_memory_sync` returns `None`, adapter returns `None`.
- [ ] Test: `test_ffi_adapter_get_record_bytes_returns_bytes_object` — `_native.get_record_bytes_sync` returns `b"raw"`, adapter returns `bytes` (not `str`).
- [ ] Test: `test_async_ffi_adapter_save_memory_awaits_native_coroutine` (async) — wires `AsyncMock` for `_native.save_memory`, asserts await round-trip.
- [ ] **Existing mock-only tests in `python/tests/test_jacs_document_store_ffi.py` must still pass** — they exercise `MockFFIAdapter` (in `python/tests/conftest.py`), not the real native binding. Do NOT modify them.

### Integration Tests (Python side)
- [ ] **DO NOT add live FFI integration tests in this task.** TASK_005 covers smoke testing the real haiipy native binding with one method. Without smoke tests, this task is "compiles + tests pass against MockFFIAdapter"; smoke is a separate gate.

## Implementation

- [ ] Step 1: In `rust/haiipy/src/lib.rs`, add a `// JACS Document Store` section after the existing `// Verification` block (around line 895). Use the same `// =================` divider style.
- [ ] Step 2: For each of the 20 methods, add the async + `_sync` pair following the existing pattern. Mass example:

  ```rust
  fn save_memory<'py>(
      &self, py: Python<'py>, content: Option<String>
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
  ```
- [ ] Step 3: For `get_record_bytes`, return `Vec<u8>` directly — pyo3 0.28's `IntoPyObject for Vec<T>` impl is specialized for `T=u8` to produce `PyBytes`. The signature is the simplest possible:
  ```rust
  fn get_record_bytes<'py>(&self, py: Python<'py>, key: String) -> PyResult<Bound<'py, PyAny>> {
      let client = self.inner.clone();
      pyo3_async_runtimes::tokio::future_into_py(py, async move {
          client.get_record_bytes(&key).await.map_err(to_py_err)
      })
  }

  fn get_record_bytes_sync(&self, py: Python, key: String) -> PyResult<Vec<u8>> {
      check_not_async()?;
      let client = self.inner.clone();
      py.detach(|| RT.block_on(async { client.get_record_bytes(&key).await }))
          .map_err(to_py_err)
  }
  ```
  Verified via `pyo3-0.28.3/src/conversions/std/vec.rs:25`: "Turns `Vec<u8>` into `PyBytes`, all other `T`s will be turned into a `PyList`". The acceptance test (`test_get_record_bytes_returns_bytes`, line 115 of `test_jacs_document_store_ffi.py`) confirms the `bytes` shape at runtime.
- [ ] Step 4: For `get_memory` / `get_soul`, return `Option<String>` — pyo3 maps to `Optional[str]`.
- [ ] Step 5: For `remove_document`, return `()` and use the existing `()` return convention (see `mark_read` pattern).
- [ ] Step 6: For `list_documents` add `#[pyo3(signature = (jacs_type=None))]` so callers can pass nothing.
- [ ] DRY check: the method body is mechanical. Don't write a macro yet (yagni); two-line body each is fine. Do confirm the imports — only `pyo3_async_runtimes::tokio::future_into_py` and `RT.block_on` are needed.

## TDD: Tests Pass (Green)
- [ ] `cargo build -p haiipy` succeeds.
- [ ] `pip install -e python/.[dev,mcp] && pytest python/tests/test_ffi_adapter.py` passes (mocked).
- [ ] `pytest python/tests/test_jacs_document_store_ffi.py` continues to pass (mocked).
- [ ] `pytest python/tests/test_mcp_parity.py` and contract tests continue to pass.

## Acceptance Criteria
- [ ] `python -c "from haiipy import HaiClient; print(dir(HaiClient))"` lists `save_memory`, `save_memory_sync`, `store_document`, `store_document_sync`, ... all 40 names.
- [ ] `_native.save_memory_sync(None)` no longer raises `AttributeError` once haiipy is built and installed (verified by smoke test in TASK_006; this task only requires the symbol exists).
- [ ] All 20 PyO3 entries have a matching `_sync` entry (40 total) in `rust/haiipy/src/lib.rs`.
- [ ] Each `get_record_bytes` / `get_record_bytes_sync` PyO3 entry returns Python `bytes` at runtime. Naked `Vec<u8>` works because pyo3 0.28 specializes the `IntoPyObject for Vec<u8>` impl to produce `PyBytes`. The acceptance test in `python/tests/test_jacs_document_store_ffi.py::test_get_record_bytes_returns_bytes` (line 115) asserts the `bytes` shape; runtime smoke verification is in TASK_006.

## Execution
- **Agent Type**: `general-purpose`
- **Wave**: 2 (depends on TASK_001 — needs `HaiClientWrapper::*` methods to exist)
- **Complexity**: Medium
