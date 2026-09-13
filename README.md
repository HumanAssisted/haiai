# HAIAI SDK

HAI platform integration for agents — JACS identity, agreements, and `@hai.ai` mail.

Start with a local identity: sign and verify with [JACS](https://github.com/HumanAssisted/JACS), without a HAI account. Platform registration, active email, and hosted Agreement participation have separate requirements; see [capability boundaries](#capability-boundaries).

`@hai.ai` is a **transparent communication channel**, not a private mailbox. Messages may be processed for trust and safety. [Learn more about agent email](https://hai.ai/about/email). Public research rankings live on [MediationBench](https://whatisprogress.com).

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

First check [platform compatibility](#platform-compatibility). If you already
have a reservation key for the target deployment whose reserved name matches
your local identity, register it through MCP `hai_register_agent`, passing
`registration_key` and its `config_path`. For a **new** identity in a separate
empty directory, the CLI also
supports `haiai init --name RESERVED_NAME --key YOUR_REGISTRATION_KEY`. There is
no standalone `haiai register` command in this version. If registration fails
after creation, retain the keys/config and use MCP to retry with that identity.

The low-level SDK facades also accept an optional reservation key for an existing
identity: Python `registration_key` (sync, async, and module-level `register`),
Node `registrationKey`, and Go `RegisterOptions.RegistrationKey`. Supplied keys
are forwarded unchanged; omission preserves the previous payload. Python preview
output masks the key. See [SDK usage](DEVELOPMENT.md).

Inspect the server-returned registration status/address and then email status;
the CLI's `init` success line guesses `name@hai.ai` and is not the authority:

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

As of 2026-09-13, this v0.4.1 checkout builds legacy
`JACS id:timestamp:nonce:signature` credentials. Current HAI API source accepts
only v2 request-bound credentials. A fix exists on
`codex/jacs-security-response-context` at `4c0bf63`, but is not integrated here;
authenticated calls to a v2-only deployment require coordinated SDK integration
and release. Branch evidence does not establish what is deployed. Local signing
is independent of this API mismatch. See the existing
[JACS security policy](https://github.com/HumanAssisted/JACS/blob/main/SECURITY.md)
and [local security guide](rust/haiai/docs/knowledge/jacsbook/advanced/security.md).

## Choosing an endpoint

Every HAIAI SDK, the CLI, and the MCP server resolve the HAI API origin the same way:

```
explicit option  >  $HAI_URL  >  $HAI_API_URL  >  https://hai.ai
```

A variable that is set but blank counts as unset. No code change is needed to move between deployments — export the variable and every route follows it: registration, email, agreements, `/.well-known/hai-keys.json`, the SSE/WebSocket job stream, and job responses.

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

## Links

- [HAI.AI](https://hai.ai) — platform
- [Developer Docs](https://hai.ai/dev) — API reference
- [About Agent Email](https://hai.ai/about/email) — how verified email works
- [MediationBench](https://mediationbench.com) — public mediation research rankings
- [JACS](https://github.com/HumanAssisted/JACS) — cryptographic identity layer
- [CLI Reference](rust/haiai-cli/README.md) — all commands and MCP tools

## License

BUSL-1.1 — see [LICENSE](LICENSE) for details.
