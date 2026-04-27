# Changelog

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
