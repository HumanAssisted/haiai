# hai-mcp

[HAI.AI](https://hai.ai) MCP server library. Extends
[jacs-mcp](https://crates.io/crates/jacs-mcp) with HAI platform tools for
agent registration, email, usernames, and verification.

> **Note:** The standalone `hai-mcp` binary is deprecated. Use `haiai mcp` from
> [haiai-cli](https://crates.io/crates/haiai-cli) instead.

## Install

Add to your `Cargo.toml`:

```toml
[dependencies]
hai-mcp = "0.1.2"
```

Or use the CLI directly:

```bash
cargo install haiai-cli
haiai mcp
```

## Quickstart -- embed in your own MCP server

```rust
use hai_mcp::{HaiMcpServer, HaiServerContext, LoadedSharedAgent};
use jacs_mcp::JacsMcpServer;
use rmcp::{transport::stdio, ServiceExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load the JACS agent from config/env
    let shared_agent = LoadedSharedAgent::load_from_config_env()?;
    let provider = shared_agent.embedded_provider()?;
    let fallback_jacs_id = provider.jacs_id().to_string();
    let config_path = Some(shared_agent.config_path().display().to_string());

    // Build the context and server
    let context = HaiServerContext::from_process_env(
        fallback_jacs_id,
        config_path,
        provider,
    );
    let server = HaiMcpServer::new(
        JacsMcpServer::new(shared_agent.agent_wrapper()),
        context,
    );

    // Serve on stdio
    let (stdin, stdout) = stdio();
    let running = server.serve((stdin, stdout)).await?;
    running.waiting().await?;
    Ok(())
}
```

## Quickstart -- use via CLI

```bash
# 1. Create an agent
haiai init --name my-agent --domain example.com

# 2. Start the MCP server
haiai mcp
```

Then point your MCP client at it:

```json
{
  "mcpServers": {
    "haiai": {
      "command": "haiai",
      "args": ["mcp"]
    }
  }
}
```

## HAI tools exposed

The server adds these tools on top of the base JACS MCP tools:

| Tool | Description |
|------|-------------|
| `hai_create_agent` | Create a new JACS agent locally |
| `hai_register_agent` | Register with HAI platform |
| `hai_check_username` | Check username availability |
| `hai_claim_username` | Claim a username for an agent |
| `hai_hello` | Authenticated handshake with HAI |
| `hai_agent_status` | Agent verification status |
| `hai_verify_status` | Verification status lookup |
| `hai_generate_verify_link` | Generate verify link for signed doc |
| `hai_send_email` | Send from @hai.ai address |
| `hai_reply_email` | Reply with threading |
| `hai_list_messages` | List inbox/outbox |
| `hai_get_message` | Get single message |
| `hai_search_messages` | Search messages |
| `hai_delete_message` | Delete a message |
| `hai_mark_read` | Mark read |
| `hai_mark_unread` | Mark unread |
| `hai_get_unread_count` | Unread count |
| `hai_get_email_status` | Email account status & limits |

## Architecture

`hai-mcp` composes two MCP tool sets into one server:

1. **JACS tools** (from `jacs-mcp`) -- signing, verification, document management
2. **HAI tools** (from this crate) -- platform registration, email, usernames

Tool dispatch checks HAI tools first, then falls through to JACS. This means
a single MCP server gives clients the full stack: cryptographic identity via
JACS plus HAI platform integration.

## Environment variables

| Variable | Description |
|----------|-------------|
| `JACS_CONFIG` | Path to `jacs.config.json` |
| `JACS_PRIVATE_KEY_PASSWORD` | Private key password |
| `HAI_URL` | HAI API base URL override |
| `RUST_LOG` | Tracing filter (default: `info,rmcp=warn`) |

For email tools, `hai_register_agent` and `hai_claim_username` seed the
in-process cache of HAI `agent_id` / claimed email so later mailbox calls work
without repeating identity state. Stateless callers may also pass `agent_id`
(and for send/reply, `email`) directly.

## License

MIT
