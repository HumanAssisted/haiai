# hai-mcp

MCP server crate for HAISDK.

This crate exposes HAI platform operations as `hai_*` MCP tools and proxies
the full `jacs_*` tool surface from `jacs-mcp`.

`hai-mcp` is intentionally local-only: it runs over stdio, rejects runtime
transport/listener arguments, and launches `jacs-mcp` locally over stdio as
well.

## Tool surface

- `jacs_*` tools: proxied from a `jacs-mcp` subprocess
- `hai_*` tools:
  - `hai_check_username`
  - `hai_hello`
  - `hai_verify_status`
  - `hai_claim_username`
  - `hai_create_agent`
  - `hai_register_agent`
  - `hai_generate_verify_link`

## Bridge configuration

`hai-mcp` discovers `jacs-mcp` in this order:

1. `JACS_MCP_BIN` (+ optional `JACS_MCP_CWD`)
2. `jacs-mcp` on `PATH`
3. `cargo run --manifest-path ~/personal/JACS/jacs-mcp/Cargo.toml` (if present)

For authenticated `hai_*` tools, set `JACS_CONFIG` (or pass `config_path` args)
so `LocalJacsProvider` can load your agent.

`hai-mcp` enforces local stdio mode and ignores `JACS_MCP_ARGS` runtime overrides.

For email tools, `hai_register_agent` and `hai_claim_username` seed the
in-process cache of HAI `agent_id` / claimed email so later mailbox calls can
run without repeating that identity state. Stateless callers may also pass
`agent_id` (and for send/reply, `email`) directly to the email tools.
