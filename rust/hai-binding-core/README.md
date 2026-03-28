# hai-binding-core

Shared JSON-in/JSON-out core for [HAI.AI](https://hai.ai) SDK FFI bindings. Wraps [`haiai::HaiClient`](https://crates.io/crates/haiai) with a string-based interface suitable for crossing FFI boundaries.

## Purpose

All HAI SDK language bindings (Python via PyO3, Node via napi-rs, Go via CGo) depend on this crate. It provides `HaiClientWrapper` -- every method accepts and returns JSON strings, so building a new language binding is:

1. Depend on `hai-binding-core`
2. Wrap `HaiClientWrapper` with language-specific error conversion
3. Handle async per your language's model

## Usage

```toml
[dependencies]
hai-binding-core = "0.2.1"
```

```rust
use hai_binding_core::{HaiClientWrapper, HaiBindingError};

// Create from JSON config
let wrapper = HaiClientWrapper::from_config_json(config_json)?;

// All methods return JSON strings
let result: String = wrapper.hello(None).await?;
let messages: String = wrapper.list_messages("{}").await?;
```

## Existing FFI bindings

| Language | Crate | FFI mechanism |
|----------|-------|---------------|
| Python | `haiipy` | PyO3 |
| Node.js | `haiinpm` | napi-rs |
| Go | `haiigo` | CGo cdylib |

## License

Apache-2.0 OR MIT
