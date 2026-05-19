# haiai-wasm

Browser-side wasm-bindgen wrapper around [`haiai`](../haiai) (compiled
with the `wasm` feature). This crate is the Rust half of the published
[`@haiai/wasm`](https://www.npmjs.com/package/@haiai/wasm) npm package.

## Build

```bash
wasm-pack build --target web --release rust/haiai-wasm
```

## Native

On any non-wasm32 target the crate compiles to a no-op stub so
workspace-wide `cargo check --workspace` stays green. Real exports
(`BrowserAgentHandle`, `EventStreamHandle`, lifecycle / crypto /
HTTP / event-stream methods) compile only on `wasm32-unknown-unknown`.

## Status

Skeleton + `initHaiaiWasm` / `version` / `about` only. The full API
surface lands across Tasks 021–030 of
[HAIAI_WASM_PRD.md](../../../docs/jacs/HAIAI_WASM_PRD.md).
</content>
