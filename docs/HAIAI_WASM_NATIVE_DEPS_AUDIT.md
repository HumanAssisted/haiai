# HAIAI WASM — Native-Dep Audit

Audit of every wasm-incompatible call site in `rust/haiai/src/`. Produced
in Wave 2 (Task 007) per HAIAI_WASM_PRD.md §7 Risks and §4.2.1. This file
is the input for Tasks 009 (cfg-gate FS + heavy crates), 010 (cfg-gate
tokio_tungstenite + tempfile + tokio::time), 014 (WebSocketTransport
trait), and 015 (reconnect-backoff helper hoist).

Verified 2026-05-18 against `rust/haiai/src/` on branch `wasm` at
commit `0d7442f`.

## Conventions

- "Reachable from HaiClient?" means: can a `pub async fn` on
  `HaiClient` (the wasm wrapper's primary surface) eventually hit this
  line in production? "No (test)" = `#[cfg(test)]` or `#[test]` only.
- Gating strategy classes:
  - **cfg(not(wasm32))** — wrap the module / fn with
    `#[cfg(not(target_arch = "wasm32"))]`. Native build keeps the code;
    wasm build never sees it.
  - **trait split** — extract a small trait so a `_native` impl uses
    the offending API and a `_wasm` impl uses a wasm-portable
    equivalent. See PRD §4.6 (`HaiTransport`, `WebSocketTransport`).
  - **shared helper** — pull pure logic out of a target-specific
    module into a shared sibling.
  - **feature gate** — gate behind a Cargo feature absent from `wasm`.

## tokio + tokio_tungstenite + tempfile + std::fs / tokio::fs

### `rust/haiai/src/client.rs` (2,949 lines)

| Line | Symbol | Reachable from HaiClient? | Gating strategy |
|------|--------|---------------------------|-----------------|
| 11   | `use tokio_tungstenite::tungstenite::Message;` | Yes (WS) | **trait split**: extract `WebSocketTransport` (Task 014). Native impl keeps tungstenite; wasm impl uses `web_sys::WebSocket`. |
| 12   | `use tokio_tungstenite::{connect_async, tungstenite};` | Yes (WS) | Same — Task 014. |
| 1661 | `tokio::time::sleep(options.poll_interval).await;` (poll loop) | Yes (a2a polling) | **shared helper**: hoist into `backoff` module (Task 015) using `gloo-timers::future::sleep` on wasm. |
| 1733 | `let task = tokio::spawn(async move { … });` (SSE reader task) | Yes (SSE) | **trait split** (Task 013/019): SSE reader lives in `sse_native.rs` (tokio spawn) and `sse_wasm.rs` (`wasm_bindgen_futures::spawn_local`). |
| 1737 | `tokio::select! { _ = &mut shutdown_rx … next_chunk = stream.next() … }` (SSE select) | Yes (SSE) | Same — Task 013/019. The wasm impl uses an async stream from `Response::body()` ReadableStream. |
| 1783 | `let task = tokio::spawn(async move { … });` (WS reader task) | Yes (WS) | Same as 1733 but for WS (Task 014/018). |
| 1785 | `tokio::select! { … }` (WS select) | Yes (WS) | Same — Task 014/018. |
| 1895 | `tokio::time::sleep(delay).await;` (SSE reconnect) | Yes | **shared helper** (Task 015) — backoff abstraction. |
| 1931 | `tokio::time::sleep(delay).await;` (WS reconnect) | Yes | Same — Task 015. |
| 1965 | `tokio::time::sleep(delay).await;` (benchmark-job reconnect) | Yes | Same — Task 015. |
| 2004 | `tokio::time::sleep(delay).await;` (benchmark-job reconnect outer) | Yes | Same — Task 015. |
| 2374 | `tempfile::Builder::new()` | No (test) | `#[cfg(test)]` already; no action. |
| 2381 | `std::fs::create_dir_all(&key_dir)` | No (test) | `#[cfg(test)]` already; no action. |
| 2382 | `std::fs::create_dir_all(&data_dir)` | No (test) | `#[cfg(test)]` already; no action. |
| 2535 | `std::fs::read_to_string(&fixture_path)` | No (test) | `#[cfg(test)]` already; no action. |
| 2933 | `std::fs::read_to_string(&fixture_path)` | No (test) | `#[cfg(test)]` already; no action. |

### `rust/haiai/src/config.rs` (741 lines)

| Line | Symbol | Reachable from HaiClient? | Gating strategy |
|------|--------|---------------------------|-----------------|
| 2    | `use std::fs;` | Yes (load_config) | **cfg(not(wasm32))** the loader; add `config_browser.rs` (Task 016) with in-memory JSON deserializer. |
| 343-694 | `tempfile::tempdir()` + `std::fs` ×11 | No (test) | `#[cfg(test)]` already; no action. |

### `rust/haiai/src/document_store.rs` (285 lines)

| Line | Symbol | Reachable from HaiClient? | Gating strategy |
|------|--------|---------------------------|-----------------|
| 89, 90 | `tempfile::TempDir`, `tempfile::tempdir()` | No (test) | `#[cfg(test)]` already; no action. |
| (whole module) | `tokio::fs` storage backend | Yes (when storage enabled) | **cfg(not(wasm32))** entire `document_store.rs` module. Wasm `JacsWasmProvider` (Task 017) provides an in-memory document store or no-op. |

### `rust/haiai/src/email.rs` (gated by `feature = "jacs-crate"`)

| Line | Symbol | Reachable from HaiClient? | Gating strategy |
|------|--------|---------------------------|-----------------|
| 392  | `tempfile::tempdir()` (`sign_email` helper) | Yes (sign_signed_email path) | **inherit feature gate**: this whole module is already `#[cfg(feature = "jacs-crate")]`, which is mutually exclusive with `wasm` (Task 005 compile_error). No wasm action; verify by Task 010 that the wasm build does not try to compile this module. |
| 734-736 | `tempfile::TempDir`, `tempfile::tempdir()` | No (test) | `#[cfg(test)]` already. |

### `rust/haiai/src/jacs_local.rs` (gated by `feature = "jacs-crate"`)

| Line | Symbol | Reachable from HaiClient? | Gating strategy |
|------|--------|---------------------------|-----------------|
| 264, 292, 304, 315, 356, 443, 477, 1035, 1366, 1408, 1415 | `std::fs::*` | Yes (LocalJacsProvider) | **inherit feature gate**: module is `#[cfg(feature = "jacs-crate")]`, absent on wasm. Replacement is `jacs_wasm.rs::JacsWasmProvider` (Task 017). No source change here. |

### `rust/haiai/src/jacs.rs`

| Line | Symbol | Reachable from HaiClient? | Gating strategy |
|------|--------|---------------------------|-----------------|
| 810, 830 | `std::fs::read("MEMORY.md" / "SOUL.md")` (default impl in trait) | Yes (default trait impls used by providers) | **cfg(not(wasm32))**: gate these default impls and add a wasm fallback returning `None` / empty. Tracked under Task 009. |

### `rust/haiai/src/jacs_remote.rs` (2,782 lines)

| Line | Symbol | Reachable from HaiClient? | Gating strategy |
|------|--------|---------------------------|-----------------|
| 737, 755 | `std::fs::read(path)` (file-based remote-attach helpers) | Yes (`store_image_file` / `store_text_file`) | **cfg(not(wasm32))** entire `store_image_file`/`store_text_file` code paths. The wasm build's `JacsRemoteProvider`-equivalent never exposes file-based attach. Tracked under Task 009. |
| 1139 | `jacs_media::extract_signature(bytes, false)` | Yes (image pre-flight) | **feature gate**: introduce `media` feature in `default`, exclude from `wasm`. The wasm build doesn't expose image signing in V1 (PRD §4.8 — attachments are base64-in-JSON only). |
| 1675-2411 | `tempfile::tempdir()`, `std::fs::write/read/read_to_string` | No (test) | `#[cfg(test)]` already; no action. |

### `rust/haiai/src/self_knowledge.rs`

| Line | Symbol | Reachable from HaiClient? | Gating strategy |
|------|--------|---------------------------|-----------------|
| 16 | `use bm25::{...}` | Yes (search-index helpers) | **cfg(not(wasm32))** the entire `self_knowledge.rs` module. `bm25` pulls a search runtime that is large and not exercised by any wasm path (V1 has no self-knowledge query surface). Tracked under Task 009. |

### `rust/haiai/src/validation.rs`

| Line | Symbol | Reachable from HaiClient? | Gating strategy |
|------|--------|---------------------------|-----------------|
| 8, 9 | `use html5ever::tendril::StrTendril;` + tokenizer | Yes (HTML validation in send_signed_email path) | **cfg(not(wasm32))** the html5ever-using `validate_html` function, OR provide a minimal wasm-safe stub (no-op pass-through). `html5ever` itself usually compiles on wasm32, but a transitive `mio`/`tokio` pull would block. Verify in Task 009; default to cfg-gate. |

### `rust/haiai/src/mime.rs`

| Line | Symbol | Reachable from HaiClient? | Gating strategy |
|------|--------|---------------------------|-----------------|
| 360 | `use mail_parser::{MessageParser, MimeHeaders as _};` (test only) | No (test) | `#[cfg(test)]` already; `mail-parser` is a `dev-dependency`. `wasm-pack test` runs dev-deps — Task 010 must verify mail-parser compiles on wasm32 or gate the test module on `cfg(not(target_arch = "wasm32"))`. |

## Cargo dependency splits

| Dep | Currently | Wasm strategy | Lands in |
|-----|-----------|---------------|----------|
| `jacs = "=0.10.2"` | optional, default feature `jacs-crate` | replaced by `jacs-core` + `jacs-wasm` under `wasm` feature; mutual exclusion already in place | Task 008 |
| `jacs-media = "=0.10.2"` | unconditional | introduce `media` feature in `default`, exclude from `wasm`; cfg-gate `jacs_remote.rs` image-signing call sites | Task 009 |
| `image = "0.25"` (dev-dep only, png+jpeg features) | test fixtures | safe as dev-dep; wasm-pack test must skip those tests (`cfg(not(target_arch = "wasm32"))`) | Task 010 |
| `bm25 = "2.3"` | unconditional, used only in `self_knowledge.rs` | cfg-gate the module on `not(target_arch = "wasm32")` so bm25 is never pulled into the wasm build | Task 009 |
| `html5ever = "0.35"` | unconditional, used only in `validation.rs` | first try a plain cfg-gate of `validation.rs` on wasm; if a wasm-safe stub is needed, provide minimal pass-through | Task 009 |
| `mail-parser = "0.11"` | dev-dep, used in tests + `mime.rs::tests` | already test-gated; reaffirm in Task 010 | Task 010 |
| `tempfile = "3"` | unconditional, used only in tests | move under `[dev-dependencies]` if it isn't already; tests are `cfg(test)` so wasm build doesn't see them | Task 010 |
| `tokio-tungstenite = "0.28"` | unconditional, used in `client.rs` WS path | move into `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`; WS path moves behind `WebSocketTransport` trait | Task 010 / Task 014 |
| `tokio` (workspace, full feature set) | unconditional | the workspace pulls `rt-multi-thread`, `io-std`, `io-util`, `process`, `sync`, `macros`. Wasm32 must see only `macros + sync`. Target-split in `rust/haiai/Cargo.toml` mirroring the reqwest split (Task 006 pattern) | Task 010 |
| `reqwest` | already split (Task 006) | done | n/a |

## Tasks unblocked by this audit

- **Task 008** — wasm-only optional deps wiring (`wasm-bindgen-futures`, `web-sys`, `js-sys`, `gloo-timers`, `jacs-core`, `jacs-wasm`). Knows it must not introduce anything from the forbidden-deps list.
- **Task 009** — cfg-gate `config.rs::load_config`, `document_store.rs`, `jacs.rs::MEMORY.md/SOUL.md` defaults, `jacs_remote.rs` image-signing paths, `self_knowledge.rs`, `validation.rs`. Adds `media` feature flag for `jacs-media`.
- **Task 010** — target-split `tokio-tungstenite`, `tempfile`, `tokio` (workspace) in `rust/haiai/Cargo.toml`; cfg-gate `tokio::time::sleep` call sites pending Task 015's `backoff` helper.
- **Task 014** — extract `WebSocketTransport` trait (`async fn next_message`, `send_message`, `close`); native impl wraps tungstenite; wasm impl uses `web_sys::WebSocket`.
- **Task 015** — hoist `tokio::time::sleep(Duration::from_millis(…))` into a target-agnostic `backoff_sleep(duration)` helper that delegates to `gloo-timers::future::sleep` on wasm.
- **Task 019** — wasm SSE reader using `wasm_bindgen_futures::spawn_local` + `Response::body()` ReadableStream.

## Notes / open items

- `html5ever` wasm32-compatibility is not 100% guaranteed (transitive deps
  shift across releases). Task 009 should attempt the cfg-gate-only path
  first; if html5ever turns out to compile cleanly on wasm32, the gate
  can be relaxed in a follow-up. The conservative default is gate.
- `bm25` is not exercised by any HaiClient HTTP method today, so the
  `not(target_arch = "wasm32")` gate is non-controversial.
- `tempfile` already lives only in test/dev paths in `rust/haiai/src/`,
  but it is currently declared in `[dependencies]` (top-level), not
  `[dev-dependencies]`. Task 010 moves it under `[dev-dependencies]` to
  keep the wasm production tree clean.
