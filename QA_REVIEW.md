# HAISDK QA Review

**Reviewer:** Code QA / Copy Editor Agent
**Date:** 2026-02-17
**Scope:** Python, Node, Go SDKs -- all source files, tests, and cross-language consistency

---

## 1. JACS-Only Auth Verification

**Status: PASS**

Grepped for `api_key`, `apiKey`, `hak_`, `Bearer`, `HAI_API_KEY` across all `python/src/`, `node/src/`, `go/` source files.

**Findings:**
- `python/src/jacs/hai/client.py:4` -- docstring comment: "JACS-only authentication (no API key / Bearer fallback)". Documentation only; correct.
- `python/src/jacs/hai/models.py` -- `RegistrationResult` dataclass previously had an `api_key` field but it has been **correctly removed**. Only `agent_id` and `jacs_id` fields remain.
- **All three SDKs use `Authorization: JACS {jacsId}:{timestamp}:{signature}` exclusively.** No Bearer token fallback path exists in any SDK.

**Note:** Two Python tests (`test_models.py::TestRegistrationResult::test_defaults` and `test_with_api_key`) are stale -- they still reference the removed `api_key` field. These tests should be updated to match the current model.

---

## 2. Feature Parity Check

### Python SDK -- PASS

All required methods verified in `/Users/jonathan.hendler/personal/haisdk/python/src/jacs/hai/client.py`:

| Method | Present | Line |
|--------|---------|------|
| `testconnection` | Yes | 164 |
| `hello_world` | Yes | 197 |
| `verify_hai_message` | Yes | 274 |
| `free_chaotic_run` | Yes | 693 |
| `baseline_run` | Yes | 776 |
| `submit_benchmark_response` | Yes | 934 |
| `register` | Yes | 329 |
| `benchmark` | Yes | 566 |
| `_poll_benchmark_result` | Yes | 634 |
| `connect` (SSE+WS) | Yes | 1090 |
| `_sse_connect` | Yes | 1117 |
| `_ws_connect` | Yes | 1216 |
| `disconnect` | Yes | 1336 |
| `is_connected` | Yes | 1355 |
| `status` | Yes | 433 |
| `get_agent_attestation` | Yes | 505 |
| `sign_benchmark_result` | Yes | 1044 |

Module-level convenience functions (13 total): `_get_client`, `testconnection`, `hello_world`, `register`, `status`, `benchmark`, `free_chaotic_run`, `baseline_run`, `submit_benchmark_response`, `sign_benchmark_result`, `connect`, `disconnect`, `register_new_agent`, `verify_agent` -- **all present**.

### Node SDK -- PASS (with gaps noted)

Methods in `/Users/jonathan.hendler/personal/haisdk/node/src/client.ts`:

| Method | Present | Notes |
|--------|---------|-------|
| `hello` | Yes | line 163 |
| `register` | Yes | line 228 |
| `status` | Yes | line 278 |
| `freeChaoticRun` | Yes | line 307 |
| `baselineRun` | Yes | line 341 |
| `submitResponse` | Yes | line 437 |
| `connect` (SSE/WS) | Yes | line 484 |
| `disconnect` | Yes | line 502 |
| `verifyHaiMessage` | Yes | line 212 |
| `onBenchmarkJob` | Yes | line 524 |
| `benchmark` (legacy) | **MISSING** | See below |
| `getAgentAttestation` | **MISSING** | See below |
| `signBenchmarkResult` | **MISSING** | See below |

**Missing from Node:**
1. `getAgentAttestation()` -- Python and Go both have this. Node does not.
2. `signBenchmarkResult()` -- Python has this; Node does not.
3. `benchmark()` -- Legacy suite-based benchmark (Python has it). Node only has tier-based runs.

### Go SDK -- PASS (with gaps noted)

Methods in `/Users/jonathan.hendler/personal/haisdk/go/client.go`:

| Method | Present | Notes |
|--------|---------|-------|
| `Hello` | Yes | line 211 |
| `Register` | Yes | line 249 |
| `Status` | Yes | line 264 |
| `FreeChaoticRun` | Yes | line 323 |
| `BaselineRun` | Yes | line 328 |
| `Benchmark` | Yes | line 306 |
| `SubmitResponse` | Yes | line 333 |
| `GetAgentAttestation` | Yes | line 349 |
| `VerifyAgent` | Yes | line 359 |
| `FetchRemoteKey` | Yes | line 369 |
| `ConnectSSE` | Yes | transport_sse.go:38 |
| `ConnectWS` | Yes | transport_ws.go:103 |
| `TestConnection` | Yes | line 220 |
| `verifyHaiMessage` | **MISSING** | No standalone signature verification |
| `signBenchmarkResult` | **MISSING** | No local result signing |

---

## 3. Data Model Completeness

### Python -- PASS (13/13 dataclasses)

All required models present in `/Users/jonathan.hendler/personal/haisdk/python/src/jacs/hai/models.py`:

| Dataclass | Present | Line |
|-----------|---------|------|
| `TranscriptMessage` | Yes | 134 |
| `FreeChaoticResult` | Yes | 151 |
| `BaselineRunResult` | Yes | 173 |
| `JobResponseResult` | Yes | 223 |
| `HaiRegistrationResult` | Yes | 48 |
| `HaiEvent` | Yes | 20 |
| `BenchmarkResult` | Yes | 196 |
| `HaiStatusResult` | Yes | 90 |
| `HaiRegistrationPreview` | Yes | 71 |
| `HelloWorldResult` | Yes | 111 |
| `AgentVerificationResult` | Yes | 240 |
| `RegistrationResult` | Yes | 39 |
| `AgentConfig` | Yes | 10 |

### Node -- PASS (all interfaces present)

In `/Users/jonathan.hendler/personal/haisdk/node/src/types.ts`:

| Interface | Present | Notes |
|-----------|---------|-------|
| `HelloWorldResult` | Yes | line 105 |
| `RegistrationResult` | Yes | line 123 |
| `FreeChaoticResult` | Yes | line 133 |
| `BaselineResult` | Yes | line 147 |
| `CertifiedResult` | Yes | line 163 (extra -- not in Python) |
| `JobResponseResult` | Yes | line 184 |
| `StatusResult` | Yes | line 196 |
| `TranscriptMessage` | Yes | line 86 |
| `HaiEvent` | Yes | line 42 |
| `BenchmarkJob` | Yes | line 64 |
| `AgentConfig` | Yes | line 18 |
| `BenchmarkJobConfig` | Yes | line 74 |

**Note:** Node has `CertifiedResult` (line 163) which Python and Go do not. This is forward-looking but represents a parity divergence.

### Go -- PASS (core types present)

In `/Users/jonathan.hendler/personal/haisdk/go/types.go`:

| Struct | Present |
|--------|---------|
| `RegistrationResult` | Yes |
| `StatusResult` | Yes |
| `PublicKeyInfo` | Yes |
| `BenchmarkResult` | Yes |
| `BenchmarkTestResult` | Yes |
| `AgentEvent` | Yes |
| `BenchmarkJobConfig` | Yes |
| `ConversationTurn` | Yes |
| `ModerationResponse` | Yes |
| `JobResponseResult` | Yes |
| `HelloResult` | Yes |
| `AttestationResult` | Yes |
| `VerifyResult` | Yes |
| `HaiSignature` | Yes |

**Missing Go types (vs Python):** `TranscriptMessage`, `FreeChaoticResult`, `BaselineRunResult`, `HaiRegistrationPreview`, `AgentVerificationResult` -- Go uses generic `BenchmarkResult` for all tiers. This is an intentional architectural simplification but reduces type safety for tier-specific results.

---

## 4. Error Hierarchy

### Python -- PASS

`/Users/jonathan.hendler/personal/haisdk/python/src/jacs/hai/errors.py`:

| Class | Present | Notes |
|-------|---------|-------|
| `HaiError` | Yes | Base exception |
| `HaiApiError` | Yes | Extends HaiError |
| `HaiAuthError` | Yes | Extends HaiApiError |
| `HaiConnectionError` | Yes | Extends HaiError |
| `RegistrationError` | Yes | Extends HaiError |
| `BenchmarkError` | Yes | Extends HaiError |
| `SSEError` | Yes | Extends HaiError |
| `WebSocketError` | Yes | Extends HaiError |
| `AuthenticationError` alias | Yes | `= HaiAuthError` (line 100) |

### Node -- PARTIAL PASS

`/Users/jonathan.hendler/personal/haisdk/node/src/errors.ts`:

| Class | Present | Notes |
|-------|---------|-------|
| `HaiError` | Yes | Base class |
| `AuthenticationError` | Yes | Extends HaiError |
| `HaiConnectionError` | Yes | Extends HaiError |
| `WebSocketError` | Yes | Extends HaiError |
| `RegistrationError` | **MISSING** | |
| `BenchmarkError` | **MISSING** | |
| `SSEError` | **MISSING** | |
| `HaiApiError` | **MISSING** | |

Node has only 4 error classes vs Python's 8+1 alias. The missing error types mean that callers cannot distinguish between registration failures, benchmark failures, and SSE errors when catching exceptions.

### Go -- PASS

`/Users/jonathan.hendler/personal/haisdk/go/errors.go` uses an `ErrorKind` enum pattern:

| Kind | Present |
|------|---------|
| `ErrConnection` | Yes |
| `ErrRegistration` | Yes |
| `ErrAuthRequired` | Yes |
| `ErrInvalidResponse` | Yes |
| `ErrKeyNotFound` | Yes |
| `ErrConfigNotFound` | Yes |
| `ErrConfigInvalid` | Yes |
| `ErrSigningFailed` | Yes |
| `ErrTimeout` | Yes |
| `ErrTransport` | Yes |
| `ErrForbidden` | Yes |
| `ErrNotFound` | Yes |
| `ErrRateLimited` | Yes |

Go has the richest error categorization of all three SDKs.

---

## 5. Namespace Package Verification

**Status: PASS**

- `/Users/jonathan.hendler/personal/haisdk/python/src/jacs/` -- contains only the `hai/` subdirectory. **No `__init__.py`** present. Correct namespace package behavior.
- `/Users/jonathan.hendler/personal/haisdk/python/src/jacs/hai/__init__.py` -- exists and exports all public symbols. Correct.
- `py.typed` marker file present in `jacs/hai/`.

---

## 6. Test Coverage

### Python -- 120 tests across 6 files -- PASS

| File | Tests |
|------|-------|
| `test_client.py` | 40 |
| `test_signing.py` | 22 |
| `test_models.py` | 19 |
| `test_errors.py` | 18 |
| `test_config.py` | 11 |
| `test_sse.py` | 10 |
| **Total** | **120** |

Exceeds the 100+ target.

### Node -- 68 test cases across 4 files -- FAIL (below target)

| File | Tests |
|------|-------|
| `types.test.ts` | 25 |
| `client.test.ts` | 21 |
| `signing.test.ts` | 16 |
| `sse.test.ts` | 7 |
| **Total** | **68** |

Below the 100+ target by 32 tests. Notable gaps:
- No `ws.test.ts` -- WebSocket transport is untested
- No `config.test.ts` -- Config loading is untested
- No `errors.test.ts` -- Error construction is tested inline in client tests but not standalone
- No `crypt.test.ts` -- Crypto operations are untested (sign/verify/keygen)

### Go -- 55 tests across 8 files -- FAIL (below target)

| File | Tests |
|------|-------|
| `client_test.go` | 15 |
| `signing_test.go` | 11 |
| `config_test.go` | 7 |
| `sse_test.go` | 5 |
| `ws_test.go` | 5 |
| `auth_test.go` | 4 |
| `types_test.go` | 4 |
| `errors_test.go` | 4 |
| **Total** | **55** |

Below the 100+ target by 45 tests.

---

## 7. Cross-Language Consistency

**Status: PASS (with naming convention gaps)**

### Naming Convention Adherence

| Feature | Python (snake_case) | Node (camelCase) | Go (PascalCase) |
|---------|-------------------|-----------------|----------------|
| Hello/test connection | `testconnection` | -- | `TestConnection` |
| Hello | `hello_world` | `hello` | `Hello` |
| Register | `register` | `register` | `Register` |
| Status | `status` | `status` | `Status` |
| Free chaotic | `free_chaotic_run` | `freeChaoticRun` | `FreeChaoticRun` |
| Baseline | `baseline_run` | `baselineRun` | `BaselineRun` |
| Submit response | `submit_benchmark_response` | `submitResponse` | `SubmitResponse` |
| Connect | `connect` | `connect` | `ConnectSSE`/`ConnectWS` |
| Disconnect | `disconnect` | `disconnect` | `Close` |
| Verify message | `verify_hai_message` | `verifyHaiMessage` | -- |
| Attestation | `get_agent_attestation` | -- | `GetAgentAttestation` |
| Sign result | `sign_benchmark_result` | -- | -- |

**Observations:**
1. Python uses `hello_world` while Node uses `hello` -- **name divergence**. Both are reasonable per convention but users switching languages will trip over this.
2. Python uses `submit_benchmark_response` while Node uses `submitResponse` -- the Node name omits "benchmark" which is imprecise since the response is specifically for a benchmark job.
3. Go's `Close()` for disconnect is idiomatic Go but deviates from the Python/Node `disconnect()` convention.
4. Node is missing `testconnection` -- the client doesn't expose a health check method at all.

### Auth Header Format -- Consistent

All three SDKs produce: `JACS {jacsId}:{timestamp}:{signature_base64}` -- verified in:
- Python: `client.py:130-132`
- Node: `client.ts:124-128`
- Go: `auth.go:17-24`

### SSE/WS Endpoints -- Consistent

All three SDKs use:
- SSE: `GET /api/v1/agents/connect`
- WS: `/ws/agent/connect`

---

## 8. Config Loading

**Status: PASS**

### Python
`/Users/jonathan.hendler/personal/haisdk/python/src/jacs/hai/config.py`:
- Explicit path: `config.load("./path/to/jacs.config.json")` -- Yes (line 36)
- JACS_CONFIG_PATH env var: **Not directly supported** in `config.py`. The migration inventory says `JACS_CONFIG_PATH` should be supported, but the Python config module only takes an explicit path. The `load()` default is `"./jacs.config.json"`.
- Auto-discovery of `jacs.config.json`: Yes, via default parameter (line 36)

**Issue:** Python config does not check `JACS_CONFIG_PATH` env var. The Node and Go SDKs do.

### Node
`/Users/jonathan.hendler/personal/haisdk/node/src/config.ts`:
- Explicit path: `loadConfig("./path")` -- Yes (line 13-14)
- JACS_CONFIG_PATH env var: Yes (line 15)
- Auto-discovery: `./jacs.config.json` fallback -- Yes (line 16)

### Go
`/Users/jonathan.hendler/personal/haisdk/go/config.go`:
- Explicit path: `LoadConfig("./path")` -- Yes (line 18)
- JACS_CONFIG_PATH env var: Yes, via `DiscoverConfig()` (line 41)
- Auto-discovery: `./jacs.config.json` then `~/.jacs/jacs.config.json` -- Yes (lines 46, 53)

Go adds a third discovery location (`~/.jacs/`) that Python and Node lack. This is a nice enhancement but is a cross-language behavior difference.

---

## Findings by Severity

### CRITICAL Issues

**C1. Node SDK missing `RegistrationError`, `BenchmarkError`, `SSEError`, `HaiApiError` error classes**
- **Location:** `/Users/jonathan.hendler/personal/haisdk/node/src/errors.ts`
- **Issue:** Only 4 error classes exist (HaiError, AuthenticationError, HaiConnectionError, WebSocketError). Python has 8+1.
- **Impact:** Node consumers cannot catch specific error types for registration vs benchmark vs SSE failures. All non-auth, non-connection errors bubble up as generic `HaiError`, making error handling in production agents much harder.
- **Recommendation:** Add `RegistrationError`, `BenchmarkError`, `SSEError`, and `HaiApiError` classes.

**C2. Node SDK missing `getAgentAttestation()` and `signBenchmarkResult()` methods**
- **Location:** `/Users/jonathan.hendler/personal/haisdk/node/src/client.ts`
- **Issue:** These methods exist in Python (and `GetAgentAttestation` in Go) but are absent from Node.
- **Impact:** Node agents cannot verify other agents' trust levels or sign benchmark results for independent verification. These are core trust and transparency features.
- **Recommendation:** Implement both methods in the Node client.

### IMPORTANT Issues

**I1. Node and Go test coverage below 100+ target**
- **Location:** Node: 68 tests; Go: 55 tests
- **Issue:** Both fall short of the 100+ per-language target. Node is missing dedicated tests for WebSocket transport, config loading, crypto primitives, and error classes. Go has better file coverage but fewer tests per file.
- **Impact:** Reduced confidence in correctness for edge cases, especially transport layer.
- **Recommendation:** Priority additions:
  - Node: Add `ws.test.ts`, `config.test.ts`, `crypt.test.ts` (would add ~40+ tests)
  - Go: Add more negative-path tests for SSE reconnection, WS failure modes, config edge cases

**I2. Python config does not support `JACS_CONFIG_PATH` env var**
- **Location:** `/Users/jonathan.hendler/personal/haisdk/python/src/jacs/hai/config.py:36`
- **Issue:** The `load()` function only accepts an explicit path or defaults to `./jacs.config.json`. It does not check the `JACS_CONFIG_PATH` environment variable, unlike Node and Go.
- **Impact:** Users following the Node/Go documentation pattern (`export JACS_CONFIG_PATH=...`) will find it doesn't work in Python.
- **Recommendation:** Add env var check: `path = Path(config_path or os.environ.get("JACS_CONFIG_PATH", "./jacs.config.json"))`.

**I3. Two stale Python tests reference removed `api_key` field**
- **Location:** `/Users/jonathan.hendler/personal/haisdk/python/tests/test_models.py:48-54`
- **Issue:** `TestRegistrationResult.test_defaults` and `test_with_api_key` reference `RegistrationResult.api_key` which was correctly removed from the model. These 2 tests now fail.
- **Impact:** 2 test failures out of 120. The model is correct; the tests are stale.
- **Recommendation:** Update or remove these 2 test cases to match the current `RegistrationResult(agent_id, jacs_id)` signature.

**I4. Node `StatusResult` differs structurally from Python `HaiStatusResult`**
- **Location:** Node `types.ts:196-207` vs Python `models.py:90-107`
- **Issue:** Python `HaiStatusResult` has `registered` (bool), `registration_id`, `hai_signatures` (list). Node `StatusResult` has `active` (bool), `benchmarkCount` (int), but no `registered`, `registrationId`, or `haiSignatures` fields.
- **Impact:** The same API response (`GET /api/v1/agents/{id}/status`) will be parsed differently depending on which SDK you use. This makes cross-language documentation confusing and could mask server-side changes.
- **Recommendation:** Align Node `StatusResult` fields with Python's `HaiStatusResult`: add `registered`, `registrationId`, `haiSignatures`; consider keeping `active` and `benchmarkCount` as extras.

**I5. Python `client.py` signed response format differs from Node**
- **Location:** Python `signing.py:227-248` vs Node `signing.ts:121-153`
- **Issue:** Python's `sign_response()` produces a JACS document with top-level keys `version`, `document_type`, `data`, `metadata`, `jacsSignature`. Node's `signResponse()` produces `payload`, `metadata`, `signature`. These are structurally different JACS document formats.
- **Impact:** If the API accepts both formats, this is fine. But if validation is strict on one format, one SDK will break. Also makes cross-language debugging harder.
- **Recommendation:** Standardize the JACS document envelope format. Pick one shape and use it in all SDKs.

**I6. Go SSE reconnection uses fixed 5-second delay**
- **Location:** `/Users/jonathan.hendler/personal/haisdk/go/transport_sse.go:192`
- **Issue:** `OnBenchmarkJob()` reconnects after a fixed 5-second delay. Python and Node use exponential backoff (doubling up to 30s/60s).
- **Impact:** In high-failure scenarios, Go agents will hammer the server every 5 seconds while Python/Node agents back off appropriately.
- **Recommendation:** Implement exponential backoff matching Python/Node behavior.

**I7. Go `transport_sse.go` silently drops events when channel is full**
- **Location:** `/Users/jonathan.hendler/personal/haisdk/go/transport_sse.go:103-105`
- **Issue:** When the events channel (buffer 16) is full, events are silently dropped (`default:` case in select).
- **Impact:** If the consumer processes events slowly, benchmark jobs could be silently lost with no error, logging, or notification.
- **Recommendation:** At minimum, log a warning when events are dropped. Consider blocking instead, or making the buffer size configurable.

### SUGGESTIONS

**S1. Node `hello()` name should match Python `hello_world()`**
- **Location:** Node `client.ts:163`
- **Issue:** Python uses `hello_world()`, Node uses `hello()`. The concepts map 1:1.
- **Recommendation:** This is a minor ergonomics issue. Document the correspondence in the migration guide if not renaming.

**S2. Go `ResolveKeyPath` assumes `{agentName}.private.pem` naming**
- **Location:** `/Users/jonathan.hendler/personal/haisdk/go/config.go:72`
- **Issue:** Go resolves the key path as `{jacsKeyDir}/{agentName}.private.pem`. Python and Node search for `*private*.pem` or `*.pem` in the key directory.
- **Impact:** If users generate keys with the Python/Node SDK and then try to use them from Go, the key file might not be found due to different naming expectations.
- **Recommendation:** Add fallback searches matching Python/Node behavior: `agent_private_key.pem`, `private_key.pem`, etc.

**S3. Python `verify_agent()` has a dead code path for DNS verification**
- **Location:** `/Users/jonathan.hendler/personal/haisdk/python/src/jacs/hai/client.py:1638-1641`
- **Issue:** Level 2 DNS verification has a comment "needs a DNS query library or server-side check" and `dns_valid` is never set to `True`. This means Level 2 is unreachable, and `verify_agent(min_level=2)` will always fail even if DNS verification would pass.
- **Recommendation:** Either implement DNS verification or document clearly that Level 2 is not available in the standalone SDK and must use server-side verification.

**S4. Node `baselineRun()` payment polling uses raw `fetch` instead of `fetchWithRetry`**
- **Location:** `/Users/jonathan.hendler/personal/haisdk/node/src/client.ts:374`
- **Issue:** During payment status polling, the code calls raw `fetch()` instead of `this.fetchWithRetry()`. This means no timeout protection and no retry for transient failures during the poll loop.
- **Recommendation:** Use `fetchWithRetry` or at minimum add an AbortController timeout.

**S5. Go `ConnectWS` does not send a JACS handshake message after connection**
- **Location:** `/Users/jonathan.hendler/personal/haisdk/go/transport_ws.go:103-140`
- **Issue:** Python and Node send an explicit JACS-signed handshake message over WebSocket after the connection is established (`_build_ws_handshake`). Go only sets the Auth header during the HTTP upgrade but doesn't send a post-connection handshake.
- **Impact:** If the server expects a post-connection handshake message (as the JACS monolith does), Go WebSocket connections will fail or be treated as unauthenticated.
- **Recommendation:** Implement a post-connection handshake in Go's `ConnectWS` matching the Python/Node behavior.

---

## Test Quality Assessment

### Python Tests -- Good

- Tests use `respx` for HTTP mocking, which is the recommended approach for `httpx`-based clients. Good boundary-level mocking.
- Error paths are tested (401, 403, 404, 409, 429).
- `conftest.py` provides `loaded_config`, `ed25519_keypair`, and `jacs_id` fixtures.
- `verify_agent` has both positive (valid Level 1 signature) and negative (bad JSON, missing sig) tests.
- `sign_benchmark_result` test verifies the signed document structure.
- **Concern:** `test_client.py:377` -- `sign_benchmark_result` test only checks that `signed_document` is in the result and `document_type` is present but doesn't verify the signature is actually valid. A test that verifies the signature round-trips would be stronger.

### Node Tests -- Acceptable but thin

- Uses `vitest` with `createMockFetch` helper -- good pattern.
- Auth header format is verified in multiple tests (JACS prefix, no Bearer).
- Error class hierarchy tested (instanceof checks).
- **Concern:** No test for `baselineRun()` -- the most complex method (Stripe checkout + polling + benchmark) has zero test coverage.
- **Concern:** No test for `onBenchmarkJob()` -- the primary harness loop is untested.
- **Concern:** `verifyHaiMessage` only tests empty inputs, not a valid signature round-trip.

### Go Tests -- Strong for what exists

- Uses `httptest.NewServer` for real HTTP server testing -- excellent boundary testing.
- JACS auth header verification is done server-side in test handlers.
- Error classification is table-tested (all HTTP status codes mapped).
- Context cancellation test (`TestContextCancellation`) is a good SRE-relevant test.
- `TestFetchRemoteKey` and `TestFetchRemoteKeyNotFound` cover the remote key distribution path.
- **Concern:** No tests for `GetAgentAttestation()` or `VerifyAgent()`.
- **Concern:** SSE reconnection behavior is not tested.

---

## Overall Quality Assessment

**Overall Grade: B+**

### What is done well:
- JACS-only auth is correctly enforced across all three SDKs. No API key fallback leakage.
- Python SDK is the most complete with 100% feature coverage, full error hierarchy, and 120 tests.
- All three SDKs have clean separation of concerns: auth, config, crypto, transport, signing, and client logic are in separate modules.
- Cross-language canonical JSON implementation matches (sorted keys, compact separators) which is critical for signature verification interop.
- Error handling is thoughtful -- retry with backoff, auth-specific errors, connection-specific errors.
- Go has particularly clean architecture: functional options, context propagation, proper goroutine lifecycle management.

### What needs improvement:
1. **Node feature parity** -- Missing 3 client methods and 4 error classes. This is the biggest gap.
2. **Node and Go test count** -- Both below the 100+ target. Critical methods are untested.
3. **Cross-language signed document format divergence** (Python vs Node) -- Should be standardized.
4. **Python JACS_CONFIG_PATH env var** -- Missing from the most complete SDK.
5. **Go SSE event dropping** -- Silent data loss is a production reliability risk.
6. **Go WebSocket handshake** -- Missing post-connection handshake may break server compatibility.

### Priority Fix Order:
1. Node error classes (C1) -- blocks proper error handling by consumers
2. Node missing methods (C2) -- blocks feature parity
3. Python JACS_CONFIG_PATH (I2) -- cross-language behavior consistency
4. Signed document format alignment (I5) -- API compatibility
5. Node/Go test additions (I1) -- confidence and safety
6. Go event dropping (I7) -- production reliability

---

---

## 9. No JACS Maturin/Native Dependency Required

**Status: PASS**

### Python
- **Build system:** `hatchling` (pure Python wheel builder). No maturin, setuptools-rust, or cffi.
- **Runtime dependencies:** `httpx>=0.27`, `cryptography>=42.0` only. No `jacs` PyPI package, no PyO3, no native bindings.
- **Namespace package:** `src/jacs/` has no `__init__.py` (correct implicit namespace package). `jacs.hai` resolves to haisdk's own code, not to any external `jacs` package.
- **Crypto:** Uses `cryptography` library (OpenSSL-backed wheel) for Ed25519, not JACS's Rust `ring` bindings.
- **Verified:** All core imports and Ed25519 sign/verify operations work in a clean venv without any external `jacs` dependency installed.

### Node
- **Dependencies:** `ws` only (WebSocket client). No native addons, no node-gyp, no NAPI, no WASM.
- **Crypto:** Uses `node:crypto` built-in Ed25519 support. Zero external crypto dependencies.
- **Build:** TypeScript compilation only. No native build step.

### Go
- **Dependencies:** `github.com/gorilla/websocket` only. No CGO, no `unsafe`, no external C libraries.
- **Crypto:** Uses Go standard library `crypto/ed25519`. Zero external crypto dependencies.

### Optional JACS Integration
Python `pyproject.toml` has optional extras (`langchain`, `crewai`, `anthropic`, etc.) that depend on `jacs[langchain]`, `jacs[crewai]`, etc. These are **opt-in** for users who want framework adapters and correctly route to the JACS PyPI package as an optional dependency, not a required one.

---

## 10. Test Execution Results

Tests were executed 2026-02-17 against all three SDKs.

### Python (venv: Python 3.11.12)
```
120 collected, 118 passed, 2 failed
```
- **2 failures:** `test_models.py::TestRegistrationResult::test_defaults` and `test_with_api_key` -- stale tests referencing removed `api_key` field on `RegistrationResult`. The model was correctly updated but tests were not.
- All other 118 tests pass cleanly.

### Node (vitest 1.6.1)
```
4 files, 68 tests, 68 passed, 0 failed
Duration: 170ms
```
- All tests pass.

### Go (go 1.23)
```
55 tests, 55 passed, 0 failed
Duration: 6.52s (includes 5s context cancellation timeout test)
```
- All tests pass.
- `examples/minimal` has no test files (expected -- it's an example).

---

## Addendum: Corrected Findings from Initial Review

The initial QA review (Section 1) incorrectly stated that `RegistrationResult.api_key` still existed in `models.py`. Upon running the actual tests, the 2 failures revealed that the field was already removed from the model but the tests were not updated. This is a test-code mismatch, not a source-code issue. The corrected finding is documented in Section 1 above.

The `I3` (Important Issue) from the initial review ("Vestigial api_key field") should be **downgraded to S-level (Suggestion)**: update the 2 stale test cases.

---

*Generated by Code QA / Copy Editor Agent -- 2026-02-17*


Let's fix the remaining issues, asking quesitons as we go. 

Using /Users/jonathan.hendler/personal/hai/docs/haisdk/JACS_AND_HAI_CLEANUP.md, /Users/jonathan.hendler/personal/hai/docs/haisdk/LIBRARY_MIGRATION_PROGRESS.md, and /Users/jonathan.hendler/personal/hai/docs/haisdk//MIGRATION_INVENTORY.md, and QA review. Make sure docs are up to date. Make sure the documentation is up to date as we go.

We need good integration tests that test a real agent registred with hai at localhost:3000. 
DO NOT make trivial tests. Make it real in the hai test env. 
make sure you use /Users/jonathan.hendler/personal/hai/docs/HUMAN_MVP.md up to IGNORE - OUTDATED  line.
~/personal/