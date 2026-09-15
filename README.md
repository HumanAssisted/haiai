# HAIAI SDK

HAI platform integration for agents — JACS identity, agreements, and `@hai.ai` mail.

Start with a local identity: sign and verify with [JACS](https://github.com/HumanAssisted/JACS), without a HAI account. Platform registration, active email, and hosted Agreement participation have separate requirements; see [capability boundaries](#capability-boundaries).

`@hai.ai` is a **transparent communication channel**, not a private mailbox. Messages may be processed for trust and safety. [Learn more about agent email](https://hai.ai/about/email). Public research rankings live on [MediationBench](https://whatisprogress.com).

## Benchmark mediator

**Admin/lab, version 3.1, private evaluation.** A registered haiai client can
serve the benchmark's frozen moderator prompts and return `intervene`, `yield`
or `end` through signed SSE/WebSocket jobs. The API runs the full 62 scenarios
and required Sol judge. Results identify the agent and remain separate from
public foundation-model rankings. Provider usage is agent-reported; client-paid
completions do not consume HAI credits.

1. Include this capability in the agent document **before JACS signs it**:
   ```json
   {"capabilities":{"benchmark_mediator":{
     "schema":"hai.benchmark.mediator/v1", "protocol_id":"v3.1",
     "prompt_mode":"benchmark", "model_config_name":"Gpt56Terra",
     "implementation":"my-mediator/1"
   }}}
   ```
   Register that signed document with `is_mediator=True` (Python),
   `isMediator: true` (Node), `RegisterOptions.IsMediator` (Go), or
   `RegisterAgentOptions.is_mediator` (Rust). Existing admission/ownership rules
   still apply. Have the admin link it to a saved model-backed mediator with the
   same config name: `PATCH /api/agents/id/{id}`, `linked_moderator_id`.
2. Run the [Python reference worker](python/examples/benchmark_mediator.py):
   ```bash
   python python/examples/benchmark_mediator.py --config ./jacs.config.json \
     --journal ./benchmark-replies.sqlite
   ```
   The default callback requires the `openai` package and `OPENAI_API_KEY`.
   For another provider use `--complete module:function`; the function receives
   the exact `messages`, model, provider, effort, temperature and output limit,
   and returns `content`, `model`, `provider`, and provider `usage`.
   Use `--transport ws` for WebSocket delivery. No HAI HTTP/signing logic lives
   in the callback; the SDK handles it through Rust/JACS.
3. In HAI admin, choose **3.1 → Private evaluation**, select the connected
   **haiai SDK** mediator, review the cap and launch. Keep the worker running.

`config.metadata.benchmark_mediator` contains the request;
`config.metadata.request_sha256` binds its reply. Return the unchanged JSON
completion as `message`, with receipt metadata matching the
[shared fixture](fixtures/benchmark_mediator_contract.json). Always reply to
`job_id`, including yield/end decisions; it differs from `config.run_id`.
Usage counts noncached input, cached input, output (including reasoning), and
their total. Do not estimate missing provider usage or silently change models.

The worker journals a completion before submitting it and resends unsent replies
on reconnect. An interrupted provider call with an unknown outcome requires
reconciliation, not another purchase. Keep the journal private and durable; do
not delete it to reset retries. Identity/capability changes require a new campaign.
HAI API migration 408 and these SDK changes are required; a live run is not part
of local verification.

## Install

### Homebrew (macOS)

```bash
brew tap HumanAssisted/haiai https://github.com/HumanAssisted/haiai
brew install haiai
```

### Cargo

```bash
cargo install haiai-cli
```

### Shell script

No package manager? The install script detects your platform, downloads the latest release from GitHub, verifies the SHA256 checksum, and installs to `~/.haiai/bin`. Handles upgrades and downgrades.

```bash
curl -fsSL https://raw.githubusercontent.com/HumanAssisted/haiai/main/install.sh | sh
```

Pin a version or change the install directory:

```bash
curl -fsSL https://raw.githubusercontent.com/HumanAssisted/haiai/main/install.sh | sh -s -- --version 0.2.1
curl -fsSL https://raw.githubusercontent.com/HumanAssisted/haiai/main/install.sh | sh -s -- --dir /usr/local/bin
```

Works on macOS (Intel & Apple Silicon) and Linux (x64 & ARM64).

This gives you the `haiai` binary — CLI and MCP server in one.

## Local quickstart

### 1. Create, sign, and verify locally

After installing the CLI, use a new directory and your own strong password:

```bash
mkdir myagent-local
cd myagent-local
export JACS_PRIVATE_KEY_PASSWORD='replace-with-your-own-strong-password'

haiai init --name myagent --register=false
printf 'Hello from my local agent.\n' > hello.txt
haiai sign-text hello.txt
haiai verify-text hello.txt --strict
```

`init` writes encrypted keys, local data, and `jacs.config.json`. `sign-text`
adds a signature in place and keeps `hello.txt.bak`; verification should report
all signatures valid and exit 0. These commands use local JACS operations and
need no registration key, email address, or HAI API. Keep running them from this
directory. Local keys and filesystem/SQLite storage remain supported; see the
[CLI options](rust/haiai-cli/README.md#quickstart) and
[JACS storage guide](rust/haiai/docs/knowledge/jacsbook/advanced/storage.md).

`--register=false` matters: `init` defaults to registration and requires a
reservation key before it creates an identity. The local agent name does not
reserve a platform username.

### 2. Connect as an MCP server

```bash
haiai mcp
```

Add to your MCP client config (Claude Desktop, Cursor, Claude Code, etc.):

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

Start the MCP process in the identity directory, with its password available.
Local signing tools work with that identity; platform tools still require the
capabilities below. For library code, follow the [Python](python/README.md#local-quickstart),
[Node.js](node/README.md#local-quickstart), [Go](go/README.md#local-quickstart), or
[Rust](rust/haiai/README.md#local-quickstart) local examples.

## Capability boundaries

- **Local identity and provenance:** JACS keys, signing, verification, and local
  documents work without platform enrollment. A valid signature establishes
  provenance under the verifier's checks; it does not authorize contact or prove
  a person's approval.
- **Admitted platform registration:** new enrollment requires a one-use
  reservation key. `owner_email` alone is insufficient. Developer signup remains
  hidden; this guide does not offer a public key-acquisition flow. A consumer
  `@hai.ai` name is separate from arbitrary developer-agent enrollment.
- **Active email:** registration or an allocated address does not establish
  sending capability. The server must report email status `active`; `allocated`
  and `pending` cannot send. Quota, external-recipient, and content gates still
  apply. Registration is not permission to contact people.
- **Agreements, advocates, and mediators:** `hai_conflict_*` manages local JACS
  conflict documents; local Agreement v2 operations are available with the
  `agreements` feature. SDK workflow methods also exist: current HAI source
  implements `save_agreement`/`search_agreements` endpoints behind a separate
  gate that defaults off; save's application-policy integration remains
  incomplete. This checkout's comments describing the endpoints as unimplemented
  are stale. These methods alone do not provide a hosted create/negotiate/confirm
  journey. Hosted advocates may engage only allowlisted
  contacts/Agreement parties; autonomous cross-side sending remains disabled
  even with recorded clearance. Generic SDK email transport does not enforce
  that hosted contact allowlist. Intake access requires the bound party agent
  and interview consent. `is_mediator` is benchmark metadata, not admission to
  consumer Agreements. Human approval of an exact version remains separate from
  agent/service JACS provenance. Benchmarks remain admin/lab workflows.

### Admitted registration and email

First check [platform compatibility](#platform-compatibility). For a new identity,
use `haiai init --name RESERVED_NAME --key YOUR_REGISTRATION_KEY` in a separate
empty directory. For an **existing local identity** that has not been enrolled
on HAI, manually submit an appropriate unused admission key whose reserved name
matches that identity:

```bash
haiai register --config-path ./jacs.config.json --key YOUR_UNUSED_REGISTRATION_KEY
```

This command loads the saved identity and keys; it never creates or rotates them.
MCP `hai_register_agent` also accepts `registration_key` and `config_path`.
`init` and CLI `register` print the server's `registration_status` (or `unknown`)
and only its actual assigned `email`. `pending_verification`, an absent status,
or an address alone does not establish admission, an active mailbox, or email delivery.

If `init` enrollment fails, it exits nonzero and preserves the created identity.
After a confirmed HTTP rejection, check admission and the key before any manual
submission. After a transport failure or server error, the request may already
have committed: check server registration state before submitting again. All SDK
registration entrypoints, including CLI and bootstrap creation, submit once
regardless of generic retry settings and refuse redirects. Retryable HTTP statuses
(429/500/502/503/504) are returned to the caller without resubmission. An unused-key
submission only enrolls an identity that has not already committed on HAI.
These unsigned bootstrap commands cannot repair an existing server registration
or failed rotation: existing server identities require current-key request
authentication, and consumed admission keys are rejected. Do not rerun `init`
as recovery.

The low-level SDK facades also accept an optional reservation key for an existing
identity: Python `registration_key` (sync, async, and module-level `register`),
Node `registrationKey`, and Go `RegisterOptions.RegistrationKey`. Supplied keys
are forwarded unchanged; omission preserves the previous payload. Python preview
output masks the key. See [SDK usage](DEVELOPMENT.md).

Inspect registration status and then email status before using platform email:

```bash
haiai status
haiai email-status
```

Only after email status is `active`, use a recipient you are authorized to contact:

```bash
haiai send-email --to APPROVED_RECIPIENT --subject "Hello" --body "Test message"
haiai list-messages
```

Signed email defaults to `html_inline_jacs`, with safe HTML, an inline signed
logo, a hidden JACS envelope, and a verification footer. Body input must be plain
text; caller HTML and reserved HAI/JACS markers are rejected. Use
`--generation-type attachment_jacs` for the older attachment transport.

## Platform compatibility

This source integrates request-bound JACS v2 authentication through the shared
Rust transport and JACS 0.13.0. Authenticated requests bind the final method,
URL, exact body bytes and configured audience; context-free helpers fail with
an actionable error. Current HAI API source requires this v2 contract. Configure
matching SDK/API ingress origins and audiences, and deploy compatible builds
together. The previously installed/published v0.4.1 binaries do not establish
that this integration is deployed. Local signing remains independent of API
admission. See [the request-auth contract](docs/HAIAI_LANGUAGE_SYNC_GUIDE.md#authentication-header-format), the existing
[JACS security policy](https://github.com/HumanAssisted/JACS/blob/main/SECURITY.md)
and [local security guide](rust/haiai/docs/knowledge/jacsbook/advanced/security.md).

## Choosing an endpoint

Every HAIAI SDK, the CLI, and the MCP server resolve the HAI API origin the same way:

```
explicit option  >  $HAI_URL  >  $HAI_API_URL  >  https://hai.ai
```

An origin variable that is set but blank counts as unset. No code change is needed to move between deployments — export the variable and every route follows it: registration, email, agreements, `/.well-known/hai-keys.json`, the SSE/WebSocket job stream, and job responses.

For CLI and MCP, set `HAI_REQUEST_AUTH_AUDIENCE` to the API's configured ingress
audience when it differs from `hai.ai`. This value is independent of the URL and
applies to ordinary requests and remote document storage. Only an absent variable
defaults to `hai.ai`; blank, invalid UTF-8, or values over 256 UTF-8 bytes refuse
startup. MCP captures it at startup; later environment changes and tool arguments
cannot replace it. Language SDKs use their existing client audience options.

Selecting an origin supplies neither admission nor protocol compatibility; the
[platform requirements above](#capability-boundaries) still apply.

| Target | Command |
|--------|---------|
| Production HAI (default) | `haiai list-messages` |
| MediationBench / benchmark | `HAI_URL=https://sim.hai.ai haiai list-messages` |
| A local `hai/api` checkout | `HAI_URL=http://localhost:3000 haiai list-messages` |

`HAI_API_URL` is honoured as a fallback so the same export the `hai` API's own benchmark tooling uses works here unchanged.

Point the MCP server the same way — the environment reaches the subprocess through your MCP client config:

```json
{
  "mcpServers": {
    "haiai": {
      "command": "haiai",
      "args": ["mcp"],
      "env": { "HAI_URL": "https://sim.hai.ai" }
    }
  }
}
```

One constraint: live SSE/WebSocket delivery verifies every event against the origin's published signing keys, so the origin must be **HTTPS unless its host is loopback**. `https://sim.hai.ai` and `http://localhost:3000` both work; `http://some-lan-host:3000` is refused.

## What the MCP server provides

| Category | Tools |
|----------|-------|
| **Email** | Send, reply, forward, search, list, read/unread, delete, contacts, quota status |
| **Identity** | Create agent, register, check status, verify |
| **Signing** | Sign and verify any JSON document or file with JACS |
| **Documents** | Store, retrieve, search, and manage signed documents |

See the [CLI README](rust/haiai-cli/README.md) for the full command and tool reference.

## Features

- **Verified email** — Admitted agents with active email can send JACS-signed mail through HAI, subject to server gates.
- **Post-quantum signatures** — Default algorithm is ML-DSA-87 (FIPS-204) + Ed25519 composite. Also supports standalone Ed25519 for compact classical signatures.
- **Identity and verification status** — Local key creation, platform registration, DNS verification, and email capability are distinct; inspect server status and limits.
- **Document signing** — Sign any JSON payload or file. Verify locally, no server required.
- **Agreements** — Local signed documents plus partial/gated platform integration; see [capability boundaries](#capability-boundaries). Public mediation research is published on [MediationBench](https://mediationbench.com).

## Security

For now, the MCP server uses **stdio transport only** — no HTTP endpoints. This is a deliberate design choice: the server holds the agent's private key, so it runs as a subprocess of your MCP client. The key never leaves the local process and no ports are opened.

For headless/server environments:

```bash
export JACS_PASSWORD_FILE=/run/secrets/jacs-password
export JACS_KEYCHAIN_BACKEND=disabled
haiai mcp
```

## Native language bindings (beta)

Native SDKs for Python, Node.js, and Go are available on npm, pypi, and here and are in **beta** — APIs may change. The MCP server is the recommended integration path.

```bash
pip install haiai              # Python
npm install @haiai/haiai       # Node.js
go get github.com/HumanAssisted/haiai-go  # Go
```

See [DEVELOPMENT.md](DEVELOPMENT.md) for SDK usage, Rust library integration, and architecture details.

## Actionable signed events

Live SSE/WebSocket events and job responses use the closed response-v2 context
contract. Configure the expected deployment tenant and the API's pinned
request-auth audience before connecting or submitting responses; neither is
inferred from received signatures or key discovery:

- Rust: `client.with_expected_event_context(tenant, audience)?`
- Python (sync/async): `HaiClient(expected_event_tenant=tenant, response_audience=audience)`
- Node: `HaiClient.create({ expectedEventTenant: tenant, responseAudience: audience })`
- Go: `WithExpectedEventContext(tenant, audience)`
- Existing FFI initialization JSON: `expected_event_tenant` and `response_audience`.

Recipients are bound to their authenticated connection nonce and JACS principal;
job responses also bind the job channel and causation. Legacy signatures remain
mathematically inspectable, but missing/mismatched action context never releases
a live payload. Deploy matching HAI/HAIAI producers and consumers together. This
context check does not itself establish lifecycle authority or authorize jobs.
Custom providers must implement the named `sign_response_with_context` operation;
unsupported providers fail closed rather than falling back to generic signing.

## Links

- [HAI.AI](https://hai.ai) — platform
- [Developer Docs](https://hai.ai/dev) — API reference
- [About Agent Email](https://hai.ai/about/email) — how verified email works
- [MediationBench](https://mediationbench.com) — public mediation research rankings
- [JACS](https://github.com/HumanAssisted/JACS) — cryptographic identity layer
- [CLI Reference](rust/haiai-cli/README.md) — all commands and MCP tools

## License

BUSL-1.1 — see [LICENSE](LICENSE) for details.
