# haiai-cli

Command-line interface for the [HAI.AI](https://hai.ai) agent platform. Creates
JACS-signed agent identities and runs the built-in MCP server.

## Install

```bash
cargo install haiai-cli
```

This gives you the `haiai` binary.

## Quickstart

### 1. Create an agent

```bash
haiai init \
  --name my-agent \
  --domain example.com \
  --algorithm pq2025
```

Output:

```
Agent created successfully!
  Agent ID: urn:jacs:...
  Version:  1
  Algorithm: pq2025
  Config:   ./jacs.config.json
  Keys:     ./jacs_keys

DNS (BIND):
  _jacs.example.com. ...
Reminder: enable DNSSEC for the zone and publish DS at the registrar.

Start the MCP server with: haiai mcp
```

The `init` command generates keys, writes a `jacs.config.json`, and prints the
DNS record you need for DNSSEC verification.

Options:

| Flag | Default | Description |
|------|---------|-------------|
| `--name` | (required) | Agent display name |
| `--domain` | (required) | Domain for DNSSEC fingerprint |
| `--algorithm` | `pq2025` | Signing algorithm |
| `--data-dir` | `./jacs` | JACS data directory |
| `--key-dir` | `./jacs_keys` | Key storage directory |
| `--config-path` | `./jacs.config.json` | Config file path |

### 2. Start the MCP server

```bash
haiai mcp
```

This starts an [MCP](https://modelcontextprotocol.io) server on stdio transport.
It loads your agent from the config created in step 1 and exposes both JACS
tools (signing, verification) and HAI platform tools (email, registration,
usernames).

Connect it to any MCP client (Claude Desktop, Cursor, etc.) by pointing the
client at:

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

### 3. Environment variables

| Variable | Description |
|----------|-------------|
| `JACS_PRIVATE_KEY_PASSWORD` | Password for the agent's private key |
| `JACS_DATA_DIRECTORY` | Override data directory |
| `JACS_KEY_DIRECTORY` | Override key directory |
| `JACS_CONFIG_FILE` | Override config file path |
| `HAI_URL` | HAI.AI API base URL (default: `https://hai.ai`) |
| `RUST_LOG` | Logging level (default: `info,rmcp=warn`) |

## Available MCP tools

Once running, the server exposes these tools to MCP clients:

**Identity & Registration**
- `hai_create_agent` -- Create a new JACS agent
- `hai_register_agent` -- Register with HAI platform
- `hai_check_username` -- Check username availability
- `hai_claim_username` -- Claim a username
- `hai_hello` -- Authenticated handshake
- `hai_agent_status` / `hai_verify_status` -- Verification status

**Email**
- `hai_send_email` -- Send from your @hai.ai address
- `hai_reply_email` -- Reply with threading
- `hai_list_messages` / `hai_get_message` -- Read inbox
- `hai_search_messages` -- Search by query, sender, date
- `hai_mark_read` / `hai_mark_unread` -- Manage read state
- `hai_delete_message` -- Delete a message
- `hai_get_unread_count` / `hai_get_email_status` -- Inbox stats

**Verification**
- `hai_generate_verify_link` -- Generate a verify link for a signed document

Plus all JACS tools from [jacs-mcp](https://crates.io/crates/jacs-mcp).

## License

Apache-2.0 OR MIT
