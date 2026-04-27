# Task 001: binding-core wires the 20 JACS document-store methods

## Context

PRD: `docs/haisdk/JACS_DOCUMENT_STORE_FFI_PRD.md` §4.2–§4.3 and §8.

The fixture `fixtures/ffi_method_parity.json` declares a `jacs_document_store` group of 20 method names. `RemoteJacsProvider` in `rust/haiai/src/jacs_remote.rs` already implements all 20 (13 trait CRUD/query + 4 D5 + 3 D9). The Rust CLI (`rust/haiai-cli/src/main.rs::build_remote_provider`) and MCP server (`rust/hai-mcp/src/hai_tools.rs::build_remote_provider`) construct a fresh `RemoteJacsProvider<LocalJacsProvider>` per call from the local agent's signing material plus `HAI_URL`.

`hai-binding-core::HaiClientWrapper` (`rust/hai-binding-core/src/lib.rs`) currently exposes 67 methods to the language bindings but **zero** of the doc-store methods. The Python adapter, Node FFI client, and Go FFI interface already declare and call the 20 methods on `self._native.*` / `this.native.*` / `c.ffi.*` — those calls fail at runtime (Python/Node) or refuse to compile (Go's `cgoFFIClient` doesn't satisfy `FFIClient`).

This task adds the 20 methods to `HaiClientWrapper`. The next three tasks expose them via PyO3, napi-rs, and cgo respectively.

## Goal

`HaiClientWrapper` exposes 20 async methods that match the fixture's `jacs_document_store` group, each returning `HaiBindingResult<String>` (or `HaiBindingResult<Option<String>>` for `get_memory` / `get_soul`, or `HaiBindingResult<Vec<u8>>` for `get_record_bytes`, or `HaiBindingResult<()>` for `remove_document`).

## Research First

- [ ] Read `rust/haiai/src/jacs_remote.rs` lines 274–685 (the `JacsDocumentProvider` impl block) to confirm exact return types per method.
- [ ] Read `rust/haiai-cli/src/main.rs` lines 2743–2758 (`build_remote_provider`) — copy the construction pattern.
- [ ] Read `rust/hai-mcp/src/hai_tools.rs` lines 1224–1247 (`build_remote_provider`) — same pattern, confirm symmetry.
- [ ] Read `rust/hai-binding-core/src/lib.rs` lines 342–377 (`from_config_json_auto`) — confirm `jacs_config_path` parsing already exists; we will store it for re-use.
- [ ] Verify `LocalJacsProvider` is NOT `Clone` (it owns `Mutex<jacs::agent::Agent>`) — confirms the per-call rebuild design.
- [ ] Verify `RemoteJacsProvider::sign_and_store`, `update_document`, `search_documents`, `query_by_*`, `storage_capabilities` return types in `rust/haiai/src/types.rs` (they return typed structs that need to be JSON-serialized for the FFI return).
- [ ] Confirm `httpmock` is NOT in `rust/hai-binding-core/Cargo.toml` `[dev-dependencies]` today (it isn't — only `tokio`, `regex`, `tempfile`, `image`). Plan to add it. Use `httpmock = "0.8"` to match the version already in `rust/haiai/Cargo.toml:49`; do NOT introduce 0.7.
- [ ] Confirm `RemoteJacsProvider<StaticJacsProvider>` is constructable in tests via the same pattern as `rust/haiai/src/jacs_remote.rs:806 make_provider` (StaticJacsProvider lives in `rust/haiai/src/jacs.rs:886`, re-exported via `haiai::jacs::StaticJacsProvider`).
- [ ] Read `rust/hai-mcp/src/server.rs` lines 75–82 and `rust/hai-mcp/src/hai_tools.rs::call_save_memory` (line 1249) — the existing async MCP handler calls `JacsDocumentProvider::save_memory(&provider, ...)` synchronously inline without `spawn_blocking`. `RemoteJacsProvider::block_on` (`rust/haiai/src/jacs_remote.rs:207`) uses `tokio::task::block_in_place` which works on a multi-thread runtime worker. Verified against tokio source (`tokio-1.52.1/src/runtime/scheduler/multi_thread/worker.rs:351-430`): `block_in_place` also gracefully handles "outside the runtime" / "spawn_blocking thread" callers via the `NotEntered` branch. So `spawn_blocking` would not panic — but the inline call path is simpler, matches MCP/CLI, and avoids the extra thread-pool hop. Use it.
- [ ] Verify return-type semantics for the `Vec<String>`-returning trait methods (`list_documents`, `get_document_versions`, `query_by_type`, `query_by_field`, `query_by_agent`) in `rust/haiai/src/jacs.rs:401,404,424,432,441`. They return JSON arrays (`["k1","k2"]`), NOT objects. Adapter / public-client return types must be `list[str]` (Python), `string[]` (Node), `[]string` (Go) — NOT `dict[str, Any]` / `Record<string, unknown>`. Existing `_ffi_adapter.py:592` and `node/src/ffi-client.ts:1036` declare these as dicts; that's a pre-existing bug the smoke test would surface. TASK_005 step 6 fixes those declarations.

## TDD: Tests First (Red)

Tests live in the existing `#[cfg(test)] mod tests` block in `rust/hai-binding-core/src/lib.rs`. Add `httpmock = "0.8"` to `[dev-dependencies]` of `rust/hai-binding-core/Cargo.toml` (it is currently NOT a dev-dep — the existing tests in this crate do not exercise HTTP). The version `0.8` matches the existing dev-dep in `rust/haiai/Cargo.toml:49` so the workspace resolves a single `httpmock` crate. Mirror the pattern of `rust/haiai/src/jacs_remote.rs::tests` — those tests construct `RemoteJacsProvider<StaticJacsProvider>` against an httpmock server and require no on-disk JACS config.

**Test approach (committed, single shape):** for each of the 20 methods, the public async method is a 2-line shim that calls `self.build_doc_store()?` then a `pub(crate)` associated function `Self::<method>_with`. The `_with` helper takes `(store: &RemoteJacsProvider<P>, args...)` (no `&self`) and is generic over `P: JacsProvider` so it works with both `LocalJacsProvider` (production) and `StaticJacsProvider` (tests). Sync trait calls are inline (no `spawn_blocking`). Tests construct a `RemoteJacsProvider<StaticJacsProvider>` directly (mirroring `make_provider` at `rust/haiai/src/jacs_remote.rs:806`) and call the `_with` associated function. The public entry-point retains the missing-config negative test. Do NOT require `LocalJacsProvider::from_config_path` to succeed in the binding-core test environment. See Step 3 for the canonical signature shape.

**Tokio test attribute (required):** every new doc-store unit test in `rust/hai-binding-core/src/lib.rs` MUST use `#[tokio::test(flavor = "multi_thread")]`. The default `#[tokio::test]` builds a `current_thread` runtime; calling `JacsDocumentProvider::save_memory(&store, ...)` inside the wrapper invokes `RemoteJacsProvider::block_on` which calls `tokio::task::block_in_place`, and `block_in_place` panics on `current_thread`. Every test in `rust/haiai/src/jacs_remote.rs::tests` already uses this attribute; mirror it.

**Acceptance for "missing-config" path:** add ONE additional test that constructs a `HaiClientWrapper` via `from_config_json_auto` with no `jacs_config_path` and asserts `wrapper.save_memory(...)` returns `Err(HaiBindingError { kind: ProviderError, message: "jacs_config_path required for document-store operations" })`. This validates the public entry point without needing a real JACS config.

### Unit Tests
- [ ] Test: `binding_core_save_memory_calls_records_endpoint_with_jacstype_memory` in `rust/hai-binding-core/src/lib.rs#tests` — POSTs to the mock and asserts the request body has `"jacsType":"memory"`, returns the `key` from the mock response. Mirrors `jacs_remote.rs::tests::save_memory_posts_with_jacstype_memory`.
- [ ] Test: `binding_core_get_memory_returns_none_when_no_record` — mock returns `{"records":[]}`, wrapper returns `Ok(None)`.
- [ ] Test: `binding_core_store_document_returns_key_from_response` — POST `application/json` body, mock returns `{"key":"id:v1"}`, wrapper returns `Ok("id:v1".to_string())`.
- [ ] Test: `binding_core_search_documents_serializes_to_json_string` — wrapper returns `Ok(String)` containing a JSON-serialized `DocSearchResults`.
- [ ] Test: `binding_core_query_by_agent_returns_empty_list_for_other_agent` — mirrors `jacs_remote.rs::tests::query_by_agent_other_returns_provider_error`. Adapted to whatever wrapper-level error mapping looks like.
- [ ] Test: `binding_core_get_record_bytes_returns_raw_bytes_not_base64` — assert `Ok(Vec<u8>)` round-trip preserves arbitrary non-UTF8 bytes (e.g. PNG magic). Mirrors `jacs_remote.rs::tests::get_record_bytes_returns_raw_bytes`.
- [ ] Test: `binding_core_storage_capabilities_returns_remote_caps_json` — wrapper returns `Ok(String)` containing JSON with `fulltext:true,vector:false,...`.
- [ ] Test: `binding_core_remove_document_returns_unit_on_success` — wrapper returns `Ok(())`.
- [ ] Test: `binding_core_doc_store_method_propagates_provider_error` — server returns 500, `HaiError::Provider` → `HaiBindingError { kind: ProviderError, ... }`.

(Nine tests cover all 20 methods by category; we don't need 20 separate tests.)

### Integration Tests
None at this layer — covered by TASK_005 smoke tests and existing `rust/haiai/tests/jacs_remote_integration.rs`.

## Implementation

- [ ] Step 1: Extend `HaiClientWrapper` to remember the `jacs_config_path` and `base_url` from `from_config_json` / `from_config_json_auto`. Today `from_config_json_auto` parses `jacs_config_path` from the config JSON, hands a `Box<dyn JacsMediaProvider>` to `HaiClient`, and drops the path. `base_url` is forwarded to `HaiClient::new` through `HaiClientOptions` and is also not retained. Add two new fields to `HaiClientWrapper` next to `client_identifier`:
  ```rust
  pub struct HaiClientWrapper {
      inner: Arc<RwLock<HaiClient<Box<dyn JacsMediaProvider>>>>,
      client_identifier: String,
      jacs_config_path: Option<PathBuf>, // None when StaticJacsProvider fallback was used
      base_url: String,
  }
  ```
  Populate from `from_config_json_auto` (read both fields once, store them, then call `from_config_json` with the same JSON). For `from_config_json` (the explicit-provider variant), `jacs_config_path` defaults to `None` and `base_url` is read from the same `config["base_url"]` field. Add `import std::path::PathBuf;` if not already present.
- [ ] Step 2: Add a private helper `fn build_doc_store(&self) -> HaiBindingResult<RemoteJacsProvider<LocalJacsProvider>>` on `HaiClientWrapper`. It re-runs the same `LocalJacsProvider::from_config_path` + `RemoteJacsProvider::new` chain that the CLI / MCP `build_remote_provider` helpers use. Returns `ProviderError` if `jacs_config_path` is `None` (StaticJacsProvider fallback): `"jacs_config_path required for document-store operations"`.
- [ ] Step 3: Add 20 async methods on `HaiClientWrapper`, each split into a public 2-line shim + a `pub(crate)` `*_with` test seam. **Do NOT use `tokio::task::spawn_blocking`** — the sync trait methods use `RemoteJacsProvider::block_on` (`rust/haiai/src/jacs_remote.rs:207`) which calls `tokio::task::block_in_place`. The MCP server (`rust/hai-mcp/src/hai_tools.rs::call_save_memory`, line 1249) and CLI invoke the trait method inline inside an async function. Match that pattern: simpler, matches existing parity contract, no extra thread-pool hop. The split lets unit tests construct `RemoteJacsProvider<StaticJacsProvider>` and call `*_with` directly without needing a real on-disk JACS config:

  ```rust
  // Public entry point — used by haiipy / haiinpm / haiigo callers.
  pub async fn save_memory(&self, content: Option<String>) -> HaiBindingResult<String> {
      let store = self.build_doc_store()?;
      Self::save_memory_with(&store, content)
  }

  // Test seam — pub(crate) so unit tests in the same crate can call it
  // with a `RemoteJacsProvider<StaticJacsProvider>` directly. Generic over
  // the inner JacsProvider so both LocalJacsProvider (production) and
  // StaticJacsProvider (tests) work without changes here.
  pub(crate) fn save_memory_with<P: JacsProvider>(
      store: &RemoteJacsProvider<P>,
      content: Option<String>,
  ) -> HaiBindingResult<String> {
      // Sync trait call inline. RemoteJacsProvider::block_on uses
      // block_in_place which works on the multi-thread runtime (haiipy /
      // haiinpm both build with new_multi_thread). The MCP server uses the
      // same inline pattern.
      JacsDocumentProvider::save_memory(store, content.as_deref())
          .map_err(HaiBindingError::from)
  }
  ```

  - Methods returning `Vec<String>` (`list_documents`, `get_document_versions`, `query_by_type`, `query_by_field`, `query_by_agent`) → JSON-serialize before returning so the FFI signature is consistent: `HaiBindingResult<String>` (the JSON string `["k1","k2"]`). The downstream adapters JSON-decode to `list[str]` / `string[]` / `[]string`. NOT objects (TASK_005 step 6 fixes the existing wrong type hints).
  - `sign_and_store` → returns `SignedDocument`, JSON-serialize, type `HaiBindingResult<String>`.
  - `update_document` → returns `SignedDocument`, JSON-serialize.
  - `search_documents` → returns `DocSearchResults`, JSON-serialize.
  - `storage_capabilities` → returns `StorageCapabilities`, JSON-serialize.
  - `get_memory` / `get_soul` → return `HaiBindingResult<Option<String>>`.
  - `get_record_bytes` → returns `HaiBindingResult<Vec<u8>>` (no JSON).
  - `remove_document` → returns `HaiBindingResult<()>`.
  - All other key-returning methods → return `HaiBindingResult<String>` directly.
- [ ] Step 4: Take `Option<String>` for nullable args (`save_memory(content)`, `save_soul(content)`, `list_documents(jacs_type)`); `usize` for `limit`/`offset` (typed numeric args, NOT a JSON params blob — diverges from the existing `list_attestations(params_json)` pattern but matches the `jacs_document_store` fixture's discrete arg list and gives PyO3 / napi-rs idiomatic types); `&str` (or owned `String`) for required strings. The four typed-numeric methods are `search_documents(query: &str, limit: usize, offset: usize)`, `query_by_type(doc_type: &str, limit: usize, offset: usize)`, `query_by_field(field: &str, value: &str, limit: usize, offset: usize)`, `query_by_agent(agent_id: &str, limit: usize, offset: usize)`. TASK_004 owns the cgo-side `size_t` ABI for these.
- [ ] Step 5: Update `rust/hai-binding-core/methods.json`. The file's `methods` field is a flat JSON array of entries (verified — current count 55; each entry carries `name`, `category`, `group`, `params`, `returns`, `auth_required`, `notes`). Append 20 entries with `"group": "jacs_document_store"` and `"category": "async"`, one per fixture method, mirroring shape of neighbours like the `media_local` group entries. After the append, update `summary` accordingly:
  - `async_methods`: `55 → 75`
  - `total_public_methods`: `81 → 101`
  - `binding_core_scope`: from `"55 async + 6 streaming + 2 callback + 11 sync + 2 mutating = 76 methods"` to `"75 async + 6 streaming + 2 callback + 11 sync + 2 mutating = 96 methods"`
  Optional per the file's preamble docstring, but PRD §8 calls it out as a deliverable. (TASK_007 step 8 verifies the count returns 20; TASK_001 owns the write.)
- [ ] DRY check: the per-method public body is exactly two statements (`let store = self.build_doc_store()?; Self::<method>_with(&store, args)`) and the `_with` body is one mapped call. Don't introduce a third helper layer — `build_doc_store` + 20 `*_with` seams is the abstraction. For the eight JSON-serialized return types, do the `serde_json::to_string` in the `_with` body itself (one line), not in a `_json` macro variant. Do NOT add `spawn_blocking` — the MCP/CLI call sync trait methods inline and that's the parity contract; spawn_blocking adds an unnecessary thread-pool hop and diverges from the established pattern.

## TDD: Tests Pass (Green)
- [ ] All new tests pass.
- [ ] `cargo test -p hai-binding-core` and `cargo test -p haiai` continue to pass.
- [ ] `rust/haiai/tests/contract_test.rs::ffi_method_parity_total_count_is_92` continues to pass (no fixture changes).

## Acceptance Criteria
- [ ] `HaiClientWrapper::save_memory(Some("test"))` against an httpmock server returns `Ok("memory:v1")` when the mock responds with `{"key":"memory:v1"}`.
- [ ] `HaiClientWrapper::get_memory()` against an httpmock server with `{"records":[]}` returns `Ok(None)`.
- [ ] `HaiClientWrapper::get_record_bytes("img-1")` against an httpmock server returning raw PNG bytes returns `Ok(Vec<u8>)` whose first 8 bytes match `\x89PNG\r\n\x1a\n`.
- [ ] When `jacs_config_path` is absent (StaticJacsProvider fallback), `save_memory` returns `Err(HaiBindingError { kind: ProviderError, message: "jacs_config_path required for document-store operations" })`.
- [ ] `cargo doc -p hai-binding-core` produces docs for all 20 methods (no doc warnings).
- [ ] No `eprintln!` / `println!` added to production code paths; if a warning is needed for missing config, route through the existing `eprintln!` in `from_config_json_auto`.

## Execution
- **Agent Type**: `general-purpose`
- **Wave**: 1 (no dependencies)
- **Complexity**: High
