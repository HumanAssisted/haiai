# Task 003: haiinpm (napi-rs) exposes 20 doc-store methods

## Context

PRD: `docs/haisdk/JACS_DOCUMENT_STORE_FFI_PRD.md` §4.4.

After TASK_001, `HaiClientWrapper` has 20 doc-store methods. This task adds 20 napi-rs entries on the `HaiClient` struct in `rust/haiinpm/src/lib.rs`.

The Node FFI client `node/src/ffi-client.ts` (lines 138–162 interface, lines 994–1183 impl) **already** declares and calls all 20 methods on `this.native.*`. The TypeScript-side declarations for `listDocuments` / `getDocumentVersions` / `queryByType` / `queryByField` / `queryByAgent` are typed as `Promise<Record<string, unknown>>` but the underlying Rust trait returns `Vec<String>` (per TASK_001 step 3). TASK_005 step 6 fixes those declarations to `Promise<string[]>`. **TASK_003 only adds the napi-rs symbols on the Rust side**; it returns each as a JSON-serialized `Result<String>` — the JS-side parsing into `string[]` lives in `node/src/ffi-client.ts` (TASK_005 territory).

## Goal

`new HaiClient(config); await client.saveMemory("...")` returns the record key. Same call works for all 20 methods. `await client.getRecordBytes("k")` returns a Node `Buffer`/`Uint8Array`.

## Research First

- [ ] Read `rust/haiinpm/src/lib.rs` lines 1–200 to confirm the `#[napi] pub async fn` pattern.
- [ ] Confirm napi-rs `Buffer` import: `use napi::bindgen_prelude::*` already present at line 15. `Buffer::from(Vec<u8>)` is the idiomatic conversion.
- [ ] Read `node/src/ffi-client.ts` lines 138–162 (interface declarations) — confirm the napi-rs side will produce these exact method names + signatures (camelCase). napi-rs translates Rust `snake_case` to JS `camelCase` automatically.
- [ ] Read `node/src/ffi-client.ts` lines 1177–1183 (`getRecordBytes`) — return type is `Promise<Uint8Array>`. napi-rs `Buffer` serializes to `Uint8Array` in TypeScript types.
- [ ] Confirm napi-rs handles `Option<String>` arg → JS `string | null` (used in `verify_status` line 92).

## TDD: Tests First (Red)

The Node tests in `node/tests/jacs-document-store-ffi.test.ts` already exist and exercise the FFIClientAdapter against `createMockFFI`. Those tests verify the adapter, not the native binding.

### Unit Tests (Node side, vitest)
- [ ] Test: `node/tests/jacs-document-store-ffi.test.ts` continues to pass without modification. (Confirm by running `npm test` in `node/`.)
- [ ] Test: `node/tests/ffi-client-doc-store.test.ts` (new file) — load `FFIClientAdapter` via the createMockFFI helper, call `storeDocument`, `signAndStore`, `getDocument`, `getLatestDocument`, `getDocumentVersions`, `listDocuments`, `removeDocument`, `updateDocument`, `searchDocuments`, `queryByType`, `queryByField`, `queryByAgent`, `storageCapabilities`. Verify each round-trips through the mock and JSON-decodes correctly.
   - **Only add tests not already covered by `jacs-document-store-ffi.test.ts`.** That file covers the 7 D5/D9 methods. We need coverage for the 13 trait CRUD/query methods.

### Integration Tests
- [ ] DO NOT add live native-binding tests in this task. TASK_005 covers smoke.

## Implementation

- [ ] Step 1: In `rust/haiinpm/src/lib.rs`, add a `// JACS Document Store` section after the existing `// Verification` section. Same divider style as the other sections.
- [ ] Step 2: Add 20 `#[napi] pub async fn` methods. Pattern:

  ```rust
  #[napi]
  pub async fn save_memory(&self, content: Option<String>) -> Result<String> {
      self.inner.save_memory(content).await.map_err(to_napi_err)
  }
  ```
- [ ] Step 3: For `get_record_bytes`, return `Buffer`:
  ```rust
  #[napi]
  pub async fn get_record_bytes(&self, key: String) -> Result<Buffer> {
      let bytes = self.inner.get_record_bytes(&key).await.map_err(to_napi_err)?;
      Ok(Buffer::from(bytes))
  }
  ```
  Confirm in research that the Node-side adapter's `Promise<Uint8Array>` is satisfied by napi `Buffer`.
- [ ] Step 4: For `get_memory` / `get_soul`, return `Option<String>` — napi-rs maps to `Promise<string | null>`. Matches the `getMemory(): Promise<string | null>` declaration in `node/src/ffi-client.ts:156`.
- [ ] Step 5: For `remove_document`, return `Result<()>` — matches `removeDocument(): Promise<void>` declaration.
- [ ] Step 6: napi-rs already converts `snake_case` to `camelCase` at the JS boundary, so no manual aliasing needed.

## TDD: Tests Pass (Green)
- [ ] `cargo build -p haiinpm` succeeds.
- [ ] `cd node && npm install && npm run build && npm test` passes.
- [ ] Existing `node/tests/jacs-document-store-ffi.test.ts` still passes.
- [ ] New trait CRUD/query tests pass.

## Acceptance Criteria
- [ ] `node -e "const {HaiClient} = require('haiinpm'); const c = new HaiClient('{\"jacs_config_path\":\"/tmp/x\"}'); console.log(typeof c.saveMemory, typeof c.getRecordBytes)"` prints `function function`.
- [ ] All 20 method names match the `node/src/ffi-client.ts` interface declarations exactly (camelCase).
- [ ] Each napi-rs entry has the correct Rust return type: `Result<String>` for key-returning methods, `Result<Option<String>>` for `get_memory`/`get_soul`, `Result<Buffer>` for `get_record_bytes`, `Result<()>` for `remove_document`. Runtime verification of the JS-side `Buffer.isBuffer(...)` and reject-on-missing-config behavior is in TASK_006.

## Execution
- **Agent Type**: `general-purpose`
- **Wave**: 2 (depends on TASK_001)
- **Complexity**: Medium
