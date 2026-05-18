# HAIAI WASM — Backend Contract Assumptions

This document captures the answers to the backend coordination questions
flagged in `HAIAI_WASM_PRD.md` §7 (Risks) so the wasm SSE / WS / HTTP
implementations downstream do not drift on assumptions. Tasks
003 / 011 / 012 / 018 / 019 / 029 reference this file.

Verified 2026-05-18 against `~/personal/hai/api` on the main branch.

## SSE auth contract

**Risk:** §7 — "SSE in browsers with proxies. `EventSource` does not support
custom headers in many browsers — which means we cannot send the
`Authorization` header on the initial GET. … if backend doesn't support
query-param auth, V1 SSE in browsers uses `fetch()` + `ReadableStream`."

**Answer (confirmed):** hai/api accepts JACS auth **only** via the
`Authorization` header.
- Evidence: `~/personal/hai/api/src/routes/agent_connection.rs:329-388`
  (`connect_agent` → `resolve_agent_for_connection`) and
  `~/personal/hai/api/src/middleware/agent_auth.rs:431-450`
  (`resolve_agent_auth` → `extract_and_validate_route_jacs_credentials` →
  `extract_jacs_credentials(headers)` — header parse only, no query
  fallback in the routes path).
- There is **no** `?token=` / query-token / cookie SSE-auth contract today.

**Downstream impact:**
- `rust/haiai/src/sse_wasm.rs` (Task 019) MUST use `fetch()` +
  `ReadableStream` with `Authorization: JACS …`. Do NOT introduce
  `web_sys::EventSource` even as an opt-in path until/unless the backend
  ships a query-token auth contract.
- `web-sys` features list in `rust/haiai/Cargo.toml` (Task 008) should
  **omit** `"EventSource"` until that contract lands; a stub feature can be
  added later without breaking the public surface.
- `node-wasm/index.ts` (Task 033) only exposes `transport: "sse" | "ws"`
  with `"sse"` defaulting to fetch-based streaming.

## CORS headers

**Risk:** §7 — "`https://hai.ai/api/*` must return
`Access-Control-Allow-Origin: *` (or a configurable origin) and
`Access-Control-Allow-Headers: authorization, content-type, x-jacs-*` for
browser requests to succeed."

**Answer (confirmed, with caveats):** hai/api has CORS configured in two
modes (`~/personal/hai/api/src/lib.rs:760-811`):

1. **`CORS_ALLOW_ALL=true`** — permissive: `Any` origin / `Any` methods /
   `Any` headers. Used in dev / preview deployments.
2. **Default (production)** — restricts to `https://hai.ai`, any
   `https://*.hai.ai` subdomain, and any origin listed in
   `ALLOWED_ORIGINS` (comma-separated env var, default
   `"https://hai.ai,https://www.hai.ai"`). Allowed methods: GET, POST, PUT,
   DELETE, OPTIONS. Allowed headers: `AUTHORIZATION`, `CONTENT_TYPE`. Allows
   credentials (cookies / `Authorization` round-trip).

**Caveats:**
- Production CORS does **not** include `x-jacs-*` in
  `allow_headers`. The wasm transport currently has no `x-jacs-*` request
  headers in scope (V1 only sends `Authorization`, `Content-Type`, and
  conditionally an `Accept: text/event-stream` for SSE — none custom).
  If we later add `X-JACS-Client: wasm/X.Y.Z` (PRD §10.4 optional), we
  must coordinate with the backend team to add it to `allow_headers`
  OR keep that header opt-in / behind a feature flag.
- An origin not in the allow-list (e.g. a developer running a Vite dev
  server on `http://localhost:5173`) will be rejected in production. The
  documented workaround is `ALLOWED_ORIGINS=…,http://localhost:5173`.

**Downstream impact:**
- `node-wasm/README.md` (Task 043) MUST document the production
  `ALLOWED_ORIGINS` requirement and the "deploy your own proxy" fallback
  for restricted production deployments.
- The Vite + Playwright smoke (Task 036) will run against either a hai/api
  preview URL with `CORS_ALLOW_ALL=true`, or a local mock that returns
  permissive CORS — both paths are acceptable.
- Any new request header added in `WasmFetchTransport` (Task 012) MUST be
  cleared with backend before merge.

## Attachment-as-base64 acceptance

**Risk:** §7 — "`reqwest` does not support multipart on wasm32. … V1
attachment uploads use base64-in-JSON only. If the existing backend does
not accept that for attachments, it's a backend coordination follow-up."

**Answer (assumed-OK, follow-up filed):** The native `SendEmailOptions`
already serializes `attachments: Vec<EmailAttachment>` as base64-in-JSON
today (see `rust/haiai/src/types.rs:344-370`). The native HTTP path POSTs
this as JSON, not multipart, so the wasm path inherits the same wire
contract — no backend change required for V1.

**Follow-up (not blocking V1):** confirm with the backend team that the
documented 25 MB raw-MIME cap (`docs/RAW_EMAIL_RETRIEVAL_ISSUES/ISSUE_004`,
mentioned in CLAUDE.md) applies the same way for base64-in-JSON
attachments as it does for multipart. If multipart had higher implicit
limits, the wasm path will hit the JSON limit sooner. No source-code
impact for V1.

**Downstream impact:**
- `rust/haiai/src/client.rs::send_email` already builds JSON bodies — no
  refactor needed for the wasm transport.
- `node-wasm/README.md` (Task 043) MUST document the base64-in-JSON
  encoding (no multipart fallback) and call out the per-attachment size
  consideration.

## Backend coordination outstanding

| Item | Status | Owner | Blocking? |
|------|--------|-------|-----------|
| SSE query-token auth | Not implemented; wasm uses `fetch()` + Stream | backend (deferred) | No — fallback already chosen |
| `X-JACS-Client: wasm/X.Y.Z` allow-header | Not configured; opt-in only | backend coordination | No — header is optional |
| Attachment-via-base64 size cap | Assumed = native; needs sanity-check | backend coordination | No — follow-up post-V1 |
| `ALLOWED_ORIGINS` includes dev origins | Operator concern (env var) | deployer | No — documented in README |

None of the above blocks V1 execution; all have documented fallbacks.
This file MUST be updated if any of the answers change.
