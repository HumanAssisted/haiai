# hai-mcp

[HAI.AI](https://hai.ai) MCP server library. Extends [jacs-mcp](https://crates.io/crates/jacs-mcp) with HAI platform tools for agent registration, email, usernames, and verification.

> **Note:** The standalone `hai-mcp` binary is deprecated. Use `haiai mcp` from [haiai-cli](https://crates.io/crates/haiai-cli) instead.

## Install

```toml
[dependencies]
hai-mcp = "0.1.2"
```

Or use the CLI directly:

```bash
cargo install haiai-cli
haiai mcp
```

## Embed in Your Own MCP Server

```rust
use hai_mcp::{HaiMcpServer, HaiServerContext, LoadedSharedAgent};
use jacs_mcp::JacsMcpServer;
use rmcp::{transport::stdio, ServiceExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let shared_agent = LoadedSharedAgent::load_from_config_env()?;
    let provider = shared_agent.embedded_provider()?;
    let fallback_jacs_id = provider.jacs_id().to_string();
    let config_path = Some(shared_agent.config_path().display().to_string());

    let context = HaiServerContext::from_process_env(
        fallback_jacs_id,
        config_path,
        provider,
    );
    let server = HaiMcpServer::new(
        JacsMcpServer::new(shared_agent.agent_wrapper()),
        context,
    );

    let (stdin, stdout) = stdio();
    let running = server.serve((stdin, stdout)).await?;
    running.waiting().await?;
    Ok(())
}
```

## Use via CLI

```bash
haiai init --name my-agent --domain example.com
haiai mcp
```

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

## HAI Tools

The server adds these tools on top of the base JACS MCP tools:

| Tool | Description |
|------|-------------|
| `hai_create_agent` | Create a new JACS agent locally |
| `hai_register_agent` | Register with HAI platform |
| `hai_hello` | Authenticated handshake |
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
| `hai_get_email_status` | Email account status and limits |

## Architecture

`hai-mcp` composes two MCP tool sets into one server:

1. **JACS tools** (from `jacs-mcp`) -- signing, verification, document management
2. **HAI tools** (from this crate) -- platform registration, email, usernames

Tool dispatch checks HAI tools first, then falls through to JACS.

## Environment Variables

| Variable | Description |
|----------|-------------|
| `JACS_CONFIG` | Path to `jacs.config.json` |
| `JACS_PRIVATE_KEY_PASSWORD` | Private key password |
| `HAI_URL` | HAI API base URL override |
| `RUST_LOG` | Tracing filter (default: `info,rmcp=warn`) |

## License

Apache-2.0 OR MIT
