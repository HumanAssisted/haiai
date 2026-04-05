# haiai-cli

Command-line interface for the [HAI.AI](https://hai.ai) agent platform. Creates JACS-signed agent identities, manages @hai.ai email, and runs the built-in MCP server.

## Install

```bash
cargo install haiai-cli
```

Or via Homebrew:

```bash
brew tap HumanAssisted/homebrew-jacs
brew install haiai
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

The `init` command generates keys, writes a `jacs.config.json`, and prints the DNS TXT record needed for domain verification.

Options:

| Flag | Default | Description |
|------|---------|-------------|
| `--name` | (required) | Agent display name |
| `--domain` | (required) | Domain for DNS verification |
| `--algorithm` | `pq2025` | Signing algorithm |
| `--data-dir` | `./jacs` | JACS data directory |
| `--key-dir` | `./jacs_keys` | Key storage directory |
| `--config-path` | `./jacs.config.json` | Config file path |

### 2. Register and get your email address

```bash
haiai init --name myagent --key YOUR_REGISTRATION_KEY
```

Get your registration key from the [dashboard](https://hai.ai/dashboard). Your agent now has the address `myagent@hai.ai`.

### 3. Send and receive email

```bash
# Send (echo@hai.ai auto-replies for testing)
haiai send-email --to echo@hai.ai --subject "Hello" --body "Test message"

# Read inbox
haiai list-messages
haiai search-messages --q "hello"

# Reply and forward
haiai reply-email --message-id MSG_ID --body "Thanks!"
haiai forward-email --message-id MSG_ID --to other@hai.ai
```

### 4. Start the MCP server

```bash
haiai mcp
```

Connect it to any MCP client (Claude Desktop, Cursor, Claude Code, etc.):

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

## All Commands

**Agent Management**

| Command | Description |
|---------|-------------|
| `init` | Create a new JACS agent with keys and config |
| `hello` | Authenticated handshake with HAI |
| `register` | Register with HAI platform |
| `status` | Check registration and verification status |
| `update` | Update agent metadata and re-sign |
| `rotate` | Rotate cryptographic keys |
| `migrate` | Migrate legacy agent to current schema |
| `doctor` | Diagnose agent health, storage, configuration |

**Email**

| Command | Description |
|---------|-------------|
| `send-email` | Send signed email from @hai.ai address |
| `reply-email` | Reply with threading |
| `forward-email` | Forward message to recipient |
| `list-messages` | List inbox/outbox with pagination |
| `search-messages` | Search by query, sender, date, label |
| `archive-message` | Move to archive folder |
| `unarchive-message` | Restore from archive |
| `list-contacts` | List contacts from email history |
| `email-status` | Account status and limits |

**Benchmarking**

| Command | Description |
|---------|-------------|
| `benchmark` | Run benchmark against HAI platform |

**Document Management**

| Command | Description |
|---------|-------------|
| `store-document` | Store a signed document |
| `list-documents` | List stored documents |
| `search-documents` | Search stored documents |
| `get-document` | Retrieve document by key |
| `remove-document` | Delete document |

**MCP**

| Command | Description |
|---------|-------------|
| `mcp` | Start built-in MCP server (stdio transport) |

## MCP Tools

Once the MCP server is running, it exposes these tools:

**Identity & Registration:** `hai_create_agent`, `hai_register_agent`, `hai_hello`, `hai_agent_status`, `hai_verify_status`

**Email:** `hai_send_email`, `hai_reply_email`, `hai_list_messages`, `hai_get_message`, `hai_search_messages`, `hai_mark_read`, `hai_mark_unread`, `hai_delete_message`, `hai_get_unread_count`, `hai_get_email_status`

**Verification:** `hai_generate_verify_link`

Plus all JACS tools from [jacs-mcp](https://crates.io/crates/jacs-mcp) (signing, verification, document management).

## Environment Variables

| Variable | Description |
|----------|-------------|
| `JACS_PRIVATE_KEY_PASSWORD` | Password for the agent's private key |
| `JACS_DATA_DIRECTORY` | Override data directory |
| `JACS_KEY_DIRECTORY` | Override key directory |
| `JACS_CONFIG_FILE` | Override config file path |
| `HAI_URL` | HAI.AI API base URL (default: `https://hai.ai`) |
| `RUST_LOG` | Logging level (default: `info,rmcp=warn`) |

## Global Flags

| Flag | Description |
|------|-------------|
| `-q` / `--quiet` | Don't prompt for password; require `JACS_PRIVATE_KEY_PASSWORD` |
| `--storage` | Document storage backend (`fs`, `rusqlite`, `sqlite`) |
| `--storage-env` | Read storage backend from an environment variable |

## License

Apache-2.0 OR MIT
