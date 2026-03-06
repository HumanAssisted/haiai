# hai-mcp

MCP server crate for HAISDK.

This crate exposes HAI platform operations as `hai_*` MCP tools and embeds the
full canonical `jacs_*` tool surface from `jacs-mcp` in-process.

`hai-mcp` is intentionally local-only: it runs over stdio, rejects runtime
transport/listener arguments, and serves one combined MCP process.

## Tool surface

- `jacs_*` tools: served directly by the embedded `jacs-mcp` Rust library
- `hai_*` tools:
  - `hai_check_username`
  - `hai_hello`
  - `hai_agent_status`
  - `hai_verify_status`
  - `hai_claim_username`
  - `hai_create_agent`
  - `hai_register_agent`
  - `hai_generate_verify_link`
  - `hai_send_email`
  - `hai_list_messages`
  - `hai_get_message`
  - `hai_delete_message`
  - `hai_mark_read`
  - `hai_mark_unread`
  - `hai_search_messages`
  - `hai_get_unread_count`
  - `hai_get_email_status`
  - `hai_reply_email`

## Runtime

`hai-mcp` supports only:

1. stdio server mode
2. `--help`
3. `--version`

Supported environment variables:

1. `JACS_CONFIG` for the local `jacs.config.json`
2. `JACS_PRIVATE_KEY_PASSWORD` for the local encrypted private key
3. `HAI_URL` to override the HAI API base URL
4. `RUST_LOG` for tracing filters

Legacy subprocess variables such as `JACS_MCP_BIN` and `JACS_MCP_ARGS` are not
part of the architecture and are ignored.

For authenticated `hai_*` tools, set `JACS_CONFIG` or pass `config_path` so
`LocalJacsProvider` can load the local agent.

For email tools, `hai_register_agent` and `hai_claim_username` seed the
in-process cache of HAI `agent_id` / claimed email so later mailbox calls can
run without repeating that identity state. Stateless callers may also pass
`agent_id` (and for send/reply, `email`) directly to the email tools.
