# haiai-cli

Command-line interface for local JACS identity, signing, and verification, plus
admitted [HAI.AI](https://hai.ai) integrations and the built-in MCP server.

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

### 1. Create, sign, and verify locally

Use a new directory and set your own `JACS_PRIVATE_KEY_PASSWORD` as shown in the
[local quickstart](../../README.md#local-quickstart), or enter it when prompted.

```bash
haiai init --name myagent --register=false
printf 'Hello from my local agent.\n' > hello.txt
haiai sign-text hello.txt
haiai verify-text hello.txt --strict
```

`init` generates encrypted keys and writes `jacs.config.json`. Signing appends
an inline JACS signature and saves `hello.txt.bak`; verification should report
all signatures valid and exit 0. These operations need no HAI account or API.
An optional `--domain example.com` configures DNS verification information.

Options:

| Flag | Default | Description |
|------|---------|-------------|
| `--name` | (required) | Agent display name |
| `--domain` | (unset) | Optional domain for DNS verification |
| `--register` | `true` | Pass `--register=false` for a local identity only |
| `--key` | (unset) | One-use reservation key, required when registering |
| `--algorithm` | `pq2025` | Signing algorithm |
| `--data-dir` | `./jacs` | JACS data directory |
| `--key-dir` | `./jacs_keys` | Key storage directory |
| `--config-path` | `./jacs.config.json` | Config file path |

The registration key is validated before creation when `--register=true`.
For an admitted new identity, use `init --name RESERVED_NAME --key KEY` in a
separate empty directory. To register or retry registration of an existing
identity, use MCP `hai_register_agent` with `registration_key` and `config_path`;
the reservation name must match the identity. There is no standalone
`haiai register` command. See
[admitted registration](../../README.md#admitted-registration-and-email).

### 2. Start the MCP server

Run from the identity directory with its password available:

```bash
haiai mcp
```

Connect it to an MCP client:

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

### 3. Use email after admission and activation

Developer signup remains hidden. First read the shared
[capability boundaries](../../README.md#capability-boundaries) and
[v0.4.1 API compatibility note](../../README.md#platform-compatibility).
Inspect the server-returned status/address with `haiai status` and
`haiai email-status`; `init`'s guessed `name@hai.ai` success line does not
establish an active mailbox. Only send after email status is `active`, within
server limits, to a recipient you are authorized to contact.

```bash
# Send
haiai send-email --to APPROVED_RECIPIENT --subject "Hello" --body "Test message"
haiai send-email --to APPROVED_RECIPIENT --subject "Hello" --body "Test message" --generation-type attachment_jacs

# Read inbox
haiai list-messages
haiai search-messages --q "hello"

# Reply and forward
haiai reply-email --message-id MSG_ID --body "Thanks!"
haiai forward-email --message-id MSG_ID --to APPROVED_RECIPIENT
```

`send-email` defaults to `html_inline_jacs`: the SDK renders HTML, embeds the signed inline logo and hidden JACS envelope, and adds the verify footer. Use `--generation-type attachment_jacs` only for compatibility. Body input is plain text for now; caller-supplied HTML and reserved HAI/JACS inline markers are rejected before signing.

## Command overview

Use `haiai --help` and `haiai <command> --help` for the complete parser reference.

**Agent Management**

| Command | Description |
|---------|-------------|
| `init` | Create keys/config; defaults to registration, use `--register=false` locally |
| `hello` | Authenticated handshake with HAI |
| `status` | Check registration and verification status |
| `update` | Update agent metadata and re-sign |
| `rotate` | Rotate cryptographic keys |
| `migrate` | Migrate legacy agent to current schema |
| `doctor` | Diagnose agent health, storage, configuration |

**Local Signing and Verification**

| Command | Description |
|---------|-------------|
| `sign-text` | Sign text/Markdown in place, keeping a backup by default |
| `verify-text` | Verify text signatures locally; `--strict` rejects unsigned input |
| `sign-image` | Sign a PNG/JPEG/WebP image with JACS |
| `verify-image` | Verify an image's JACS signature locally |

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

**Benchmarking (admin/lab)**

| Command | Description |
|---------|-------------|
| `benchmark` | Run an admitted benchmark workflow; not developer enrollment |

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

**Local files:** `hai_sign_text`, `hai_verify_text`, `hai_sign_image`, `hai_verify_image`.

`hai_conflict_*` operates on local JACS conflict documents. These tools do not
enroll an advocate or mediator into a hosted Agreement; see the shared
[Agreement limits](../../README.md#capability-boundaries).

JACS tools from [jacs-mcp](https://crates.io/crates/jacs-mcp) are advertised only
for the active profile. MCP normally requests `local-sign`; if JACS denies it,
the server logs `event=mcp_local_signing_denied` and falls back to `verify-only`.

## Environment Variables

| Variable | Description |
|----------|-------------|
| `JACS_PRIVATE_KEY_PASSWORD` | Password for the agent's private key |
| `JACS_DATA_DIRECTORY` | Override data directory |
| `JACS_KEY_DIRECTORY` | Override key directory |
| `JACS_CONFIG_FILE` | Override config file path |
| `HAI_URL` | HAI.AI API base URL (default: `https://hai.ai`) |
| `HAI_REQUEST_AUTH_AUDIENCE` | API ingress audience for ordinary requests and remote records (default when absent: `hai.ai`) |
| `RUST_LOG` | Logging level (default: `info,rmcp=warn`) |

Set `HAI_REQUEST_AUTH_AUDIENCE` to the API's configured ingress audience; it is
independent of `HAI_URL`. Blank, invalid UTF-8, or values over 256 UTF-8 bytes
refuse startup. `haiai mcp` pins the value at startup, including for remote
document storage; tool arguments cannot override it.

## Global Flags

| Flag | Description |
|------|-------------|
| `-q` / `--quiet` | Don't prompt for password; require `JACS_PRIVATE_KEY_PASSWORD` |
| `--storage` | Document storage backend (`fs`, `rusqlite`, `sqlite`) |
| `--storage-env` | Read storage backend from an environment variable |

## License

BUSL-1.1 — see [LICENSE](../../LICENSE) for details.
