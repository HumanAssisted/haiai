# Task 006: Real-FFI smoke tests for Python, Node, Go (one method each)

## Context

PRD: `docs/haisdk/JACS_DOCUMENT_STORE_FFI_PRD.md` §5.5.

Existing tests at every layer use mocks:
- `rust/hai-binding-core/src/lib.rs#tests` — uses httpmock against `RemoteJacsProvider`.
- `python/tests/test_jacs_document_store_ffi.py`, `node/tests/jacs-document-store-ffi.test.ts`, `go/jacs_document_store_ffi_test.go` — use `MockFFIAdapter` / `createMockFFI` / `mockFFIClient`.
- `python/tests/test_client_doc_store.py` (TASK_005) — uses `MockFFIAdapter`.
- `rust/haiai/tests/jacs_remote_integration.rs` — `#[ignore]` until live stack.

**No test exercises the real native binding loaded into the host language.** This is the gap the PRD calls out: a Python/Node user would have hit `AttributeError` the first time they touched any doc-store method.

This task adds ONE smoke test per language SDK that loads the real native binding (haiipy / haiinpm / haiigo) and round-trips `save_memory("test")` against an `httpmock` server. Gated so it doesn't run unless the binding is built.

## Goal

When `make test-python`, `make test-node`, `make test-go` run with the local Rust build of the FFI artifacts available, each suite includes a passing test that proves the real binding loads and dispatches `save_memory` end-to-end.

## Research First

- [ ] Read `rust/haiai/tests/jacs_remote_integration.rs` for the existing JACS-config test scaffolding. Decide whether the smoke test reuses the same `tests/fixtures/` JACS agent or stands one up freshly.
- [ ] Read `python/tests/conftest.py` for `pytest` mark / fixture patterns. Decide marker name (suggest `@pytest.mark.native_smoke`).
- [ ] Read `node/tests/setup.ts` for vitest config patterns; pick a way to skip when `haiinpm` native isn't built locally (e.g. try-import + skip).
- [ ] Read `go/Makefile` (or `go.mod` + `go test` invocation) to confirm the build tag setup; pick a tag (suggest `cgo_smoke`).
- [ ] All three smoke tests need a REAL HTTP listener (not a Python/Node-level mock library) because the call goes language → FFI → Rust reqwest → real network port. Use Python stdlib `http.server.HTTPServer` (no new dep), Node `node:http.createServer`, Go `httptest.NewServer`.
- [ ] Re-confirm: with TASK_001's design, the native binding's HTTP traffic is from Rust `reqwest` (inside `RemoteJacsProvider`). Mocks above the FFI boundary (respx, vitest fetch mocks, gomock httpClient) cannot intercept this.

## TDD: Tests First (Red)

### Smoke Tests (one per language)
- [ ] Test: `python/tests/test_ffi_native_smoke.py` — guarded by `@pytest.mark.native_smoke`.
   1. Spin up a stdlib `http.server.HTTPServer` on `127.0.0.1:0` in a background thread (no `pytest_httpserver` dep).
   2. Build a JACS agent in a tmp dir via `LocalJacsProvider` — see `rust/haiai/tests/jacs_remote_integration.rs` for an existing scaffolding pattern. Reuse if practical; otherwise create `python/tests/fixtures/smoke_jacs/` with a `jacs.config.json` that auto-creates a tmp keypair on load.
   3. Construct `haiipy.HaiClient` with `jacs_config_path` pointing to the tmp config and `base_url` pointing to the mock server URL (`f"http://127.0.0.1:{server.server_port}"`).
   4. The mock handler responds to `POST /api/v1/records` with HTTP 200 + body `{"key":"smoke:v1"}`.
   5. Call `client.save_memory_sync("smoke")`.
   6. Assert the result equals `"smoke:v1"`.
   7. Assert one POST hit `/api/v1/records` with `Content-Type: application/json` and body containing `"jacsType":"memory"`.
- [ ] Test: `node/tests/ffi-native-smoke.test.ts` — same flow, vitest, skipped via `try { require('haiinpm') } catch { it.skip(...) }`. Use `node:http.createServer` for the mock listener.
- [ ] Test: `go/ffi_native_smoke_test.go` — `//go:build cgo_smoke` build tag. Uses `httptest.NewServer` + real cgo client.

### Implementation Tests (only the smoke tests above; no source code changes besides build wiring)

## Implementation

- [ ] Step 1: Add the marker `native_smoke` to `python/pyproject.toml` `[tool.pytest.ini_options]` with a description "Tests that load the real haiipy native binding".
- [ ] Step 2: Create `python/tests/test_ffi_native_smoke.py`. Use `pytest.importorskip("haiipy")` to skip cleanly when the native binding isn't built. **Do NOT use `respx`** — `respx` mocks the Python `httpx` library; the smoke test exercises the Rust `reqwest` HTTP client running INSIDE the haiipy native binding, which `respx` cannot intercept. Use Python's stdlib `http.server.HTTPServer` in a background thread (no new dev-dep needed) — the test only needs one endpoint and one assertion. The mock server must bind to `127.0.0.1:<random_port>` and the smoke test passes that URL via `base_url` in the haiipy config JSON. Same approach for the Node and Go smoke tests (their mocks are `node:http.createServer` and `httptest.NewServer`, both of which are real HTTP listeners).
- [ ] Step 3: Create `node/tests/ffi-native-smoke.test.ts`. Use `try { require('haiinpm') } catch { skip }` pattern. Bring in `tinyhttp` or vanilla `node:http` to mock.
- [ ] Step 4: Create `go/ffi_native_smoke_test.go` with `//go:build cgo_smoke` and use `httptest.NewServer`.
- [ ] Step 5: Update `Makefile`:
   - Add `make smoke` target that runs all three.
   - Add `make smoke-python`, `make smoke-node`, `make smoke-go`.
   - Wire `smoke` into CI on a separate workflow lane (see "follow-up"; for this PRD only commit the targets).
- [ ] DRY check: each smoke test is ~30 lines. Don't share infrastructure across languages — they each have their own mock conventions. Don't migrate other tests to use the mock server pattern.

## TDD: Tests Pass (Green)
- [ ] `pip install -e python/.[dev,mcp]` (with haiipy native built locally), then `pytest -m native_smoke python/tests/` passes.
- [ ] `cd node && npm install && npm run build && npm test -- ffi-native-smoke` passes.
- [ ] `cd go && go test -tags cgo_smoke ./...` passes (requires `libhaiigo.dylib` next to the binary or in `LD_LIBRARY_PATH`).
- [ ] All existing tests still pass; smoke tests are SKIPPED (not failed) when the native binding isn't available.

## Acceptance Criteria
- [ ] `pytest -m native_smoke python/tests/` runs exactly 1 test, passing.
- [ ] `npm test -- ffi-native-smoke` runs exactly 1 test, passing.
- [ ] `go test -tags cgo_smoke -run NativeSmoke ./go/...` runs exactly 1 test, passing.
- [ ] When the native binding is missing, each smoke test SKIPS with a clear message (does NOT fail).
- [ ] Each smoke test asserts BOTH: (a) the FFI returned the expected key string, AND (b) the mock server received exactly one POST to `/api/v1/records` with `jacsType:"memory"` in the body.

## Execution
- **Agent Type**: `general-purpose`
- **Wave**: 4 (depends on 002, 003, 004, 005 — though strictly only 002/003/004; gating on 005 keeps it sequential)
- **Complexity**: Medium
