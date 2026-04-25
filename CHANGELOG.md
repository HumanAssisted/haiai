# Changelog

## 0.3.0

- **Local media signing (Layer 8)**: New `JacsMediaProvider` trait exposes JACS's offline `sign_text_file` / `verify_text_file` / `sign_image` / `verify_image` / `extract_media_signature` operations. PNG / JPEG / WebP via metadata channel (PNG iTXt / JPEG APP11 / WebP XMP) plus optional LSB robust mode for PNG/JPEG.
- **CLI**: 5 new subcommands (`sign-text`, `verify-text`, `sign-image`, `verify-image`, `extract-media-signature`) mirroring the `jacs` reference CLI flag set and exit codes (0 valid / 1 bad-or-strict-missing / 2 permissive-missing).
- **MCP**: 5 new tools (`hai_sign_text`, `hai_verify_text`, `hai_sign_image`, `hai_verify_image`, `hai_extract_media_signature`) with `require_relative_path_safe` traversal guard imported from `jacs::validation`.
- **Python / Node / Go**: per-language wrappers on `HaiClient` and `AsyncHaiClient` (Python sync+async). Result dataclasses / interfaces / structs, plus `MediaVerifyStatus*` constants in Go.
- **Public API contract**: the user-facing key is `robust` everywhere (CLI flag, MCP param, Python kwarg, Node interface, Go field). The JACS-internal `scan_robust` field is hidden inside binding-core's parser.
- **Cross-language verify-parity contract**: pre-signed `fixtures/media/signed.{png,jpg,webp,md}` (signed once by `rust/haiai/tests/regen_media_fixtures.rs`, watchdog'd by `CHECKSUMS.txt` + `SIGNER.json`) drive 32 verify-parity tests — 8 each in Rust / Python / Node / Go — proving the four SDKs agree on `valid` and `hash_mismatch` for the same input bytes (PRD §5.5).
- **JACS pinned to 0.11.0** across `rust/haiai`, `rust/haiai-cli`, and `rust/hai-mcp`. Binding-core widened to `Box<dyn JacsMediaProvider>` (supertrait `JacsProvider` continues to compile transparently).

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
