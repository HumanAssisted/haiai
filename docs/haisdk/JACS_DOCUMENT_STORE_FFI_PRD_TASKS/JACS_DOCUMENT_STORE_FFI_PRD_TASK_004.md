# Task 004: haiigo (cgo) exports 20 doc-store methods + Go-side wrappers satisfy `FFIClient`

## Context

PRD: `docs/haisdk/JACS_DOCUMENT_STORE_FFI_PRD.md` §4.4–§4.5.

After TASK_001, `HaiClientWrapper` has 20 doc-store methods. This task does TWO things:

1. Export 20 `extern "C"` symbols from `rust/haiigo/src/lib.rs` (the cdylib that becomes `libhaiigo.so/.dylib`).
2. Implement 20 methods on `*ffi.Client` in `go/ffi/ffi.go` (currently stubbed at lines 1184–1268 with `notWiredThroughLibhaiigo`) so the existing `cgoFFIClient` satisfies `FFIClient` (`go/ffi_iface.go:8-142`).

The Go interface `FFIClient` already declares all 20 methods (lines 117–141). The Go SDK's `client.go` is currently NOT calling these methods (no `SaveMemory`/etc. on `*Client`); that's TASK_006.

## Goal

`go test ./go/...` builds without "missing method" errors on `cgoFFIClient`. Calling `client.SaveMemory("...")` from a real Go program against a mock server returns the record key.

## Research First

- [ ] Read `rust/haiigo/src/lib.rs` lines 160–290 (the macros and FFI boilerplate). Confirm the existing patterns: `ffi_method_str!`, `ffi_method_str_two_args!`, `ffi_method_str_three_args!`, `ffi_method_noarg!`, `ffi_method_void!`. **None of these takes numeric (size_t/usize) args** — every existing extern that needs `limit`/`offset` (e.g. `hai_list_attestations`) reads them out of a JSON params blob. TASK_001 commits to typed `usize` args on `search_documents`/`query_by_*`, so haiigo MUST define a new macro variant that takes `*const c_char` plus two `size_t` values at the C ABI.
- [ ] Read `rust/haiigo/src/lib.rs::result_to_json` (line 40) and `result_unit_to_json` (line 47). Both take `Result<String, _>`/`Result<(), _>`. Neither handles `Result<Option<String>, _>`. `get_memory` and `get_soul` return `Option<String>` from binding-core, so haiigo MUST define a new helper `result_option_to_json` that emits `{"ok":null}` for `None` and `{"ok":"<inner-json-string>"}` for `Some`. Pattern matches `result_unit_to_json` plus the `Some` arm.
- [ ] Search `rust/haiigo/src/lib.rs` for any **bytes-returning** FFI export. Conclusion from PRD §7 risk #2: `get_raw_email` returns base64-in-JSON, not raw bytes. **There is NO existing bytes-returning convention in haiigo.** TASK_004 must define one.
- [ ] Read `go/ffi/ffi.go` lines 1184–1268 (the existing stub block). Replace `notWiredThroughLibhaiigo(...)` with real cgo calls.
- [ ] Read `go/ffi_iface.go` lines 117–141 — confirm exact method signatures (`StoreDocument(signedJSON string) (string, error)`, etc.).
- [ ] Read `go/ffi/ffi.go` lines 34–153 — this is the **inline cgo `/* ... */` block** that declares all C `extern` symbols. There is NO separate `jacs_cgo.h` file (verified — `find go -name "*.h"` returns nothing). The 20 new `hai_*` C declarations + `hai_get_record_bytes` + `hai_free_bytes` MUST be appended to this inline block.
- [ ] Read `go/jacs_document_store_ffi_test.go` and `go/mock_ffi_test.go` — the mock infrastructure expects each method to exist on `FFIClient`. Confirm that file's tests already cover the wire shape; we only need to wire `*ffi.Client` (the cgo client struct in `go/ffi/ffi.go`) so that all 20 method bodies actually invoke the new cgo bridges instead of returning `notWiredThroughLibhaiigo`.

## TDD: Tests First (Red)

### Unit Tests (Go side)
- [ ] **Existing tests must pass:** `go test ./go -run TestSaveMemoryCapturesContent` (and the rest of `jacs_document_store_ffi_test.go`) currently passes against `mockFFIClient` — most stay green untouched. The work is to replace `notWiredThroughLibhaiigo` stubs with real CGo calls so the runtime path is real.
- [ ] Test: `go/jacs_document_store_ffi_test.go` mostly passes without modification, EXCEPT for the 5 array-returning methods (`ListDocuments`, `GetDocumentVersions`, `QueryByType`, `QueryByField`, `QueryByAgent`). Those test stages flip from object-shaped (`json.RawMessage("{\"items\":[]}")` at line 199-204) to array-shaped (`[]string{"k1","k2"}` or `[]string{}`). The flip is mechanical and bounded — five test functions max.
- [ ] Test: `go/ffi/ffi_test.go` — add a CGo-gated test that calls `(*Client).SaveMemory("test")` against a real `httptest.Server` mock. The test must assert (a) a real cdylib symbol is loaded (not `notWiredThroughLibhaiigo`), and (b) the response key round-trips. Skipped under `!cgo`.
- [ ] Test (red, no cgo): `go test ./go -run TestListDocumentsReturnsStringSlice` — staging a `mockFFIClient.ListDocuments` returns `[]string{"a","b"}` and the test asserts `len(out)==2 && out[0]=="a"`. This RED-fails today because the iface signature is `(json.RawMessage, error)`. After the iface flip (Step 8) the test is green. Same shape test for `GetDocumentVersions`, `QueryByType`, `QueryByField`, `QueryByAgent`.
- [ ] **Build test:** `go build -tags cgo ./...` succeeds end-to-end with the new C extern declarations resolved by the freshly-built libhaiigo cdylib. Today, `go build ./...` succeeds (stubs satisfy the interface), but the runtime path always returns `notWiredThroughLibhaiigo`. After the iface flip the existing stubs at `go/ffi/ffi.go:1184-1268` must compile against the new `([]string, error)` signature — that's covered by replacing the stubs with real bridges in this task.

### Integration Tests
- [ ] DO NOT add live integration here. TASK_005 covers smoke.

## Implementation

- [ ] Step 1: Define a **bytes-returning FFI convention** for haiigo. Two options:

  **Option A — Length-prefixed buffer + free callback.** New macro:
  ```rust
  // Returns *mut c_uchar; caller passes a *mut size_t out-param to receive length.
  // Caller must call hai_free_bytes(ptr, len) to release.
  pub extern "C" fn hai_get_record_bytes(
      handle: HaiClientHandle, key: *const c_char, out_len: *mut usize
  ) -> *mut u8 { ... }
  pub extern "C" fn hai_free_bytes(ptr: *mut u8, len: usize) { ... }
  ```

  **Option B — Wrap bytes in JSON envelope as base64.** Reuse `ffi_method_str!`. The Go side decodes base64. Mirrors `get_raw_email`. Contradicts PRD §3.6 ("`get_record_bytes` does NOT base64-round-trip across the FFI boundary").

  **Choose Option A.** The PRD says native types per language. base64 round-trip is not free. Implement `hai_get_record_bytes` + `hai_free_bytes`.
- [ ] Step 2: Add 19 single/multi-arg `extern "C"` exports for the non-bytes methods. Group them in `// JACS Document Store` block at the bottom of the FFI section. Use `ffi_method_str!` / `ffi_method_str_two_args!` / `ffi_method_void!` / `ffi_method_noarg!` macros where signatures fit. For `get_memory` and `get_soul` use the new `result_option_to_json` helper from Step 2b — write a small extern body (no macro) that calls binding-core's `Option<String>`-returning method and routes through the new helper. For `search_documents` / `query_by_type` / `query_by_field` / `query_by_agent` see Step 2a — they take typed numeric args at the C ABI.

- [ ] Step 2a: Define a new macro `ffi_method_str_with_two_usize!` (and `ffi_method_str2_with_two_usize!` for `query_by_field`'s 2-string variant) that takes:
  ```rust
  pub extern "C" fn $fn_name(
      handle: HaiClientHandle,
      arg: *const c_char,
      limit: libc::size_t,
      offset: libc::size_t,
  ) -> *mut c_char { ... }
  ```
  Body parallels `ffi_method_str!` but passes the two `size_t` values straight into the binding-core method (Rust auto-converts `size_t` ↔ `usize` since both are word-sized). The cgo `extern` block in `go/ffi/ffi.go` declares these as `size_t` per `<stdlib.h>`, and the Go side passes `C.size_t(limit)`. This is the cleanest mapping; no JSON-blob round-trip.

- [ ] Step 2b: Define a new helper `fn result_option_to_json(result: Result<Option<String>, hai_binding_core::HaiBindingError>) -> String` next to the existing `result_to_json` (line 40 of `rust/haiigo/src/lib.rs`):
  ```rust
  fn result_option_to_json(result: Result<Option<String>, hai_binding_core::HaiBindingError>) -> String {
      match result {
          Ok(None) => r#"{"ok":null}"#.to_string(),
          Ok(Some(s)) => format!(r#"{{"ok":{s}}}"#),  // s is already a JSON string envelope
          Err(e) => error_to_json(&e),
      }
  }
  ```
  The Go side's existing `parseEnvelope` reads `ok` as `json.RawMessage`; for `null` it can be detected and surfaced as `("", nil)` (matching the existing `GetMemory() (string, error)` interface contract — empty string == None per fixture row `get_memory: returns string?`).
- [ ] Step 3: For `Option<String>` args (`save_memory`, `save_soul`, `list_documents`), follow the `hai_verify_status` pattern at line 322: empty cstring → `None`.
- [ ] Step 4: **Order matters.** Do these substeps in this exact order so the codebase compiles at every checkpoint:
   - Step 4a (FIRST — Rust side): Add the `extern "C"` exports in `rust/haiigo/src/lib.rs` (Step 2 / 2a / 2b) and rebuild `libhaiigo`. This must precede any Go-side `C.hai_*` reference.
   - Step 4b (SECOND — Go cgo declarations): Append the C `extern` lines from Step 8 to the inline cgo `/* ... */` block at top of `go/ffi/ffi.go`. Must precede Step 4d (any `C.hai_save_memory` etc. call requires the declaration in scope).
   - Step 4c (THIRD — iface flip): Flip `go/ffi_iface.go:122-130` for `ListDocuments`, `GetDocumentVersions`, `QueryByType`, `QueryByField`, `QueryByAgent` from `(json.RawMessage, error)` to `([]string, error)`. Also flip `go/mock_ffi_test.go:713,720,748,755,762` to return `[]string` and update any object-shaped staged values to array shape (e.g. `{"items":[]}` → `[]string{}`). At this point the existing stubs at `go/ffi/ffi.go:1184-1268` no longer compile — that's expected; Step 4d is right behind.
   - Step 4d (FOURTH — replace stubs): Replace the stub block at `go/ffi/ffi.go:1184-1268` with real CGo bridges, matching the new `[]string` signatures from 4c. Pattern (mirror existing methods):
  ```go
  func (c *Client) SaveMemory(content string) (string, error) {
      cContent := C.CString(content)
      defer C.free(unsafe.Pointer(cContent))
      result := C.hai_save_memory(c.handle, cContent)
      defer C.hai_free_string(result)
      return parseStringResponse(C.GoString(result))
  }
  ```
   - Step 4e (FIFTH — array-method parser): For the five array-returning methods, parse the JSON envelope's `ok` field directly into `[]string`. If `parseStringResponse` doesn't generalize, add a small `parseStringSliceResponse(json string) ([]string, error)` helper next to it.
- [ ] Step 5: For `GetRecordBytes` use the new convention from Step 1:
  ```go
  func (c *Client) GetRecordBytes(key string) ([]byte, error) {
      cKey := C.CString(key)
      defer C.free(unsafe.Pointer(cKey))
      var outLen C.size_t
      ptr := C.hai_get_record_bytes(c.handle, cKey, &outLen)
      if ptr == nil {
          // error path: read error from hai_last_error or similar
          ...
      }
      defer C.hai_free_bytes(ptr, outLen)
      return C.GoBytes(unsafe.Pointer(ptr), C.int(outLen)), nil
  }
  ```
  Decide the error-encoding for `hai_get_record_bytes`: the simplest approach matches `hai_client_new` (returns null on error, `hai_last_error()` retrieves the JSON error envelope from thread-local storage). Document the convention inline.
- [ ] Step 6: For `Option<String>` args via cgo, encode "empty cstring → None" matching `verify_status` line 322.
- [ ] Step 7: For `RemoveDocument` use the existing `ffi_method_void!` macro on the Rust side; the Go side does standard error-only return.
- [ ] Step 8 (declarations catalog — applied in Step 4b): The 22 extern declarations to append to the inline cgo `/* ... */` block in `go/ffi/ffi.go` (lines 34–153, current end-marker `extern char* hai_last_error();`). There is no separate `jacs_cgo.h` to update. Two declarations have non-string args:
  ```c
  // JACS Document Store — typed-numeric variants
  extern char* hai_search_documents(HaiClientHandle handle, const char* query, size_t limit, size_t offset);
  extern char* hai_query_by_type(HaiClientHandle handle, const char* doc_type, size_t limit, size_t offset);
  extern char* hai_query_by_field(HaiClientHandle handle, const char* field, const char* value, size_t limit, size_t offset);
  extern char* hai_query_by_agent(HaiClientHandle handle, const char* agent_id, size_t limit, size_t offset);

  // JACS Document Store — bytes-return convention
  extern unsigned char* hai_get_record_bytes(HaiClientHandle handle, const char* key, size_t* out_len);
  extern void hai_free_bytes(unsigned char* ptr, size_t len);
  ```
  The other 18 use `extern char*` with `*const c_char` args and follow the existing per-section block style.
- [ ] DRY check: of the 20 methods, 13 use existing macros (`ffi_method_str!` / `ffi_method_str_two_args!` / `ffi_method_void!` / `ffi_method_noarg!`); 4 use the new `ffi_method_str_with_two_usize!` variant from Step 2a (one of those four is the 2-string variant for `query_by_field`); 2 (`get_memory`, `get_soul`) use a hand-written body routed through the new `result_option_to_json` helper from Step 2b; 1 (`get_record_bytes`) is the novel bytes-return pair. Don't macroize the bytes pair (single use) or the option pair (two uses, but both differ on the binding-core method name only — not worth a macro).

## TDD: Tests Pass (Green)
- [ ] `cargo build -p haiigo --release` succeeds.
- [ ] `go build ./go/...` succeeds (without `-tags cgo`, the nocgo stub still applies).
- [ ] `go build -tags cgo ./go/...` succeeds with the real cdylib.
- [ ] `go test ./go/...` passes against `mockFFIClient` (no change to test).
- [ ] `go test -tags cgo ./go/ffi/...` passes against a real mock server (new tests in this task).

## Acceptance Criteria
- [ ] `*ffi.Client` satisfies `haiai.FFIClient` (compile-time check via `var _ haiai.FFIClient = (*ffi.Client)(nil)` somewhere — already true today via stubs, so a regression check rather than a new property).
- [ ] `nm libhaiigo.dylib | grep hai_save_memory` returns a symbol; same for the other 19 methods.
- [ ] `nm libhaiigo.dylib | grep hai_get_record_bytes` and `nm libhaiigo.dylib | grep hai_free_bytes` both return symbols.
- [ ] `(*Client).GetRecordBytes("img-1")` against an httptest server returning raw PNG bytes returns `[]byte` whose first 8 bytes match `\x89PNG\r\n\x1a\n`.
- [ ] No `notWiredThroughLibhaiigo` references remain in `go/ffi/ffi.go`.
- [ ] All 20+2 new C extern declarations are present in the inline cgo block (`go/ffi/ffi.go` lines 34–153 region), reachable via `nm` against the freshly-built `libhaiigo`.
- [ ] `result_option_to_json` helper exists in `rust/haiigo/src/lib.rs` and is referenced by the `hai_get_memory` and `hai_get_soul` extern bodies (verify via `grep result_option_to_json rust/haiigo/src/lib.rs` returning the helper definition + 2 usage sites).
- [ ] `ffi_method_str_with_two_usize!` (and the 2-string variant for `query_by_field`) macro is defined and used by the four typed-numeric externs (`hai_search_documents`, `hai_query_by_type`, `hai_query_by_field`, `hai_query_by_agent`).
- [ ] **TASK_004 owns the Go FFI interface flip** for the array-returning methods. Specifically: (1) flip `go/ffi_iface.go:123,124,127-130` from `(json.RawMessage, error)` to `([]string, error)` for `ListDocuments`, `GetDocumentVersions`, `QueryByType`, `QueryByField`, `QueryByAgent`. (2) `cgoFFIClient` in `go/ffi/ffi.go` parses the JSON-array envelope into `[]string` (add a helper `parseStringSliceResponse` if a similar one doesn't exist; check whether the existing `parseStringResponse` can be reused). (3) Update `go/mock_ffi_test.go:713,720,748,755,762` to return `[]string` (not `json.RawMessage`); flip any staged values to `[]string{"k1","k2"}`. (4) `go/jacs_document_store_ffi_test.go:158` (the `query_by_type` stage `{"items": []}`) and any equivalent stages in `mock_ffi_test.go` flip from object-shaped to array-shaped (`[]string{}`). TASK_005 then surfaces these on the public `*Client` as `[]string`.

## Execution
- **Agent Type**: `general-purpose`
- **Wave**: 2 (depends on TASK_001)
- **Complexity**: High (defines new bytes-return convention; cgo + Rust + Go in one task)
