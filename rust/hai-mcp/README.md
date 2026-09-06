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

The server adds these tools to the **active**, not the compiled, JACS inventory.
`JacsMcpServer::new` preserves JACS's verify-only default even when a HAI provider
has loaded a private key. Embedding a JACS server never broadens its granted scope.

For explicitly configured local JSON/Agreement signing, use:

```bash
JACS_CONFIG=/absolute/path/jacs.config.json haiai mcp --profile local-sign
```

The CLI delegates authority checks to JACS's `local_signing_from_config`: an
existing signed config, encrypted key and filesystem storage are required. It
rejects conflicting effective JACS environment overrides rather than loading a
different identity for HAI's provider. `JACS_CONFIG_PATH` is the existing fallback;
cwd-only config discovery does not authorize local-sign. Password handling uses
the CLI's existing environment or `--password-file` channel (`--quiet` delegates
lookup to JACS, including an already configured keychain).

Local-sign exposes exactly nine JACS tools: explicit-key document verification,
ordinary JSON signing, and seven Agreement V2 create/apply/sign/inspect/branch
tools. JSON input remains nested ordinary content; created documents persist
under the config directory's `documents/`. File/media, raw signing and key/trust
administration are not part of the embedded JACS scope. Signatures prove agent
provenance, not per-action human approval.

This does **not** restrict the whole HAIAI process to verification or offline
operation: retained `hai_*` tools have separate configured authority for HAI API
requests, email, media and storage. In particular, existing HAI-prefixed media
tools remain available; they are not additional `jacs_*` profile permissions.

| Tool | Description |
|------|-------------|
| `hai_create_agent` | Create a new JACS agent locally |
| `hai_register_agent` | Register with HAI platform |
| `hai_hello` | Authenticated handshake |
| `hai_agent_status` | Agent verification status |
| `hai_verify_status` | Verification status lookup |
| `hai_generate_verify_link` | Generate verify link for signed doc |
| `hai_send_email` | Send from @hai.ai address (`generation_type` defaults to `html_inline_jacs`; use `attachment_jacs` for compatibility) |
| `hai_reply_email` | Reply with threading |
| `hai_list_messages` | List inbox/outbox |
| `hai_get_message` | Get single message |
| `hai_search_messages` | Search messages |
| `hai_delete_message` | Delete a message |
| `hai_mark_read` | Mark read |
| `hai_mark_unread` | Mark unread |
| `hai_get_unread_count` | Unread count |
| `hai_get_email_status` | Email account status and limits |

`hai_send_email` accepts plain-text body input. In the default `html_inline_jacs` mode the SDK renders the HTML, embeds the signed inline logo and hidden JACS envelope, and adds the verify footer. Caller-supplied HTML and reserved HAI/JACS inline markers are rejected before signing.

## Architecture

`hai-mcp` composes two MCP tool sets into one server:

1. **Active JACS tools** (from `jacs-mcp`) -- verify-only by default; explicitly scoped JSON/Agreement signing
2. **HAI tools** (from this crate) -- platform registration, email, usernames

Tool dispatch checks HAI tools first, then falls through to JACS.

## Environment Variables

| Variable | Description |
|----------|-------------|
| `JACS_CONFIG` | Path to `jacs.config.json` |
| `JACS_CONFIG_PATH` | Existing fallback when `JACS_CONFIG` is unset or empty |
| `JACS_MCP_PROFILE` | JACS scope selection; `--profile` wins; default `verify-only` |
| `JACS_PRIVATE_KEY_PASSWORD` | Private key password |
| `HAI_URL` | HAI API base URL override |
| `RUST_LOG` | Tracing filter (default: `info,rmcp=warn`) |

## License

BUSL-1.1 — see [LICENSE](../../LICENSE) for details.
