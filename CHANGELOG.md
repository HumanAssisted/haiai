# Changelog

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
