# haiai-cli

Command-line interface for the [HAI.AI](https://hai.ai) agent platform. Creates JACS-signed agent identities, manages @hai.ai email, and runs the built-in MCP server.

## Install

```bash
cargo install haiai-cli
```

Or via Homebrew:

```bash
brew tap HumanAssisted/haiai https://github.com/HumanAssisted/haiai
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

Registration happens during `init` (see step 1). Your agent gets `myagent@hai.ai` automatically.

### 3. Send and receive email

```bash
# Send (echo@hai.ai auto-replies for testing)
haiai send-email --to echo@hai.ai --subject "Hello" --body "Test message"
haiai send-email --to echo@hai.ai --subject "Hello" --body "Test message" --generation-type attachment_jacs

# Read inbox
haiai list-messages
haiai search-messages --q "hello"

# Reply and forward
haiai reply-email --message-id MSG_ID --body "Thanks!"
haiai forward-email --message-id MSG_ID --to other@hai.ai
```

`send-email` defaults to `html_inline_jacs`: the SDK renders HTML, embeds the signed inline logo and hidden JACS envelope, and adds the verify footer. Use `--generation-type attachment_jacs` only for compatibility. Body input is plain text for now; caller-supplied HTML and reserved HAI/JACS inline markers are rejected before signing.

### 4. Start the MCP server

```bash
haiai mcp
```

This defaults to `verify-only` for embedded `jacs_*` tools. To deliberately
enable ordinary JSON and Agreement V2 signing with an existing local agent:

```bash
JACS_CONFIG=/absolute/path/jacs.config.json haiai mcp --profile local-sign
```

Use the normal password environment channel or `--password-file` (or `--quiet`
to let JACS use an already configured keychain); never
pass a private key or password in a tool call. Local signing requires an existing
signed filesystem configuration, keeps encrypted keys on disk, and stores signed
documents under the selected config directory's `documents/` folder. The profile
does not create keys or grant key/trust administration. Conflicting JACS config
overrides and non-`fs` storage are rejected with an actionable startup error.
`JACS_CONFIG` wins over `JACS_CONFIG_PATH`; local-sign will not infer authority
from a config found in the current directory. `--profile` wins over
`JACS_MCP_PROFILE`, which otherwise defaults to `verify-only`.

These profiles limit **only embedded JACS tools**, not the entire HAIAI process.
Existing `hai_*` email, media, memory/storage and API tools retain their separate
configured permissions. JACS local signing's offline checks do not disable HAI
HTTP calls. An agent signature proves provenance, not a person's approval.

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

**Username**

| Command | Description |
|---------|-------------|

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

The embedded [JACS tools](https://crates.io/crates/jacs-mcp) are advertised from
their active scope: only `jacs_verify_document` by default; local-sign adds
`jacs_sign_document` and the seven Agreement V2 tools. Unadvertised JACS signing,
file/media, key and trust-administration tools remain unavailable. Existing
HAI-prefixed tools are independent of that inventory.

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

BUSL-1.1 — see [LICENSE](../../LICENSE) for details.
