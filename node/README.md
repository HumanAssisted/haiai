# haiai -- Node.js SDK

Node.js/TypeScript SDK for local JACS identity, signing and verification, plus admitted [HAI.AI](https://hai.ai) platform integrations. See the shared [capability boundaries](../README.md#capability-boundaries) for registration, active email and current Agreement/advocate/mediator limits, and [platform compatibility](../README.md#platform-compatibility) before making API calls.

## Install

```bash
npm install @haiai/haiai
```

### CLI and MCP Server

The `haiai` CLI binary and built-in MCP server are implemented in Rust. `npm install @haiai/haiai` includes the platform-specific Rust binary -- there is no separate Node CLI or MCP server.

```bash
# After npm install @haiai/haiai:
npx haiai init --name my-agent --register=false
npx haiai mcp    # Start MCP server (stdio transport)
```

See the [CLI README](../rust/haiai-cli/README.md) for full command and MCP tool documentation.

## Local quickstart

Follow the shared [local identity setup](../README.md#local-quickstart), then run this from the directory containing `jacs.config.json` with `JACS_PRIVATE_KEY_PASSWORD` set to its key password. It writes a disposable note, signs it in place (keeping a `.bak` copy), and checks the signature locally. No platform registration is needed.

```typescript
import { writeFile } from "node:fs/promises";
import { Agent } from "@haiai/haiai";

const agent = await Agent.fromConfig();
const client = agent.client;
await writeFile("sdk-note.md", "Hello from my agent.\n", "utf8");
await client.signText("sdk-note.md");
const result = await client.verifyText("sdk-note.md", { strict: true });
if (!result.signatures.length || result.signatures.some(s => s.status !== "valid")) {
  throw new Error(`Signature verification failed: ${JSON.stringify(result)}`);
}
console.log("valid");
```

Expected output: `valid`. The file-level `signed` status only means a signature was found; each signature must be `valid`. This proves agent provenance, not a person's approval of an Agreement.

## Caller-built request authentication

SDK API methods authenticate requests automatically. For your own HTTP call,
use `await client.buildRequestAuthHeader('POST', finalUrl, bodyBytes)` with a
`Buffer` or `Uint8Array`. Send those exact bytes to that URL without redirects,
and build a fresh header for each retry. The URL must match the configured HAI
origin. The old no-argument helper now returns a clear error.

The service audience defaults to `hai.ai`; set the client option
`requestAuthAudience` only when your API deployment uses another pinned
audience. It cannot be changed per request. Node only encodes bytes for FFI;
Rust/JACS owns the authentication policy and cryptography.

## Email

For admitted existing-identity registration, `HaiClient.register` accepts optional
`registrationKey`. See the shared [registration guidance](../README.md#admitted-registration-and-email).

Platform email requires admitted registration and server-returned email status `active`; an allocated or pending address cannot send. Inspect `agent.email.status()` for the actual address, status and limits. Quota, external-recipient and content gates still apply; see [capability boundaries](../README.md#capability-boundaries).

Signed email defaults to `html_inline_jacs`: the SDK renders safe HTML, embeds the signed inline logo and hidden JACS envelope, and adds the verify footer. Use `generationType: "attachment_jacs"` with `sendSignedEmail` only for compatibility with the older attachment transport. For now, signed email body input must be plain text; caller-supplied HTML and reserved HAI/JACS inline markers are rejected before signing.

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

```typescript
const raw = await client.getRawEmail("m.uuid");
if (!raw.available) throw new Error(raw.omittedReason ?? "unknown");
const result = await client.verifyEmail(raw.rawEmail!);
if (!result.valid) throw new Error("tampered or revoked");
```

Bytes are byte-identical to what JACS signed (25 MB cap).
Full recipe: [How verified email works](https://hai.ai/about/email).

## Framework Integration

```typescript
import {
  createJacsLangchainTools,      // LangChain tool creation
  langgraphWrapToolCall,          // LangGraph tool wrapper
  langgraphToolNode,              // LangGraph tool node
  createJacsMcpTransportProxy,    // MCP transport proxy
  getJacsMcpToolDefinitions,      // MCP tool definitions
  registerJacsMcpTools,           // Register MCP tools
  createAgentSdkToolWrapper,      // Agent SDK wrapper
} from "@haiai/haiai";
```

## A2A Integration

```typescript
import { getA2AIntegration, signArtifact, verifyArtifact, exportAgentCard } from "@haiai/haiai";

const a2a = await getA2AIntegration(jacsClient, { trustPolicy: "verified" });
const signed = await signArtifact(jacsClient, { taskId: "t-1", input: "hello" }, "task");
const verified = await verifyArtifact(jacsClient, signed);
```

## Dual Build

The package ships both ESM and CJS builds. `import` and `require` both work.

## Requirements

- Node.js 18+
- A JACS keypair (generated locally via `npx haiai init --name my-agent --register=false` or programmatically)

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

Apache-2.0 OR MIT
