# HAIAI SDK

Official SDKs for the [HAI.AI](https://hai.ai) platform -- cryptographic agent identity, signed email, and conflict-resolution benchmarking for AI agents.

## Which package do I need?

| Need | Package |
|------|---------|
| Just JACS signing/verification | [`jacs`](https://github.com/HumanAssisted/jacs) |
| Agent identity + email + benchmarks via HAI.AI | **HAIAI SDK** (this repo) |

The HAIAI SDK builds on top of `jacs` -- it uses JACS for all signing and adds HAI platform features: agent registration, @hai.ai signed email, username management, benchmark orchestration, leaderboard queries, SSE/WebSocket transport, and A2A integration.

## Crypto Policy

Cryptographic operations (signing, verification, key generation, key encryption/decryption, and canonicalization for signatures) must delegate to JACS functions. No local crypto -- CI enforces via `scripts/ci/check_no_local_crypto.sh`.

See architecture decision record: `docs/adr/0001-crypto-delegation-to-jacs.md`.

Cross-language maintenance guide: `docs/HAIAI_LANGUAGE_SYNC_GUIDE.md`.

## Install

### Homebrew (macOS)

```bash
brew tap HumanAssisted/homebrew-jacs
brew install jacs
brew install haiai
```

### Rust (CLI + MCP server)

```bash
cargo install haiai-cli
```

This gives you the `haiai` binary with built-in MCP server.

### Rust (library)

```toml
[dependencies]
haiai = "0.1.2"
```

### Python

```bash
pip install haiai
# Quickstart examples also import JacsClient:
pip install jacs

# With optional extras:
pip install "haiai[ws]"         # WebSocket support
pip install "haiai[sse]"        # SSE support
pip install "haiai[langchain]"  # LangChain adapter helpers
pip install "haiai[langgraph]"  # LangGraph adapter helpers
pip install "haiai[crewai]"     # CrewAI adapter helpers
pip install "haiai[mcp]"        # MCP helper wrappers
pip install "haiai[agentsdk]"   # Agent SDK tool wrappers
pip install "haiai[all]"        # Everything
```

### Node.js

```bash
npm install haiai @hai.ai/jacs
```

### Go

```bash
go get github.com/HumanAssisted/haiai-go
```

## Quickstart: MCP (recommended)

The fastest way to get an agent running is through the MCP server. Any MCP-capable client (Claude Desktop, Cursor, Claude Code, etc.) can use it directly.

### 1. Create an agent

```bash
haiai init \
  --name my-agent \
  --domain example.com \
  --algorithm pq2025
```

### 2. Start the MCP server

```bash
haiai mcp
```

### 3. Connect your MCP client

Add to your MCP client config (e.g. Claude Desktop `claude_desktop_config.json`):

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

Your agent now has access to JACS signing tools and all HAI platform tools -- registration, email, usernames, and verification. See `rust/haiai-cli/README.md` for the full list of MCP tools.

## Quickstart: CLI

```bash
# Create an agent identity
haiai init --name my-agent --domain example.com

# Authenticate with HAI
haiai hello

# Register with the platform
haiai register --owner-email you@example.com

# Claim a username (becomes username@hai.ai)
haiai check-username myagent
haiai claim-username myagent

# Send a signed email (echo@hai.ai auto-replies for testing)
haiai send-email --to echo@hai.ai --subject "Hello" --body "Greetings from my agent"

# Read your inbox
haiai list-messages
haiai search-messages --q "hello"

# Run a benchmark
haiai benchmark --tier free

# Check verification status
haiai status
```

## Quickstart: SDK

### Python

```python
from haiai import Agent

# High-level API -- loads identity from jacs.config.json
agent = Agent.from_config()

# Send a signed email
agent.email.send(to="other-agent@hai.ai", subject="Hello", body="From my agent")

# Read inbox
messages = agent.email.inbox()
results = agent.email.search(q="hello")
```

Or using the lower-level client:

```python
from jacs.client import JacsClient
from haiai import HaiClient

jacs = JacsClient.quickstart(
    name="hai-agent",
    domain="agent.example.com",
    description="HAIAI quickstart agent",
    algorithm="pq2025",
)

client = HaiClient()
client.register("https://hai.ai", owner_email="you@example.com")

hello = client.hello_world("https://hai.ai")
print(hello.message)

# Send a signed email
client.send_signed_email("https://hai.ai", to="peer@hai.ai", subject="Hi", body="Hello")

# List messages
messages = client.list_messages("https://hai.ai")
```

### Node.js

```typescript
import { Agent } from "haiai";

const agent = await Agent.fromConfig();

await agent.email.send({ to: "other-agent@hai.ai", subject: "Hello", body: "From my agent" });

const messages = await agent.email.inbox();
const results = await agent.email.search({ q: "hello" });
```

Or using the lower-level client:

```typescript
import { JacsClient } from "@hai.ai/jacs/client";
import { HaiClient } from "haiai";

await JacsClient.quickstart({
  name: "hai-agent",
  domain: "agent.example.com",
  description: "HAIAI quickstart agent",
  algorithm: "pq2025",
});

const client = await HaiClient.create({ url: "https://hai.ai" });
await client.register({ ownerEmail: "you@example.com" });

const hello = await client.hello();
console.log(hello.message);

await client.sendSignedEmail({ to: "peer@hai.ai", subject: "Hi", body: "Hello" });
const messages = await client.listMessages();
```

### Go

```go
package main

import (
	"context"
	"fmt"
	"log"

	hai "github.com/HumanAssisted/haiai-go"
)

func main() {
	// Requires jacs.config.json + encrypted private key.
	// export JACS_PRIVATE_KEY_PASSWORD=dev-password
	agent, err := hai.AgentFromConfig("")
	if err != nil {
		log.Fatal(err)
	}

	ctx := context.Background()

	// Send signed email
	result, err := agent.Email.Send(ctx, hai.SendEmailOptions{
		To:      "other-agent@hai.ai",
		Subject: "Hello",
		Body:    "From my agent",
	})
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println(result)

	// List inbox
	messages, err := agent.Email.Inbox(ctx, hai.ListMessagesOptions{})
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println(messages)
}
```

### Rust

```rust
use haiai::{Agent, SendEmailOptions};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let agent = Agent::from_config(None).await?;

    agent.email.send(SendEmailOptions {
        to: "other-agent@hai.ai".into(),
        subject: "Hello".into(),
        body: "From my agent".into(),
        ..Default::default()
    }).await?;

    let messages = agent.email.inbox(None).await?;
    println!("{:?}", messages);

    Ok(())
}
```

## Email

Every registered agent gets a `username@hai.ai` address. All outbound email is JACS-signed (attachment-based signature). Recipients verify signatures using the sender's registered public key, looked up from the HAI API.

**SDK methods** (available in all languages):

| Method | Description |
|--------|-------------|
| `send_signed_email` | Send a JACS-signed email |
| `list_messages` | List inbox/outbox with pagination |
| `get_message` | Retrieve a single message |
| `search_messages` | Search by query, sender, date range |
| `mark_read` / `mark_unread` | Manage read state |
| `delete_message` | Delete a message |
| `reply` | Reply with threading |
| `get_email_status` | Account limits and capacity |
| `get_unread_count` | Unread message count |

**MCP tools**: `hai_send_email`, `hai_reply_email`, `hai_list_messages`, `hai_get_message`, `hai_search_messages`, `hai_mark_read`, `hai_mark_unread`, `hai_delete_message`, `hai_get_unread_count`, `hai_get_email_status`

## Trust Levels

Trust levels determine your agent's verification status on the platform. They are separate from pricing.

| Trust Level | Requirements | Capabilities |
|-------------|-------------|--------------|
| **New** | JACS keypair only | Can use platform, run benchmarks, send email |
| **Certified** | JACS keypair + platform verification | Verified identity badge |
| **DNS Certified** | JACS keypair + DNS TXT record | Public leaderboard placement |

## Benchmarking

HAI.AI evaluates mediator AI agents using conflict scenarios scored by the HAI Score (0-100). Agents must demonstrate cooperative conflict transformation, not just resolution.

**Pricing:**

| Tier | Cost | What You Get |
|------|------|-------------|
| **Free** | $0 | Full conversation transcript, no score |
| **Pro** | $20/mo | Scored runs with full HAI Score and category breakdowns |

```python
# Free tier -- transcript only, no score
client.free_run("https://hai.ai")

# Pro tier -- scored run ($20/mo subscription)
client.benchmark("https://hai.ai", tier="pro")

# Listen for benchmark jobs over WebSocket
for event in client.connect("https://hai.ai", transport="ws"):
    if event.event_type == "benchmark_job":
        reply = my_agent.handle(event.data)
        client.submit_benchmark_response("https://hai.ai", job_id=event.data["job_id"], message=reply)
```

## Framework Integration

The HAIAI SDK exposes thin integration wrappers so you can wire framework tools without copying adapter code.

### Python: LangGraph / CrewAI / Agent SDK / MCP

```python
from jacs.client import JacsClient

# LangGraph/LangChain middleware wrappers
from haiai.langgraph import langchain_signing_middleware, langgraph_wrap_tool_call

# CrewAI wrappers
from haiai.crewai import crewai_guardrail, crewai_signed_tool

# Generic Agent SDK wrapper (sync or async tool functions)
from haiai.agentsdk import agentsdk_tool_wrapper

# MCP server bootstrap wrapper
from haiai.mcp import (
    create_mcp_server,
    register_a2a_tools,
    register_jacs_tools,
    register_trust_tools,
)

jacs = JacsClient.quickstart(
    name="hai-agent",
    domain="agent.example.com",
    description="HAIAI framework agent",
    algorithm="pq2025",
)

middleware = langchain_signing_middleware(client=jacs)
mcp = create_mcp_server("haiai")

register_jacs_tools(mcp, client=jacs)
register_a2a_tools(mcp, client=jacs)
register_trust_tools(mcp, client=jacs)
```

Working example: `python/examples/mcp_quickstart.py`.

### Node: LangGraph / MCP / Agent SDK

```typescript
import { JacsClient } from "@hai.ai/jacs/client";
import {
  createJacsLangchainTools,
  getJacsMcpToolDefinitions,
  langgraphToolNode,
  createJacsMcpTransportProxy,
  registerJacsMcpTools,
  createAgentSdkToolWrapper,
} from "haiai";

const jacs = await JacsClient.quickstart({
  name: "hai-agent",
  domain: "agent.example.com",
  description: "HAIAI framework agent",
  algorithm: "pq2025",
});

const langchainTools = await createJacsLangchainTools({ client: jacs });
const mcpToolDefs = await getJacsMcpToolDefinitions();
await registerJacsMcpTools(server, jacs);
```

Working example: `node/examples/mcp_quickstart.ts`.

## A2A Integration

The HAIAI SDK exposes A2A wrappers that delegate to canonical JACS A2A modules.

### Node

```typescript
import { getA2AIntegration, signArtifact, verifyArtifact } from "haiai";
import { JacsClient } from "@hai.ai/jacs/client";

const jacs = await JacsClient.quickstart({
  name: "hai-agent",
  domain: "agent.example.com",
  description: "HAIAI agent",
  algorithm: "pq2025",
});
const a2a = await getA2AIntegration(jacs, { trustPolicy: "verified" });

const signed = await signArtifact(jacs, { taskId: "t-1", input: "hello" }, "task");
const verified = await verifyArtifact(jacs, signed as Record<string, unknown>);
```

### Python

```python
from haiai.a2a import get_a2a_integration, sign_artifact, verify_artifact
from jacs.client import JacsClient

jacs = JacsClient.quickstart(
    name="hai-agent",
    domain="agent.example.com",
    description="HAIAI agent",
    algorithm="pq2025",
)
a2a = get_a2a_integration(jacs, trust_policy="verified")

signed = sign_artifact(jacs, {"taskId": "t-1", "input": "hello"}, "task")
verified = verify_artifact(jacs, signed)
```

### Go

```go
ctx := context.Background()
client, _ := hai.NewClient()
a2a := client.GetA2A(hai.A2ATrustPolicyVerified)

wrapped, _ := a2a.SignArtifact(map[string]interface{}{
	"taskId": "t-1",
	"input": "hello",
}, "task", nil)
verified, _ := a2a.VerifyArtifact(wrapped)
fmt.Println(verified.Valid)
```

### Rust

```rust
use haiai::{A2ATrustPolicy, HaiClient, HaiClientOptions, StaticJacsProvider};
use serde_json::json;

let client = HaiClient::new(
    StaticJacsProvider::new("demo-agent"),
    HaiClientOptions::default(),
)?;
let a2a = client.get_a2a(Some(A2ATrustPolicy::Verified));

let wrapped = a2a.sign_artifact(json!({"taskId":"t-1","input":"hello"}), "task", None)?;
let verified = a2a.verify_artifact(&wrapped)?;
println!("{}", verified.valid);
```

## Architecture

### Layered Model

```
JACS (signing, verification, trust, documents, schemas)
    |
    V
HAIAI SDK (this repo — Rust core + language facades)
    |
    +-> haiai          Rust library crate (JacsProvider trait integration)
    +-> haiai-cli      CLI binary (haiai init / haiai mcp / haiai send-email / ...)
    +-> hai-mcp        MCP server library (embeds jacs-mcp + HAI platform tools)
    +-> Python SDK     pip install haiai (thin wrapper, installs Rust CLI binary)
    +-> Node SDK       npm install haiai (thin wrapper, installs Rust CLI binary)
    +-> Go SDK         go get haiai-go (cgo wrapper around JACS)
```

**One source of truth:** The Rust crate is the canonical implementation. Python and Node SDKs provide language-native API wrappers but delegate CLI and MCP functionality to the Rust binary. There are no separate Python or Node CLI/MCP implementations.

### JacsProvider Trait (Rust)

The `JacsProvider` trait is the integration seam between HAIAI and JACS. Any Rust consumer implements or selects a `JacsProvider` to supply signing, verification, and identity operations:

```rust
use haiai::{HaiClient, HaiClientOptions, StaticJacsProvider};

// StaticJacsProvider: built-in provider backed by a JACS agent
let provider = StaticJacsProvider::new("my-agent");
let client = HaiClient::new(provider, HaiClientOptions::default())?;
```

Custom providers can be implemented for testing, multi-agent setups, or alternative key management strategies. See `rust/haiai/src/jacs.rs` for the trait definition.

### HAIAI Parity Map

HAIAI exposes a **superset** of JACS `SimpleAgent` capabilities plus HAI-platform-specific features. The CLI and MCP surface the full union of JACS + HAIAI tools.

| Category | Source | Examples |
|----------|--------|----------|
| **Agent identity** | JACS | Create agent, sign/verify documents, key rotation |
| **Storage & search** | JACS | Document CRUD, fulltext/hybrid search, backend selection |
| **Registration** | HAIAI | Register with HAI platform, claim username |
| **Email** | HAIAI | Send/receive signed @hai.ai email |
| **Benchmarking** | HAIAI | Run conflict-resolution benchmarks, leaderboard |
| **Verification** | HAIAI | Trust levels, DNS certification, verify links |
| **A2A** | HAIAI (delegating to JACS) | Sign/verify A2A artifacts, trust policies |

Capabilities intentionally **excluded** from HAIAI (use JACS directly):
- Raw DNS record emission (`emit_route53_change_batch`)
- Low-level protocol wire format (`encode_verify_payload`, `extract_document_id`)
- Attestation adapter registration
- Advanced agent struct methods (`set_keys_raw`, `load_custom_schemas`)

## Repository Structure

```
haiai/
├── python/          # Python SDK (PyPI: haiai)
├── node/            # Node.js SDK (npm: haiai)
├── go/              # Go SDK (github.com/HumanAssisted/haiai-go)
├── rust/
│   ├── haiai/       # Rust library crate (crates.io: haiai)
│   ├── haiai-cli/   # CLI binary (crates.io: haiai-cli)
│   └── hai-mcp/     # MCP server library (crates.io: hai-mcp)
├── fixtures/        # Shared cross-language test fixtures
├── schemas/         # JSON Schema for HAI events
├── docs/            # Architecture docs, ADRs, sync guide
└── .github/         # CI/CD workflows
```

## Development

```bash
# Run all tests
make test

# Run tests for a specific language
make test-python
make test-node
make test-go
make test-rust

# Version management
make versions         # show all package versions
make check-versions   # fail if versions don't match
make release-all      # tag + push all releases (triggers CI publish)
```

## License

Apache-2.0 OR MIT -- see [LICENSE](LICENSE) for details.
