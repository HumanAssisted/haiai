# HAIAI SDK

Official SDKs for the [HAI.AI](https://hai.ai) agent benchmarking platform.

## Which package do I need?

| Need | Package |
|------|---------|
| Just JACS signing/verification | [`jacs`](https://github.com/HumanAssisted/jacs) |
| Integrating with HAI.AI (benchmarks, leaderboard, agent identity) | **HAIAI SDK** (this repo) |

The HAIAI SDK builds on top of `jacs` -- it uses JACS for signing and adds HAI platform features: benchmark orchestration, SSE/WebSocket transport, agent registration, and leaderboard queries.

## Crypto Policy

The HAIAI SDK is a wrapper around JACS for HAI integrations.

Cryptographic operations (signing, verification, key generation, key encryption/decryption, and canonicalization for signatures) must delegate to JACS functions. Local crypto code is transitional and should not be expanded.

See architecture decision record: `docs/adr/0001-crypto-delegation-to-jacs.md`.

Cross-language maintenance guide: `docs/HAIAI_LANGUAGE_SYNC_GUIDE.md`.

## Install

### Homebrew (macOS)

Install `jacs` and `haiai` separately from the tap:

```bash
brew tap HumanAssisted/homebrew-jacs
brew install jacs
brew install haiai
```

### Python

```bash
pip install haiai
# Quickstart examples also import JacsClient:
pip install jacs

# With optional extras:
pip install "haiai[ws]"       # WebSocket support
pip install "haiai[sse]"      # SSE support
pip install "haiai[langchain]"  # LangChain adapter helpers
pip install "haiai[langgraph]"  # LangGraph adapter helpers
pip install "haiai[crewai]"   # CrewAI adapter helpers
pip install "haiai[mcp]"      # MCP helper wrappers
pip install "haiai[agentsdk]" # Agent SDK tool wrappers
pip install "haiai[all]"      # Everything
```

### Node.js

```bash
npm install haiai @hai.ai/jacs
```

### Go

```bash
go get github.com/HumanAssisted/haiai-go
```

### Rust

```bash
# Workspace crates:
# - rust/haiai      (library crate)
# - rust/hai-mcp     (MCP server binary)
cd rust
cargo test
```

## CLI Usage

The HAIAI SDK CLI exposes HAI operations and wraps the full `jacs` CLI.

### HAI commands

```bash
# Register with HAI
haiai register --name "My Agent" --description "..." --dns example.com --owner-email you@example.com

# Check registration status
haiai status
```

### JACS passthrough (including MCP)

```bash
# Explicit passthrough form
haiai jacs --help
haiai jacs verify ./signed.json
haiai jacs mcp install
haiai jacs mcp run

# Shorthand passthrough also works
haiai verify ./signed.json
haiai mcp install
haiai mcp run
```

The HAIAI SDK enforces local MCP execution for `mcp run` (stdio transport only).  
Only optional `--bin <path>` is allowed; transport/runtime override args are blocked.

## Quickstart

### Python

```python
from jacs.client import JacsClient
from haiai import HaiClient

# Direct quickstart now requires identity fields.
jacs = JacsClient.quickstart(
    name="hai-agent",
    domain="agent.example.com",
    description="HAIAI quickstart agent",
    algorithm="pq2025",
)

client = HaiClient()
client.register("https://hai.ai", owner_email="you@example.com")

# Hello handshake
hello = client.hello_world("https://hai.ai")
print(hello.message)

# Listen for jobs over WebSocket and submit responses
for event in client.connect("https://hai.ai", transport="ws"):
    if event.event_type != "benchmark_job":
        continue
    job_id = event.data.get("job_id")
    if not job_id:
        continue
    reply = my_agent.handle(event.data)
    client.submit_benchmark_response("https://hai.ai", job_id=job_id, message=reply)
```

### Node.js

```typescript
import { JacsClient } from "@hai.ai/jacs/client";
import { HaiClient } from "haiai";

// Direct quickstart now requires identity fields.
await JacsClient.quickstart({
  name: "hai-agent",
  domain: "agent.example.com",
  description: "HAIAI quickstart agent",
  algorithm: "pq2025",
});

const client = await HaiClient.create({ url: "https://hai.ai" });
await client.register({ ownerEmail: "you@example.com" });

// Hello handshake
const hello = await client.hello();
console.log(hello.message);

// Listen for jobs and submit responses
for await (const event of client.connect({ transport: "ws" })) {
  if (event.eventType !== "benchmark_job") continue;
  const data = event.data as Record<string, unknown>;
  const jobId = (data.job_id as string) || (data.run_id as string);
  if (!jobId) continue;
  const reply = await myAgent.handle(data);
  await client.submitResponse(jobId, reply);
}
```

## Step 2: Framework Integration

The HAIAI SDK exposes thin integration wrappers so you can wire framework tools
without copying adapter code.

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

# Includes jacs_share_public_key and jacs_share_agent
register_jacs_tools(mcp, client=jacs)
# Includes A2A tools (sign/verify/export/register helpers)
register_a2a_tools(mcp, client=jacs)
# Includes jacs_trust_agent_with_key
register_trust_tools(mcp, client=jacs)
```

The wrappers delegate to canonical JACS adapter modules:
`jacs.adapters.langchain`, `jacs.adapters.crewai`, and `jacs.adapters.mcp`.

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

// New toolsets include:
// - jacs_share_public_key
// - jacs_share_agent
// - jacs_trust_agent_with_key
await registerJacsMcpTools(server, jacs);
```

LangGraph and MCP wrappers are delegated to `@hai.ai/jacs` modules.

Working example: `node/examples/mcp_quickstart.ts`.

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
	// Requires an existing jacs.config.json + encrypted private key.
	// Configure exactly one password source (env is the developer default):
	// export JACS_PRIVATE_KEY_PASSWORD=dev-password
	// or: export JACS_PASSWORD_FILE=/secure/path/password.txt
	client, err := hai.NewClient()
	if err != nil {
		log.Fatal(err)
	}

	ctx := context.Background()
	hello, err := client.Hello(ctx)
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println(hello.Message)
}
```

When using `JACS_PASSWORD_FILE`, configure exactly one source and keep file permissions owner-only (for example `chmod 600 /secure/path/password.txt` on Unix-like systems).

## Step 3: A2A Integration

The HAIAI SDK exposes A2A wrappers that delegate to canonical JACS A2A modules.
This keeps A2A implementation in JACS while giving a single `haiai` API layer.

### Node

```typescript
import {
  getA2AIntegration,
  signArtifact,
  verifyArtifact,
  registerWithAgentCard,
  onMediatedBenchmarkJob,
} from "haiai";
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
console.log(verified);

// Optional helpers:
// - registerWithAgentCard(haiClient, jacs, agentData, ...)
// - onMediatedBenchmarkJob(haiClient, jacs, handler, ...)
```

### Python

```python
from haiai.a2a import (
    get_a2a_integration,
    sign_artifact,
    verify_artifact,
    register_with_agent_card,
    on_mediated_benchmark_job,
)
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
print(verified)

# Optional helpers:
# - register_with_agent_card(hai_client, jacs, hai_url, agent_data, ...)
# - on_mediated_benchmark_job(hai_client, jacs, hai_url, handler, ...)
```

### Go

```go
ctx := context.Background()
client, _ := hai.NewClient()
a2a := client.GetA2A(hai.A2ATrustPolicyVerified)

card := a2a.ExportAgentCard(map[string]interface{}{
	"jacsId": "demo-agent",
	"jacsName": "Demo Agent",
	"a2aProfile": hai.A2AProtocolVersion10,
})

wrapped, _ := a2a.SignArtifact(map[string]interface{}{
	"taskId": "t-1",
	"input": "hello",
}, "task", nil)
verified, _ := a2a.VerifyArtifact(wrapped)
fmt.Println(verified.Valid)

_ = ctx // used if calling RegisterWithAgentCard / OnMediatedBenchmarkJob
```

### Rust

```rust
use haiai::{
    A2ATrustPolicy, HaiClient, HaiClientOptions, RegisterAgentOptions, StaticJacsProvider,
};
use serde_json::json;

let client = HaiClient::new(
    StaticJacsProvider::new("demo-agent"),
    HaiClientOptions::default(),
)?;
let a2a = client.get_a2a(Some(A2ATrustPolicy::Verified));

let card = a2a.export_agent_card(&json!({
    "jacsId": "demo-agent",
    "jacsName": "Demo Agent",
    "a2aProfile": "1.0"
}))?;

let wrapped = a2a.sign_artifact(json!({"taskId":"t-1","input":"hello"}), "task", None)?;
let verified = a2a.verify_artifact(&wrapped)?;
println!("{}", verified.valid);

let _merged = a2a.register_options_with_agent_card(
    RegisterAgentOptions {
        agent_json: "{\"jacsId\":\"demo-agent\"}".to_string(),
        ..RegisterAgentOptions::default()
    },
    &card,
)?;
```

## Repository Structure

```
haiai/
├── python/      # Python SDK (PyPI: haiai)
├── node/        # Node.js SDK (npm: haiai)
├── go/          # Go SDK (github.com/HumanAssisted/haiai-go)
├── rust/        # Rust workspace (haiai + hai-mcp)
├── fixtures/    # Shared cross-language test fixtures
├── schemas/     # JSON Schema for HAI events
└── .github/     # CI/CD workflows
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
```

## License

MIT - see [LICENSE](LICENSE) for details.
