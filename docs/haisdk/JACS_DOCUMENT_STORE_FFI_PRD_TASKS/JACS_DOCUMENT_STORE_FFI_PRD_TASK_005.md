# Task 005: Public clients (Python `HaiClient`/`AsyncHaiClient`, Node `HaiClient`, Go `HaiClient`) surface 20 methods

## Context

PRD: `docs/haisdk/JACS_DOCUMENT_STORE_FFI_PRD.md` §4.5.

After TASKS 002, 003, 004, the FFI adapters in each language can call the 20 doc-store methods. The end-user `HaiClient` classes do NOT yet expose them:

- `python/src/haiai/client.py` (sync `HaiClient`) — no `save_memory` / `store_document` / etc. defined on the class.
- `python/src/haiai/async_client.py` (`AsyncHaiClient`) — same.
- `node/src/client.ts` — no `saveMemory` / `storeDocument` / etc. on the public `HaiClient`.
- `go/client.go` — no `SaveMemory` / etc. on `*Client`.

This task wires the FFI adapter methods through to the public `HaiClient` API so end users can call `from haiai import HaiClient; HaiClient(...).save_memory("hello")`.

## Goal

The 20 doc-store methods are reachable from the public client class in each language SDK with idiomatic naming and types.

## Research First

- [ ] Read `python/src/haiai/client.py` — find a representative existing FFI delegation (e.g. `register` at line 508, `verify_document` at line 931). Confirm the `self._get_ffi().X(...)` pattern.
- [ ] Read `python/src/haiai/async_client.py` — find an async equivalent.
- [ ] Read `python/src/haiai/_ffi_adapter.py` lines 548–691 — confirm sync FFI adapter has all 20 methods. **Note:** the existing return-type hints for `list_documents`/`get_document_versions`/`query_by_*` are declared as `dict[str, Any]` but the underlying RemoteJacsProvider returns `Vec<String>` (`rust/haiai/src/jacs.rs:401,404,424,432,441`). Step 6 fixes this.
- [ ] Read `python/src/haiai/_ffi_adapter.py` lines 1247–1386 — confirm async FFI adapter has all 20 methods. Same array-vs-object mismatch on the async side.
- [ ] Read `node/src/ffi-client.ts:144,1036+` — same issue on Node side. The interface declares `listDocuments(): Promise<string>` but the implementation parses to `Record<string, unknown>` — both wrong. Should be `Promise<string[]>`.
- [ ] Read `node/src/client.ts` — find a representative existing FFI delegation (e.g. `register` at line 446, `hello` at line 377). Confirm the `this.ffi.X(...)` pattern.
- [ ] Read `go/client.go` — find a representative existing FFI delegation. Confirm pattern.
- [ ] Read `python/tests/conftest.py:452-475` (MockFFIAdapter list/query methods) and `python/tests/test_jacs_document_store_ffi.py:158` (test stages `{"items": []}`) — both encode the wrong shape. Need to flip to `list[str]` returns.
- [ ] Read `node/tests/jacs-document-store-ffi.test.ts` mock stages for `listDocuments`/`queryBy*` — same.
- [ ] Read `go/mock_ffi_test.go` `ListDocuments`/`QueryBy*` returns — same.

## TDD: Tests First (Red)

The cross-language facade tests (`python/tests/test_jacs_document_store_ffi.py`, `go/jacs_document_store_ffi_test.go`, `node/tests/jacs-document-store-ffi.test.ts`) currently exercise the FFI adapter directly. We need analogous tests at the public-client layer.

### Unit Tests
- [ ] Test: `python/tests/test_client_doc_store.py` (new file) — for each of the 20 methods, monkey-patch `HaiClient._ffi` to a `MockFFIAdapter`, call the public method, assert delegation. ~20 short tests.
- [ ] Test: `python/tests/test_async_client_doc_store.py` (new file, async) — same for `AsyncHaiClient` with `MockAsyncFFIAdapter`.
- [ ] Test: `node/tests/client-doc-store.test.ts` (new file) — for each of the 20 methods, mock `FFIClientAdapter`, call the public method, assert delegation.
- [ ] Test: `go/client_doc_store_test.go` (new file) — for each of the 20 methods, inject `mockFFIClient` via `WithFFIClient`, call the public method, assert delegation.
- [ ] Test (red): `test_list_documents_returns_list_of_strings` — staging `["k1","k2"]` returns a Python `list[str]`, NOT `dict`. Same for `get_document_versions`, `query_by_type`, `query_by_field`, `query_by_agent`. This test will fail today against the existing `_ffi_adapter.py:592` because the adapter declares `dict[str, Any]` and tests stage `{"items": []}`. Updating the adapter return type to `list[str]` and fixing the staged value to `["k1","k2"]` makes it pass.
- [ ] Test (red): `test_listDocuments_returns_string_array` (Node) — same shape assertion.
- [ ] Test (red): `TestListDocumentsReturnsStringSlice` (Go) — same.

### Integration Tests
None at this layer — covered by TASK_006 smoke tests.

## Implementation

- [ ] Step 1: Add 20 methods to `python/src/haiai/client.py` `HaiClient` class. Pattern:
  ```python
  def save_memory(self, content: Optional[str] = None) -> str:
      """Sign and store a MEMORY.md record. If `content` is None, reads MEMORY.md from CWD. Returns the record key."""
      return self._get_ffi().save_memory(content)

  def get_record_bytes(self, key: str) -> bytes:
      """Fetch raw record bytes (any content type, no UTF-8 decode, no JSON parse)."""
      return self._get_ffi().get_record_bytes(key)
  ```
- [ ] Step 2: Add 20 async methods to `python/src/haiai/async_client.py` `AsyncHaiClient` class with `await self._get_ffi().X(...)`.
- [ ] Step 3: Add 20 methods to `node/src/client.ts` `HaiClient` class. Pattern:
  ```typescript
  async saveMemory(content?: string | null): Promise<string> {
      return await this.ffi.saveMemory(content ?? null);
  }
  ```
  For methods that return parsed JSON (e.g. `searchDocuments`, `signAndStore`, `getDocumentVersions`, etc.), the FFI adapter already JSON-decodes — return `Record<string, unknown>` or a typed interface. Match the existing FFI adapter return shapes.
- [ ] Step 4: Add 20 methods to `go/client.go` `*Client`. **Confirmed convention:** all existing public methods on `*Client` take `ctx context.Context` as the first arg (verified — see `Hello`, `Register`, `RotateKeys`, etc. in `go/client.go`). Match it. Pattern:
  ```go
  func (c *Client) SaveMemory(ctx context.Context, content string) (string, error) {
      // Note: ctx isn't propagated through FFI yet; reserved for future cancellation.
      _ = ctx
      return c.ffi.SaveMemory(content)
  }
  ```
- [ ] Step 5: For `get_memory` / `get_soul`, `RemoteJacsProvider` returns `Result<Option<String>>` where the inner `String` is the latest record's signed-envelope JSON (a JSON document serialized to a string — not a parsed object). Surface as `Optional[str]` (Python), `string | null` (Node), `string` with empty-string-as-None (Go). Do NOT `json.loads(envelope)` / `JSON.parse(envelope)` on the SDK side — pass the raw envelope string through; callers can parse it if they want the inner fields.
- [ ] Step 6: For methods returning JSON arrays/objects, distinguish two cases. The existing FFI adapter declarations are WRONG for the array-returning trait methods — fix the type hints in this task before wiring the public client.
   - **Methods returning JSON arrays of strings** (`list_documents`, `get_document_versions`, `query_by_type`, `query_by_field`, `query_by_agent`): the trait returns `Vec<String>` (`rust/haiai/src/jacs.rs:401,404,424,432,441`), binding-core JSON-serializes to `["k1","k2"]`. Surface as:
     - Python: `list[str]` — fix `python/src/haiai/_ffi_adapter.py:585,592,602,608,614` (sync) and matching async at lines 1281+. Currently wrong: declared `dict[str, Any]`. The pre-existing `MockFFIAdapter` in `python/tests/conftest.py:452-475` returns `dict` and the test at `python/tests/test_jacs_document_store_ffi.py:158` stages `{"items": []}` — both must be updated to return `list[str]`. Update test fixture stages to use `["k1", "k2"]` style.
     - Node: `string[]` — fix `node/src/ffi-client.ts:144,1036` etc. Currently wrong: declared `Promise<Record<string, unknown>>`. Update `node/tests/jacs-document-store-ffi.test.ts` mock stages.
     - Go: `[]string` — TASK_004 owns the Go-side flip (it's part of getting `cgoFFIClient` to satisfy the iface): (1) `go/ffi_iface.go:123,124,127-130` flipped from `(json.RawMessage, error)` to `([]string, error)` for `ListDocuments`, `GetDocumentVersions`, `QueryByType`, `QueryByField`, `QueryByAgent`; (2) `cgoFFIClient` impl parses the JSON string into `[]string`; (3) `go/mock_ffi_test.go:713,720,748,755,762` returns `[]string` and stages updated to `[]string{"k1","k2"}`. This task (TASK_005) only adds the public `*Client` methods that delegate; the interface flip is in TASK_004.
   - **Methods returning JSON objects** (`search_documents` → `DocSearchResults`, `storage_capabilities` → `StorageCapabilities`, `sign_and_store`/`update_document` → `SignedDocument`): existing adapter declarations of `dict[str, Any]` / `Record<string, unknown>` are correct. Surface those on the public client as-is.
   - Go-only: existing FFI methods return `json.RawMessage`. Surface that as-is on the public `*Client` so callers can `json.Unmarshal` into their preferred type. EXCEPT for the array methods listed above, which return `[]string`.
- [ ] Step 7: Update SDK docstrings/JSDoc/Go-doc with one line each from the trait docs.
- [ ] DRY check: per-method body is one line. Don't add an indirection layer. The Python adapter, Node adapter, and Go FFI client are the abstraction.

## TDD: Tests Pass (Green)
- [ ] `pytest python/tests/test_client_doc_store.py python/tests/test_async_client_doc_store.py` passes.
- [ ] `npm test -- client-doc-store` passes in `node/`.
- [ ] `go test ./go -run TestClientDocStore` passes.
- [ ] All existing tests (`pytest`, `npm test`, `go test ./go/...`) still pass.

## Acceptance Criteria
- [ ] `from haiai import HaiClient; client = HaiClient(...); client.save_memory("test")` returns a string when the FFI mock returns one.
- [ ] `from haiai import AsyncHaiClient; client = AsyncHaiClient(...); await client.save_memory("test")` works the same way.
- [ ] `import { HaiClient } from 'haiai'; const c = await HaiClient.create(...); await c.saveMemory("test")` returns a string.
- [ ] `c.saveMemory(null)` is allowed and passes `null` through.
- [ ] `client.GetRecordBytes(ctx, "k")` returns `[]byte`.
- [ ] All 20 names (Python `snake_case`, Node `camelCase`, Go `PascalCase`) are reachable on each public client.

## Execution
- **Agent Type**: `general-purpose`
- **Wave**: 3 (depends on 002, 003, 004)
- **Complexity**: Medium
