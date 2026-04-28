# Changelog

## 0.4.0 (2026-04-28)

### Breaking

- **Wire format for `Go SignBenchmarkResult`, `Go A2A artifact signing`, and `Rust RemoteJacsProvider::sign_file`** now uses RFC 8785 canonical JSON / canonical JACS file envelopes via JACS, replacing hand-rolled `json.Marshal` / `payload_b64` shapes. Records persisted by pre-0.4.0 clients carry `metadata.hash` and `jacsSignature.signature` over stdlib JSON / flat envelopes — they cannot be verified by 0.4.0+ clients. Re-sign or tombstone any persisted `benchmark_result` / `A2AWrappedArtifact` / `.jacs` file envelope from before this release. (Issue 003)
- **Removed:** `compute_content_hash` and `AttachmentInput`. Affects Python `haiai.compute_content_hash`, Node `computeContentHash`, Rust `haiai::compute_content_hash` / `haiai::AttachmentInput`, Go `haiai.ComputeContentHash`. The wrapper duplicated JACS canonical hashing and drifted from JACS's CRLF/BOM handling. Migrate to JACS's canonical hashing helpers (`jacs::crypt::hash::hash_public_key` in Rust; the equivalent `hashString` / `hash_string` exposed by the JACS Python and Node bindings). The corresponding `fixtures/email_conformance.json` entries that referenced the helper have been trimmed. (Issue 004)
- **Node `signResponse` and `unwrapSignedEvent` no longer fall back to the in-process `sortedKeyJson`** when the agent / canonicalizer is omitted. RFC 8785 canonicalization is delegated to JACS with no local fallback, matching Python's behaviour. `unwrapSignedEvent`'s `agent` parameter is now required; `signResponse`'s local-envelope path requires either a signer that exposes `signResponseSync` or an explicit `canonicalizer: JacsAgent`. (Task list #61.)

### Fixed

- **Python `canonicalize_json` against ephemeral JACS adapters.** When the loaded agent does not expose `canonicalize_json` (e.g. JACS's `_EphemeralAgentAdapter` wrapping `SimpleAgent`), `signing.canonicalize_json` now delegates to a stateless `jacs.JacsAgent()` instance — RFC 8785 canonicalization is keyless, so any JACS install can produce the canonical bytes. Eliminates the `JACS_TOO_OLD` regression that was failing 21 Python tests. (Issue 001)
- **Typed errors from `/api/v1/records`.** Non-success responses now surface as the typed `HaiError::Api { status, message }` variant rather than the catch-all `HaiError::Provider`. Cross-language SDKs map this to `ErrorKind::AuthFailed` / `NotFound` / `RateLimited` / `ApiError`. If you previously relied on `IsAuthError(err)` / `HaiAuthError` / `AuthenticationError` to fire on every records-endpoint error, audit your branches: a 5xx will no longer hit the auth-error branch. (Prior Issue 008.)
- **`query_by_type` / `query_by_agent` cursor walk.** Previously returned an empty array for any `offset >= 100` because the server's per-page cap is 100 and the SDK didn't walk further. Now walks forward via the server's cursor until `limit` records past `offset` are accumulated. If your code used "empty page" as a loop terminator, switch to "result count < limit" instead. (Prior Issue 009.)
- **`RemoteJacsProvider::sign_file` delegation.** Now delegates to `inner.sign_file_envelope` so the file envelope is byte-identical to `LocalJacsProvider::sign_file` (canonical `(jacsType="file", jacsLevel, jacsFiles[...])` shape). The previous hand-rolled `payload_b64` / flat `sha256` envelope diverged from the JACS schema and cross-language verification. (Prior Issue 006.)
- **`verify_dns_public_key` uses JACS canonical `hash_public_key`** rather than hand-rolled SHA-256 over PEM. Hash bytes are now byte-identical to JACS's reference. (Prior Issue 012.)
- **Makefile `test-go`** now sets `CGO_LDFLAGS` / `DYLD_LIBRARY_PATH` / `LD_LIBRARY_PATH` and depends on `build-haiigo` so a fresh-clone `make test-go` actually runs. (Prior Issue 017.)

### Notes

- **Go `ctx` is not propagated through cgo** in the 20 doc-store methods on `*Client`. Method signatures take `_ctx context.Context` for forward-compatibility, but cancellation is not honoured at the FFI boundary — a `WithTimeout` will not abort the in-flight cgo call. The 19 pre-existing methods that go through the Go reqwest layer still honour ctx. Honest cancellation via a watchdog goroutine is tracked as a follow-up. (Prior Issue 015.)
- **PRD / TASK / EMAIL_VERIFICATION recipe docs** for the 0.3.0 work were moved out of `docs/haisdk/` into `docs/archive/2026-04/` rather than deleted. `CLAUDE.md` now points consumers at the archived `EMAIL_VERIFICATION.md`.
- **JACS pinned to `=0.10.0`** for now; bump in 0.4.x once 0.10.1+ is published.

## 0.3.0

- **Remote JACS Document Store (`RemoteJacsProvider`)**: `JacsDocumentProvider` impl that talks to `hai-api` at `/api/v1/records/...` instead of touching the filesystem. Set `JACS_STORAGE=remote` (or `--storage remote` in CLI) and the same trait calls a developer makes against `LocalJacsProvider` (sign + store + fetch + version + list + search) work over HTTPS. Auth uses the existing `Authorization: JACS {jacsId}:{ts}:{sig}` header; signing keys never leave the client. Maps server 4xx to `HaiError::Provider(server_message)` and 5xx to `HaiError::Provider("server error: ...")`. See `~/personal/haisdk/rust/haiai/src/jacs_remote.rs`.
- **MEMORY.md / SOUL.md wrappers (D5)**: New inherent methods on `RemoteJacsProvider` — `save_memory(Option<&str>)`, `save_soul(Option<&str>)`, `get_memory()`, `get_soul()`. When called with `None`, reads `MEMORY.md` / `SOUL.md` from CWD. Server-route-free (sets `jacsType="memory"|"soul"` on the signed envelope).
- **Native signed-media uploads (D9)**: `RemoteJacsProvider::store_text_file(path)` / `store_image_file(path)` / `get_record_bytes(key)` post pre-signed inline-text or image bytes to `/api/v1/records` with the right `Content-Type: text/markdown; profile=jacs-text-v1` / `image/png` / `image/jpeg` / `image/webp`. Local sanity check rejects unsigned files BEFORE making any HTTP call.
- **FFI parity contract (`fixtures/ffi_method_parity.json`)**: new `jacs_document_store` section listing 20 methods (13 trait + 4 D5 + 3 D9). `total_method_count` 72 → 92; contract test renamed `ffi_method_parity_total_count_is_92`.
- **Integration tests**: `~/personal/haisdk/rust/haiai/tests/jacs_remote_integration.rs` — 13 tests (12 `#[ignore]`) covering the full SDK ↔ server end-to-end surface against the hosted-stack Docker compose. Run with `cargo test -p haiai --test jacs_remote_integration -- --ignored` after exporting `HAI_URL` to your hosted stack.
- **`remote` storage label** (TASK_010): `haiai::config::resolve_storage_backend_label("remote")` returns `Ok("remote")`; `JACS_STORAGE=remote` is now a valid env value alongside `fs` / `rusqlite` / `sqlite`.
- **Local media signing (Layer 8)**: New `JacsMediaProvider` trait exposes JACS's offline `sign_text_file` / `verify_text_file` / `sign_image` / `verify_image` / `extract_media_signature` operations. PNG / JPEG / WebP via metadata channel (PNG iTXt / JPEG APP11 / WebP XMP) plus optional LSB robust mode for PNG/JPEG.
- **CLI**: 5 new subcommands (`sign-text`, `verify-text`, `sign-image`, `verify-image`, `extract-media-signature`) mirroring the `jacs` reference CLI flag set and exit codes (0 valid / 1 bad-or-strict-missing / 2 permissive-missing).
- **MCP**: 5 new tools (`hai_sign_text`, `hai_verify_text`, `hai_sign_image`, `hai_verify_image`, `hai_extract_media_signature`) with `require_relative_path_safe` traversal guard imported from `jacs::validation`.
- **Python / Node / Go**: per-language wrappers on `HaiClient` and `AsyncHaiClient` (Python sync+async). Result dataclasses / interfaces / structs, plus `MediaVerifyStatus*` constants in Go.
- **Public API contract**: the user-facing key is `robust` everywhere (CLI flag, MCP param, Python kwarg, Node interface, Go field). The JACS-internal `scan_robust` field is hidden inside binding-core's parser.
- **Cross-language verify-parity contract**: pre-signed `fixtures/media/signed.{png,jpg,webp,md}` (signed once by `rust/haiai/tests/regen_media_fixtures.rs`, watchdog'd by `CHECKSUMS.txt` + `SIGNER.json`) drive 32 verify-parity tests — 8 each in Rust / Python / Node / Go — proving the four SDKs agree on `valid` and `hash_mismatch` for the same input bytes (PRD §5.5).
- **JACS pinned to 0.10.0** across `rust/haiai`, `rust/haiai-cli`, and `rust/hai-mcp`. Binding-core widened to `Box<dyn JacsMediaProvider>` (supertrait `JacsProvider` continues to compile transparently).

## 0.2.2

- **One-step registration**: `haiai init --name <name> --key <key>` generates keypair, registers, and claims username in one command. Removed `checkUsername` / `claimUsername` from all SDKs.
- **CI uses JACS workspace patch**: Test jobs clone JACS at the expected relative path instead of stripping `[patch.crates-io]`.
- **Fix JACS `rotate()` API**: Updated call to pass the new `algorithm` parameter added upstream.
- **Native SDKs promoted to beta**.

## 0.2.0

- **FFI-first architecture**: All HTTP calls, auth, retry, and URL building now live in Rust and are exposed to Python, Node, and Go via FFI bindings (PyO3, napi-rs, CGo). Eliminates 4 separate HTTP implementations.
- **New crates**: `hai-binding-core`, `haiipy`, `haiinpm`, `haiigo` provide the shared FFI dispatch and per-language bindings.
- **No local crypto in SDKs**: Removed `go/crypto.go`, `go/signing.go`, and equivalents. All signing/verification delegates to JACS via FFI.
- **Removed Go CLI and MCP server**: `go/cmd/haiai/` and `go/cmd/hai-mcp/` deleted. Rust `haiai` binary is the canonical CLI and MCP server.
- **MCP email signing fixed**: `EmbeddedJacsProvider` now implements `sign_email_locally` via an `AgentSigner` wrapper, matching CLI behavior.
- **MCP log file support**: `haiai --log-file <path> mcp` captures logs to a file for debugging when spawned by an MCP client.
- **Email reply/forward fixes**: Fixed subject-line CR/LF corruption, added proper threading headers (In-Reply-To, References), fixed signature attachment handling.
- **SSE and WebSocket transports migrated to FFI**: Streaming connections use an opaque handle pattern through `hai-binding-core`.
- **CI updates**: Reduced PR test matrix, added concurrency controls, updated publish workflows for FFI build requirements.
