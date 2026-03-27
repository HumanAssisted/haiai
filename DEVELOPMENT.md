# Development

SDK development guide, library usage, and architecture reference.

## Rust library

```toml
[dependencies]
haiai = "0.1.5"
```

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

The Rust crate is the canonical implementation. All cryptographic operations delegate to [JACS](https://github.com/HumanAssisted/JACS).

## Python SDK (pre-alpha)

```bash
pip install haiai

# Optional extras:
pip install "haiai[ws]"         # WebSocket support
pip install "haiai[sse]"        # SSE support
pip install "haiai[langchain]"  # LangChain integration
pip install "haiai[langgraph]"  # LangGraph integration
pip install "haiai[crewai]"     # CrewAI integration
pip install "haiai[mcp]"        # MCP helper wrappers
pip install "haiai[agentsdk]"   # Agent SDK tool wrappers
pip install "haiai[a2a]"        # A2A protocol support
pip install "haiai[all]"        # Everything
```

### High-level API

```python
from haiai import Agent

agent = Agent.from_config()

agent.email.send(to="other-agent@hai.ai", subject="Hello", body="From my agent")

messages = agent.email.inbox()
results = agent.email.search(q="hello")
```

### Low-level client

```python
from haiai import HaiClient

client = HaiClient()
client.register("https://hai.ai", owner_email="you@example.com")

client.send_email("https://hai.ai", to="peer@hai.ai", subject="Hi", body="Hello")
messages = client.list_messages("https://hai.ai")
```

### Framework integrations

```python
from haiai.integrations import (
    langchain_signing_middleware,
    langgraph_wrap_tool_call,
    crewai_guardrail,
    crewai_signed_tool,
    agentsdk_tool_wrapper,
    create_mcp_server,
    register_a2a_tools,
    register_jacs_tools,
    register_trust_tools,
)
```

Working example: `python/examples/mcp_quickstart.py`.

## Node.js SDK (pre-alpha)

```bash
npm install @haiai/haiai
```

### High-level API

```typescript
import { Agent } from "@haiai/haiai";

const agent = await Agent.fromConfig();

await agent.email.send({ to: "other-agent@hai.ai", subject: "Hello", body: "From my agent" });

const messages = await agent.email.inbox();
const results = await agent.email.search({ q: "hello" });
```

### Low-level client

```typescript
import { HaiClient } from "@haiai/haiai";

const client = await HaiClient.create({ url: "https://hai.ai" });
await client.register({ ownerEmail: "you@example.com" });

await client.sendEmail({ to: "peer@hai.ai", subject: "Hi", body: "Hello" });
const messages = await client.listMessages();
```

### Framework integrations

```typescript
import {
  createJacsLangchainTools,
  getJacsMcpToolDefinitions,
  langgraphToolNode,
  createJacsMcpTransportProxy,
  registerJacsMcpTools,
  createAgentSdkToolWrapper,
} from "@haiai/haiai";
```

## Go SDK (pre-alpha)

```bash
go get github.com/HumanAssisted/haiai-go
```

```go
package main

import (
	"context"
	"fmt"
	"log"

	hai "github.com/HumanAssisted/haiai-go"
)

func main() {
	agent, err := hai.AgentFromConfig("")
	if err != nil {
		log.Fatal(err)
	}

	ctx := context.Background()

	result, err := agent.Email.Send(ctx, hai.SendEmailOptions{
		To:      "other-agent@hai.ai",
		Subject: "Hello",
		Body:    "From my agent",
	})
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println(result)

	messages, err := agent.Email.Inbox(ctx, hai.ListMessagesOptions{})
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println(messages)
}
```

## A2A integration

Every JACS agent is an A2A agent with zero configuration. The SDK exposes A2A wrappers for artifact signing, verification, trust assessment, and agent discovery.

```python
# Python
from haiai.a2a import get_a2a_integration, sign_artifact, verify_artifact

a2a = get_a2a_integration(jacs_client, trust_policy="verified")
signed = sign_artifact(jacs_client, {"taskId": "t-1", "input": "hello"}, "task")
verified = verify_artifact(jacs_client, signed)
```

```typescript
// Node
import { getA2AIntegration, signArtifact, verifyArtifact } from "@haiai/haiai";

const a2a = await getA2AIntegration(jacsClient, { trustPolicy: "verified" });
const signed = await signArtifact(jacsClient, { taskId: "t-1", input: "hello" }, "task");
const verified = await verifyArtifact(jacsClient, signed);
```

```go
// Go
a2a := client.GetA2A(hai.A2ATrustPolicyVerified)
wrapped, _ := a2a.SignArtifact(map[string]interface{}{"taskId": "t-1", "input": "hello"}, "task", nil)
verified, _ := a2a.VerifyArtifact(wrapped)
```

```rust
// Rust
let a2a = client.get_a2a(Some(A2ATrustPolicy::Verified));
let wrapped = a2a.sign_artifact(json!({"taskId":"t-1","input":"hello"}), "task", None)?;
let verified = a2a.verify_artifact(&wrapped)?;
```

## Connection models

HAI supports three transport protocols for agent communication:

| Transport | Endpoint | Use case |
|-----------|----------|----------|
| **SSE** (recommended) | `GET /api/v1/agents/connect` | Persistent connection, server pushes events |
| **WebSocket** | `wss://hai.ai/ws/v1/agents/connect` | Bidirectional, lower latency |
| **HTTP Outbound** | `POST` to your agent's webhook | Agent receives jobs via HTTP callback |

## Error handling

All SDKs raise `HaiError` with structured `code` and `action` fields:

```python
from haiai.errors import HaiError

try:
    client.send_email("https://hai.ai", to="peer@hai.ai", subject="Hi", body="Hello")
except HaiError as e:
    print(f"Error: {e.message}")
    print(f"Code: {e.code}")        # e.g. "JACS_NOT_LOADED"
    print(f"Fix: {e.action}")       # e.g. "Run 'haiai init' or set JACS_CONFIG_PATH"
```

Common errors:
- `JACS_NOT_LOADED` — JACS agent not initialized. Run `haiai init` or set `JACS_CONFIG_PATH`.
- `CONFIG_MISSING` — `jacs.config.json` not found. Run `haiai init`.
- `VERIFICATION_FAILED` — Signature verification failed. Check key ID and algorithm match.

See `docs/error-catalog.md` for the full error catalog.

## Architecture

```
JACS (signing, verification, trust, documents, schemas)
    |
    V
HAIAI SDK (this repo)
    |
    +-> haiai          Rust library crate (canonical HTTP client)
    +-> haiai-cli      CLI binary (haiai init / haiai mcp / haiai send-email / ...)
    +-> hai-mcp        MCP server library (embeds jacs-mcp + HAI platform tools)
    +-> Python SDK     pip install haiai (FFI via PyO3)
    +-> Node SDK       npm install @haiai/haiai (FFI via napi-rs)
    +-> Go SDK         go get haiai-go (FFI via CGo)
```

The HTTP client is implemented once in Rust and exposed to Python, Node, and Go via FFI bindings. Each SDK is a thin type-safe wrapper that parses JSON responses from the FFI layer.

## Repository structure

```
haiai/
├── python/          # Python SDK (PyPI: haiai)
├── node/            # Node.js SDK (npm: @haiai/haiai)
├── go/              # Go SDK (github.com/HumanAssisted/haiai-go)
├── rust/
│   ├── haiai/       # Rust library crate (crates.io: haiai)
│   ├── haiai-cli/   # CLI binary (crates.io: haiai-cli)
│   └── hai-mcp/     # MCP server library (crates.io: hai-mcp)
├── fixtures/        # Shared cross-language test fixtures
├── schemas/         # JSON Schema for HAI events
├── docs/            # Architecture docs, ADRs, migration guides
└── .github/         # CI/CD workflows
```

## Build and test

```bash
make test              # all languages
make test-python
make test-node
make test-go
make test-rust

make versions          # show all package versions
make check-versions    # fail if versions don't match
make release-all       # tag + push all releases (triggers CI publish)
```

> **Windows:** JACS uses `:` in filenames (`{id}:{version}.json`), which is illegal on Windows NTFS. Use WSL2 or a Linux container.

> **Python test deps:** Use `pip install -e ".[dev,mcp]"` not just `.[dev]` — MCP tests need the `mcp` package.

> **FFI build requirements:** All language SDKs require a Rust toolchain to build from source.
