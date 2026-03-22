# Agents — Lessons Learned

Quality audit of the HAIAI SDK (Python, Node, Go, Rust) — March 2026.

## Critical Findings

### Wrong endpoints (Go)
- `ProRun()` called `/api/benchmark/subscribe` instead of `/api/benchmark/purchase`.
- `GetAgentAttestation()` used singular `/attestation` instead of plural `/attestations`.
- Key lookup functions (`FetchKeyByEmail`, etc.) defaulted to a separate `keys.hai.ai` host instead of the main HAI endpoint.
- **Lesson:** Endpoint paths drift silently across SDKs. The `fixtures/contract_endpoints.json` fixture now exists to catch this — keep it updated when adding new endpoints.

### Missing payload fields (Python, Go)
- Python `hello_world()` omitted `agent_id` from the POST body (present in the other 3 SDKs).
- `benchmark()` in Python and Go omitted `"transport": "sse"`.
- **Lesson:** When one SDK adds a field, grep all SDKs for the same call and confirm parity.

## High-Priority Patterns

### No retry / no backoff
- Go and Rust had zero retry logic. Node threw immediately on 429.
- **Fix:** All SDKs now retry on `[429, 500, 502, 503, 504]` with exponential backoff (default 3 attempts). The status-code list is validated by a cross-SDK contract test (`test_retryable_status_codes_match_python`).
- **Lesson:** Retry policy is a cross-cutting concern — treat it like a shared spec, not per-SDK discretion.

### Unbounded reads (Go)
- `io.ReadAll()` without a size cap could exhaust memory on malicious responses.
- **Fix:** `limitedReadAll()` helper caps at 10 MB.
- **Lesson:** Any `ReadAll`/`.text()`/`.read()` on an HTTP response should have a size limit.

## Medium-Priority Patterns

### Private key exposure (Node)
- Private key was stored as an enumerable property, visible in `JSON.stringify()` and property iteration.
- **Fix:** Moved to a module-level `WeakMap`, non-enumerable and non-serializable.
- **Lesson:** Secrets should never be plain object properties. Use `WeakMap`, closures, or `Object.defineProperty` with `enumerable: false`.

### Hardcoded passphrase (Node)
- `register()` used `"register-temp"` as a temp key passphrase.
- **Fix:** `crypto.randomBytes()`.
- **Lesson:** Grep for string literals that look like passphrases/tokens — they're easy to miss in review.

### Base URL validation
- Python, Node, and Rust accepted arbitrary URL schemes (`ftp://`, `file://`, `javascript://`).
- **Fix:** All SDKs now reject non-`http(s)://` schemes at construction time.
- **Lesson:** Validate inputs at the boundary, not deep in the call chain.

### Unbounded reconnection (Node, Rust)
- SSE/WebSocket reconnection loops had no cap — could retry forever.
- **Fix:** `maxReconnectAttempts` (default 10) with exponential backoff.
- **Lesson:** Every retry loop needs a ceiling.

### Missing SSE timeout (Go)
- SSE HTTP client had no `ResponseHeaderTimeout` — could hang indefinitely.
- **Fix:** 30-second timeout on response headers.

### Race conditions (Go)
- Mutable fields (`haiAgentID`, `agentEmail`) accessed without synchronization.
- **Fix:** `sync.RWMutex` around mutable state.
- **Lesson:** Any field written after construction needs a mutex in Go.

### Missing query parameters (Python, Go, Node)
- `list_messages()` and `search_messages()` were missing `has_attachments`, `since`, `until` filters that Rust already had.
- **Lesson:** When Rust (the reference implementation) has a feature, all SDKs should too.

### Reply endpoint undocumented (Rust)
- Rust uses a server-side `/email/reply` endpoint; other SDKs compose client-side. Neither approach was documented.
- **Fix:** Added to `fixtures/contract_endpoints.json` with a migration note.
- **Lesson:** Architectural divergences between SDKs need explicit documentation, not just code.

## Process Takeaways

1. **Contract fixtures prevent drift.** `fixtures/contract_endpoints.json` is the source of truth for endpoints. Add new entries there first.
2. **Cross-SDK grep is mandatory.** Before merging a behavior change in one SDK, grep the same function/field name across all four.
3. **Test the sad path.** Most bugs were in error handling, retry logic, and edge cases — not the happy path.
4. **Security is a default, not a feature.** Key exposure, hardcoded secrets, and unbounded reads were all "it works" code that was silently dangerous.
5. **Rust is the reference.** When in doubt about what a function should do, check the Rust implementation first — it tends to be the most complete.
