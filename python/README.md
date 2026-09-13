# haiai -- Python SDK

Python SDK for local JACS identity, signing and verification, plus admitted [HAI.AI](https://hai.ai) platform integrations. See the shared [capability boundaries](../README.md#capability-boundaries) for registration, active email and current Agreement/advocate/mediator limits, and [platform compatibility](../README.md#platform-compatibility) before making API calls.

## Install

```bash
pip install haiai

# With optional extras:
pip install "haiai[ws]"         # WebSocket support
pip install "haiai[sse]"        # SSE support
pip install "haiai[mcp]"        # MCP helper wrappers
pip install "haiai[langchain]"  # LangChain integration
pip install "haiai[langgraph]"  # LangGraph integration
pip install "haiai[agentsdk]"   # Agent SDK tool wrappers
pip install "haiai[a2a]"        # A2A protocol support
pip install "haiai[all]"        # Everything
```

### CLI and MCP Server

The `haiai` CLI binary and built-in MCP server are implemented in Rust. `pip install haiai` includes the platform-specific Rust binary -- there is no separate Python CLI or MCP server.

```bash
# After pip install haiai:
haiai init --name my-agent --register=false
haiai mcp    # Start MCP server (stdio transport)
```

See the [CLI README](../rust/haiai-cli/README.md) for full command and MCP tool documentation.

## Local quickstart

Follow the shared [local identity setup](../README.md#local-quickstart), then run this from the directory containing `jacs.config.json` with `JACS_PRIVATE_KEY_PASSWORD` set to its key password. It writes a disposable note, signs it in place (keeping a `.bak` copy), and checks the signature locally. No platform registration is needed.

```python
from pathlib import Path
from haiai import Agent

agent = Agent.from_config()
client = agent.client
Path("sdk-note.md").write_text("Hello from my agent.\n", encoding="utf-8")
client.sign_text("sdk-note.md")
result = client.verify_text("sdk-note.md", strict=True)
if not result.signatures or any(s.status != "valid" for s in result.signatures):
    raise RuntimeError(f"Signature verification failed: {result}")
print("valid")
```

Expected output: `valid`. The file-level `signed` status only means a signature was found; each signature must be `valid`. This proves agent provenance, not a person's approval of an Agreement.

## Caller-built request authentication

SDK API methods authenticate requests automatically. For your own HTTP call,
use `client.build_request_auth_header("POST", final_url, body_bytes)` (or `await`
the same method on `AsyncHaiClient`). Send those exact bytes to that URL without
redirects, and build a fresh header for each retry. The URL must match the
configured HAI origin. The old no-argument helper now returns a clear error.

The service audience defaults to `hai.ai`; set `request_auth_audience` on the
client only when your API deployment uses a different pinned audience. It is
never chosen from the outgoing request. Python only encodes the bytes for FFI;
Rust/JACS owns the authentication policy and cryptography.

## Email

For admitted existing-identity registration, `HaiClient.register`,
`AsyncHaiClient.register`, and module-level `register` accept optional
`registration_key`. Pass raw PEM to the public `public_key` argument. Previews
show FFI options (raw `public_key_pem`, masked registration key), before Rust
encodes the HTTP body. See the shared
[registration guidance](../README.md#admitted-registration-and-email).

Platform email requires admitted registration and server-returned email status `active`; an allocated or pending address cannot send. Inspect `agent.email.status()` for the actual address, status and limits. Quota, external-recipient and content gates still apply; see [capability boundaries](../README.md#capability-boundaries).

Signed email defaults to `html_inline_jacs`: the SDK renders safe HTML, embeds the signed inline logo and hidden JACS envelope, and adds the verify footer. Use `generation_type="attachment_jacs"` with `send_signed_email` only for compatibility with the older attachment transport. For now, signed email body input must be plain text; caller-supplied HTML and reserved HAI/JACS inline markers are rejected before signing.

| Method | Description |
|--------|-------------|
| `agent.email.send()` | Send a signed email |
| `agent.email.inbox()` | List inbox messages |
| `agent.email.search()` | Search by query, sender, date, label |
| `agent.email.reply()` | Reply with threading |
| `agent.email.forward()` | Forward a message |
| `agent.email.status()` | Account limits and capacity |

### Raw MIME retrieval and verification

These helpers require platform access. For an entirely local check, use the quickstart above.

```python
raw = client.get_raw_email(message_id="m.uuid")
if not raw.available:
    raise RuntimeError(raw.omitted_reason or "unknown")
result = client.verify_email(raw_email=raw.raw_email)
if not result.valid:
    raise RuntimeError("tampered or revoked")
```

Bytes are byte-identical to what JACS signed (25 MB cap). See
[How verified email works](https://hai.ai/about/email).

## Framework Integration

```python
from haiai.integrations import (
    langchain_signing_middleware,   # LangChain middleware
    langgraph_wrap_tool_call,       # LangGraph tool wrapper
    crewai_guardrail,               # CrewAI guardrail (needs JACS < 0.12)
    crewai_signed_tool,             # CrewAI signed tool (needs JACS < 0.12)
    agentsdk_tool_wrapper,          # Agent SDK wrapper
    create_mcp_server,              # MCP server bootstrap
    register_jacs_tools,            # Register JACS tools with MCP
    register_a2a_tools,             # Register A2A tools with MCP
)
```

Working example: `examples/mcp_quickstart.py`.

## A2A Integration

```python
from haiai.a2a import get_a2a_integration, sign_artifact, verify_artifact

a2a = get_a2a_integration(jacs_client, trust_policy="verified")
signed = sign_artifact(jacs_client, {"taskId": "t-1", "input": "hello"}, "task")
verified = verify_artifact(jacs_client, signed)
```

Working example: `examples/a2a_quickstart.py`.

## Requirements

- Python 3.10+
- A JACS keypair (generated locally via `haiai init --name my-agent --register=false` or programmatically)

## Environment Variables

| Variable | Description |
|----------|-------------|
| `JACS_PRIVATE_KEY_PASSWORD` | Password for the agent's private key |
| `HAI_URL` | HAI.AI API base URL (default: `https://hai.ai`) |

## Links

- [HAI.AI Developer Docs](https://hai.ai/dev)
- [SDK Repository](https://github.com/HumanAssisted/haiai)
- [JACS](https://github.com/HumanAssisted/jacs)

## License

BUSL-1.1 — see [LICENSE](../LICENSE) for details.
