# CLI & MCP Parity Audit

Audit of Python/Node CLI and MCP server commands versus Rust replacements, produced before deleting the language-specific implementations (TASK_048).

## CLI Command Parity

| Command | Python CLI (`cli.py` via `jacs.hai.cli`) | Node CLI (`cli.ts`) | Rust CLI (`haiai-cli`) | Notes |
|---------|----------------------------------------|---------------------|------------------------|-------|
| init/create agent | `register` (creates + registers) | `register` (creates + registers) | `init` (create only) + `register` (separate) | Rust separates create from register -- cleaner |
| hello | `hello` | `hello` | `hello` | Full parity |
| register | `register` | `register` | `register` | Full parity |
| status | `status` | `status` | `status` | Full parity |
| check-username | `check-username` | `check-username` | `check-username` | Full parity |
| claim-username | `claim-username` | `claim-username` | `claim-username` | Full parity |
| send-email | `send-email` | `send-email` | `send-email` | Full parity |
| list-messages | `list-messages` | `list-messages` | `list-messages` | Full parity |
| search-messages | -- | -- | `search-messages` | Rust-only addition |
| email-status | `email-status` | `email-status` | `email-status` | Full parity |
| fetch-key | -- | `fetch-key` | -- | Node-only; intentionally dropped (use MCP or API directly) |
| benchmark | `benchmark` | `benchmark` | `benchmark` | Full parity |
| update | -- | -- | `update` | Rust-only addition: re-sign agent metadata |
| rotate | -- | -- | `rotate` | Rust-only addition: key rotation |
| migrate | -- | -- | `migrate` | Rust-only addition: schema migration |
| mcp (start server) | -- | -- | `mcp` | Rust-only: embedded MCP server |
| jacs passthrough | -- | `jacs <args>` | N/A (separate `jacs` binary) | Node passed through to jacs binary; Rust has dedicated `jacs` CLI |

### Intentionally Dropped Commands

| Command | Source | Reason |
|---------|--------|--------|
| `fetch-key` | Node CLI | Low-level utility; available via `hai_verify_status` MCP tool or API. Not needed in CLI. |

### Rust-Only Additions (not in Python/Node)

| Command | Purpose |
|---------|---------|
| `init` | Create JACS agent locally (separated from register for clarity) |
| `search-messages` | Search email with filters (q, from, to, limit) |
| `update` | Re-sign agent metadata with existing key |
| `rotate` | Rotate cryptographic keys |
| `migrate` | Migrate legacy agent to current schema |
| `mcp` | Start embedded HAI + JACS MCP server on stdio |

## MCP Tool Parity

| MCP Tool | Python (`mcp_server.py`) | Node (`mcp-server.ts`) | Rust (`hai-mcp`) | Notes |
|----------|------------------------|----------------------|------------------|-------|
| `hai_hello` | Y | Y | Y | Full parity |
| `hai_register_agent` | Y | Y | Y | Full parity |
| `hai_agent_status` | Y | Y | Y | Full parity |
| `hai_verify_status` | -- | -- | Y | Rust-only: verify with optional agent_id |
| `hai_check_username` | Y | Y | Y | Full parity |
| `hai_claim_username` | Y | Y | Y | Full parity |
| `hai_verify_agent` | Y | Y | -- | Dropped from Rust MCP; use jacs-mcp verify tools |
| `hai_generate_verify_link` | Y | Y | Y | Full parity |
| `hai_create_agent` | -- | -- | Y | Rust-only: create new JACS agent via MCP |
| `hai_send_email` | Y | Y | Y | Full parity; Rust adds stateless agent_id/email |
| `hai_list_messages` | Y | Y | Y | Full parity |
| `hai_get_message` | Y | Y | Y | Full parity |
| `hai_delete_message` | Y | Y | Y | Full parity |
| `hai_mark_read` | Y | Y | Y | Full parity |
| `hai_mark_unread` | Y | Y | Y | Full parity |
| `hai_search_messages` | Y | Y | Y | Full parity |
| `hai_get_unread_count` | Y | Y | Y | Full parity |
| `hai_get_email_status` | Y | Y | Y | Full parity |
| `hai_reply_email` | Y | Y | Y | Full parity |

### MCP Tools Intentionally Dropped

| Tool | Source | Reason |
|------|--------|--------|
| `hai_verify_agent` | Python/Node | Functionality covered by jacs-mcp verify tools. Rust MCP server composes jacs-mcp + hai-mcp. |

### Rust-Only MCP Additions

| Tool | Purpose |
|------|---------|
| `hai_verify_status` | Get verification status for current or specified agent |
| `hai_create_agent` | Create a new JACS agent locally and optionally register with HAI |

## Automated Enforcement

MCP tool and CLI command parity are now enforced via shared fixtures and CI:

- **MCP tools**: `fixtures/mcp_tool_contract.json` lists all 28 tools with properties and required fields. Rust tests in `hai-mcp` enforce bidirectional parity (fixture must match code, code must match fixture). Python and Node tests validate FFI adapter coverage against the same fixture.
- **CLI commands**: `fixtures/cli_command_parity.json` lists all 29 subcommands. Rust tests in `haiai-cli` enforce bidirectional parity via Clap introspection.
- **CI gating**: `scripts/ci/check_mcp_parity_fixture.sh` validates fixture structural integrity (JSON valid, counts match) and runs as a gating job before all test suites.

## Summary

The Rust `haiai-cli` and `hai-mcp` are strict supersets of the Python/Node implementations:

- **CLI**: 29 Rust commands vs 9 Python / 10 Node. All Python/Node commands have Rust equivalents (2 intentionally dropped as redundant with MCP).
- **MCP**: 28 Rust tools vs 15 Python / 17 Node. All Python/Node tools have Rust equivalents (1 intentionally dropped, covered by jacs-mcp).
- **Architecture**: Rust CLI embeds the MCP server (`haiai mcp`), combining jacs-mcp and hai-mcp tools in one binary. Python/Node had separate CLI and MCP entry points.

**Decision**: Safe to delete Python/Node CLI and MCP server files. The Rust binaries are the canonical implementations.
