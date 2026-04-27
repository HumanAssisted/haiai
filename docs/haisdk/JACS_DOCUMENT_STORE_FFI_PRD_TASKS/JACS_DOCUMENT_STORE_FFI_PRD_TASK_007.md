# Task 007: Final cleanup pass — full test suite, DRY review, stale-stub removal

## Context

PRD: `docs/haisdk/JACS_DOCUMENT_STORE_FFI_PRD.md` §6 (Evaluation Criteria — DRY/Quality 15%, Scope discipline 10%).

After TASKS 001–006, the 20 doc-store methods are wired all the way through. This task is the final QA pass that verifies cross-cutting properties the per-task work could miss.

## Goal

The full test suite passes across all 4 SDKs, no dead code remains, and the cleanup specifically named in the PRD is done. No new functionality.

## Research First

- [ ] Read `rust/hai-binding-core/methods.json` — confirm the `jacs_document_store` group was added in TASK_001. If not, add it now.
- [ ] Search the repo for any remaining `notWiredThroughLibhaiigo` reference. Should be 0 after TASK_004.
- [ ] Search the repo for any `TODO` or `FIXME` introduced in TASKS 001–006.
- [ ] `grep -r "TASK_001\|TASK_002\|TASK_003\|TASK_004\|TASK_005\|TASK_006" rust/ python/ node/ go/` — confirm no in-source comments reference these task numbers (they should reference the PRD or Issue 025, not transient task IDs).
- [ ] Check `python/src/haiai/_ffi_adapter.py` lines 548–558 — the docstring says "When the haiipy native shim hasn't been wired to a particular method yet, the call surfaces a clear AttributeError". With this PRD shipping, that comment is stale; update to reflect that all 20 methods are now wired.

## TDD: Tests First (Red)
- [ ] No new tests in this task. The acceptance is "all existing tests pass".

## Implementation

- [ ] Step 1: Run `make test` (or `make test-rust && make test-python && make test-node && make test-go`). Fix any failure.
- [ ] Step 2: Run `make smoke` (TASK_006). Confirm all three smoke tests pass when the native bindings are built.
- [ ] Step 3: Run `cargo clippy --all-targets --all-features -- -D warnings` on the workspace. Fix warnings introduced by the new code.
- [ ] Step 4: Run `cargo fmt --all` and `npm run format` (in node) and `gofmt -w go/`.
- [ ] Step 5: Run `python -m ruff check python/` and `mypy python/`. Fix any new violations.
- [ ] Step 6: Search for and remove dead code:
    - Any `notWiredThroughLibhaiigo` references in `go/ffi/ffi.go`.
    - Any `_sync` shim in `_ffi_adapter.py` that wraps a `try/except RuntimeError` around a method that no longer raises (check the 20 doc-store entries — they raise from real FFI errors now, so the existing `try/except` is correct; do NOT delete those).
    - Any commented-out CLI/MCP method calls that used to be stubs.
- [ ] Step 7: Update the docstring at `python/src/haiai/_ffi_adapter.py:548-558` from "When the haiipy native shim hasn't been wired ... AttributeError" to a current description (all 20 are wired through, errors surface as `RuntimeError("ProviderError: ...")`).
- [ ] Step 8: Verify `rust/hai-binding-core/methods.json` contains 20 entries with `"group": "jacs_document_store"` (one per fixture method). The file is a flat JSON array — confirm the count via `python3 -c "import json; d=json.load(open('rust/hai-binding-core/methods.json')); print(sum(1 for m in d['methods'] if m.get('group')=='jacs_document_store'))"` returns 20. If TASK_001 didn't write them, do it here. Also confirm `summary.async_methods` and `summary.total_public_methods` were updated accordingly.
- [ ] Step 9: Run the parity test by hand: `cargo test -p haiai ffi_method_parity_total_count_is_92`. Must pass — no fixture changes.
- [ ] Step 10: Run `make check-versions` and `make check-jacs-versions`. No version churn expected from this PRD; if any test imports drift JACS, investigate.
- [ ] Step 11: Verify `scripts/ci/check_no_local_crypto.sh` passes — i.e., no new local crypto introduced. RemoteJacsProvider delegates everything to JACS; this should be a no-op check.
- [ ] Step 11b: Enforce PRD §3.8 / Rule 5 (no HTTP outside Rust). Grep the SDK source roots for new HTTP-client imports introduced by this PRD's diffs (regex matches only `+` lines, i.e. additions):
   - Python: `git diff main -- python/src/haiai/client.py python/src/haiai/async_client.py | grep -E "^\+.*(import httpx|import requests|import aiohttp|from http\.client|from urllib)"` must be empty.
   - Node: `git diff main -- node/src/client.ts node/src/ffi-client.ts | grep -E "^\+.*(import .* from ['\"](node-fetch|axios|undici|node:http|node:https)['\"])"` must be empty. Avoid the previous regex `http\.|https\.` — that flags JSDoc URLs and method-name fragments. Restrict to module-import lines only.
   - Go: `git diff main -- go/client.go | grep -E "^\+.*\"net/http\""` must be empty (httptest is test-only and lives in `_test.go`, not `client.go`).
- [ ] DRY check: Glob for the 20 method names across the 4 SDKs and count. Each name should appear (occurrences = expected definition sites + binding-internal call sites; tests excluded from this count):
   - Once in `fixtures/ffi_method_parity.json`
   - Once in `rust/hai-binding-core/src/lib.rs` `HaiClientWrapper`
   - Once each in haiipy `lib.rs` async + sync entry pair = 2 in `rust/haiipy/src/lib.rs`
   - Once in `rust/haiinpm/src/lib.rs`
   - Once in `rust/haiigo/src/lib.rs` (the `extern "C"` export)
   - Three sites in `go/ffi/ffi.go`: (1) the inline cgo C extern declaration, (2) the `*ffi.Client` method body, (3) any internal helper calls if added — typically just (1) and (2) = 2
   - Once each in Python `client.py`, `async_client.py`, FFI adapter sync (`_ffi_adapter.py`), FFI adapter async (`_ffi_adapter.py`) = 3 unique files (4 definitions)
   - Once each in Node `ffi-client.ts` interface declaration + `ffi-client.ts` class method + `node/src/client.ts` public method = 3 occurrences across 2 files
   - Once each in Go `ffi_iface.go`, `mock_ffi_test.go`, `client.go` = 3 (test file is allowed here because it mirrors the interface)

   Anywhere a name appears more times than the expected ceiling, look for accidental duplication (helper layers, dead old definitions).

## TDD: Tests Pass (Green)
- [ ] `make test` passes.
- [ ] `make smoke` passes.
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` clean.
- [ ] `make check-versions` passes.

## Acceptance Criteria
- [ ] `make test` exits 0.
- [ ] `make smoke` exits 0 (or skips cleanly if native artifacts not built).
- [ ] No `notWiredThroughLibhaiigo` references remain.
- [ ] No `TODO`/`FIXME` references with `Issue 025` or `JACS_DOCUMENT_STORE_FFI_PRD` remain (the implementation is complete).
- [ ] `rust/hai-binding-core/methods.json` contains exactly 20 entries with `"group": "jacs_document_store"` (verifiable via the Step 8 one-liner) and `summary.async_methods` reflects the 20-entry addition.
- [ ] `cargo test -p haiai ffi_method_parity_total_count_is_92` passes.
- [ ] No new files in `python/src/haiai/`, `node/src/`, `go/` other than test files and the adapter changes called out in TASKS 001–006.
- [ ] Adapter return-type fixes from TASK_005 step 6 are present: `_ffi_adapter.py`'s `list_documents` / `get_document_versions` / `query_by_*` return `list[str]`; `node/src/ffi-client.ts` interface declares `Promise<string[]>` for the same; `go/ffi_iface.go`'s `ListDocuments`/`GetDocumentVersions`/`QueryBy*` declarations have flipped from `(json.RawMessage, error)` to `([]string, error)` and `go/ffi/ffi.go`'s `cgoFFIClient` parses to `[]string`; `go/mock_ffi_test.go` returns `[]string`. The MockFFIAdapter / mockFFI / mock_ffi_test return matching shapes.
- [ ] cgo additions from TASK_004 step 2a/2b are present: `result_option_to_json` helper exists in `rust/haiigo/src/lib.rs` and is referenced exactly twice (`hai_get_memory`, `hai_get_soul`); `ffi_method_str_with_two_usize!` macro is defined and used by `hai_search_documents`/`hai_query_by_type`/`hai_query_by_field`/`hai_query_by_agent`; the inline cgo extern block in `go/ffi/ffi.go` lists `size_t` args for those four C declarations.

## Execution
- **Agent Type**: `general-purpose`
- **Wave**: 5 (depends on 001–006)
- **Complexity**: Low
