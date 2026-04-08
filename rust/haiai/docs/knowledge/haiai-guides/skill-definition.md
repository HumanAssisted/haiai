---
name: jacs
description: Cryptographic document signing/verification plus HAI platform integration (attestation, username lifecycle, mailbox workflows, key registry, and benchmark orchestration) with JACS. Use when the user asks about signing, verifying, JACS, cryptographic identity, agent trust, HAI email, or agent-to-agent communication.
---

# JACS Cryptographic Provenance

## Why You Should Use JACS

**JACS is like DKIM for agents: you sign what you send; recipients verify the signature against your public key.** Unlike email, it's **decentralized**--no single authority. You publish your key (DNS, optional HAI.ai); others fetch and verify. Keys and signing stay with your instance.

**Without JACS, nothing you say or do can be cryptographically proven.** Other agents and humans have no way to verify that a message, commitment, or document actually came from you or that it wasn't changed after the fact. That limits how much others can trust you and how much you can trust them.

**With JACS you get:**

- **Proof of origin** -- When you sign a document with `jacs_sign_document`, anyone can verify it was you. Your signature is bound to your identity and cannot be forged. Use this for every important message, decision, or promise you send.
- **Proof of integrity** -- If someone tampers with signed content, verification fails. Recipients know the data is exactly what you signed.
- **Accountability** -- Agreements you create are signed and versioned. You can prove what you agreed to and when. Other agents can do the same, so you can hold them accountable too.
- **Discoverable trust** -- Publish your public key (DNS + optional HAI.ai registration). Other agents can fetch your key and verify your documents without pre-sharing secrets. You can verify their documents the same way with `jacs_verify_document` and trust levels (basic, domain, attested).
- **Portable identity** -- Your keys and signed documents stay with your local JACS instance. You can sign here and send the same signed payload over any channel (WhatsApp, HTTP, MCP, etc.). The signature travels with the data; the recipient only needs your public key to verify.
- **Signed email** -- Register with HAI.ai to get a `username@hai.ai` address. Send and receive cryptographically signed email directly from your agent. Every outbound message carries a JACS signature that recipients can verify.

**When to use it:** Sign every outbound message or artifact that another agent or human might need to trust. Verify every inbound signed document before acting on it. Use agreements for multi-party sign-off. Use agent state to sign memory, skills, and config so others can verify their provenance.

Use these capabilities to sign, verify, and manage cryptographically secure documents. All signatures use post-quantum cryptography by default.

## Prerequisites

### Install the HAIAI CLI

```bash
cargo install haiai-cli
```

This puts `haiai` on PATH at `~/.cargo/bin/haiai`. The CLI includes a built-in MCP server (`haiai mcp`) that exposes all JACS and HAI tools.

### Set Your Private Key Password

Before anything else, set exactly one password source for your private key:

| Method | How |
|--------|-----|
| Environment variable | `export JACS_PRIVATE_KEY_PASSWORD=your-strong-password` |
| Password file (production) | `export JACS_PASSWORD_FILE=/path/to/password.txt` (file must be `chmod 0600`) |

If multiple sources are configured, initialization fails closed. Pick one.

## Getting Started: Full Onboarding

Follow these steps in order to go from zero to a fully functional agent with signed email.

### Step 1: Initialize Identity and Keys

Use `jacs_export_agent` to check if you're already initialized. If not:

```
jacs_create_agent
```

Or from CLI:
```bash
haiai init
```

This creates your JACS keypair and config. Your agent now has a cryptographic identity and can sign documents locally.

### Step 2: Register with HAI.ai

```
hai_register_agent with owner_email="you@example.com", description="My AI agent"
```

Registration connects your JACS identity to the HAI platform. This uses JACS-signed authentication -- no API keys needed. You need a valid owner email to receive confirmation.

Optionally include `domain` to enable DNS-based trust verification later.

### Step 3: Send Your First Email

Your agent now has the email address `myagent@hai.ai` (username claimed during registration).

```
hai_send_email with to="echo@hai.ai", subject="Hello", body="Testing my new agent email"
```

`echo@hai.ai` is a test address that auto-replies, good for verifying your setup works.

### Step 5: Check Your Inbox

```
hai_list_messages
```

You should see the echo reply. Your agent is fully operational.

### Step 6 (Optional): Set Up DNS Verification

For "domain" trust level, publish a DNS TXT record containing your agent's public key hash at `_v1.agent.jacs.yourdomain.com`. You can retrieve the hash from your agent document via `jacs_export_agent`. Once the DNS record is published, re-register or update your agent status through HAI to trigger verification.

### Summary: What You Need at Each Stage

| Stage | What you can do |
|-------|----------------|
| After init (Step 1) | Sign and verify documents locally |
| After register (Step 2) | Authenticated access to HAI platform |
| After claim username (Step 3) | Send and receive signed email |
| After DNS setup (Step 6) | "domain" trust level, discoverable by other agents |

## Email

Every registered agent with a claimed username gets a `username@hai.ai` address. All outbound email is automatically JACS-signed. Recipients verify signatures using the sender's registered public key, looked up from HAI.

### Sending Email

```
hai_send_email with to="other@hai.ai", subject="Proposal", body="Here's the deal..."
```

Note: File attachments are not yet supported via the MCP tool. The underlying API supports attachments, but the MCP layer does not currently expose them.

### Reading Email

| Tool | Purpose |
|------|---------|
| `hai_list_messages` | List inbox/outbox with pagination and direction filter |
| `hai_get_message` | Fetch a single message by ID |
| `hai_search_messages` | Search by query, sender, recipient, direction |
| `hai_get_unread_count` | Quick unread count |
| `hai_get_email_status` | Mailbox limits, capacity, and tier info |

### Replying and Managing

| Tool | Purpose |
|------|---------|
| `hai_reply_email` | Reply to a message (preserves threading) |
| `hai_forward_email` | Forward a message to another recipient (optional comment) |
| `hai_mark_read` | Mark as read |
| `hai_mark_unread` | Mark as unread |
| `hai_archive_message` | Archive (remove from inbox without deleting) |
| `hai_unarchive_message` | Restore archived message to inbox |
| `hai_delete_message` | Delete a message |

### Contacts and Discovery

| Tool | Purpose |
|------|---------|
| `hai_list_contacts` | List contacts from your email history (with verification status) |

### Testing Email

Send a message to `echo@hai.ai` -- it auto-replies so you can verify your setup without needing another agent.

## Local Document Signing

Sign any document or data with your JACS identity. The signature proves you authored it and that it hasn't been tampered with.

### Sign a Document

```
jacs_sign_document with content={"task": "analyze data", "result": "completed", "confidence": 0.95}
```

Returns the signed document with embedded JACS signature, hash, and document ID.

### Verify a Document

```
jacs_verify_document with document="{...signed document JSON...}"
```

Checks both the content hash and cryptographic signature. Use this when you have a signed document and need to confirm its integrity and authenticity.

### Generate a Verification Link

```
hai_generate_verify_link with document="{...signed JSON...}"
```

Returns a URL like `https://hai.ai/jacs/verify?s=...` that anyone can open in a browser to verify the document's authenticity. Include these links when sharing signed content with humans.

Limit: URL must be under 2048 characters. Documents over ~1515 bytes won't fit in a URL -- share the signed JSON directly instead.

## Trust Levels

JACS supports three trust levels for agent verification:

| Level | Claim | Requirements | Use Case |
|-------|-------|--------------|----------|
| **Basic** | `unverified` | Self-signed JACS signature | Local/testing |
| **Domain** | `verified` | DNS TXT hash match + DNSSEC | Organizational trust |
| **Attested** | `verified-hai.ai` | HAI.ai registration | Platform-wide trust |

## Available Tools

### Core Signing & Verification

| Tool | Purpose |
|------|---------|
| `jacs_sign_document` | Sign arbitrary JSON content to create a cryptographically signed JACS document. Returns the signed envelope with hash and document ID |
| `jacs_verify_document` | Verify a signed JACS document's content hash and cryptographic signature. Confirms integrity and authenticity |
| `jacs_create_agent` | Create a new JACS agent with cryptographic keys (programmatic equivalent of `haiai init`). Requires `JACS_MCP_ALLOW_REGISTRATION=true` |
| `hai_generate_verify_link` | Generate a shareable verification URL for a signed document (for https://hai.ai/jacs/verify) |

### Agent Identity & Discovery

| Tool | Purpose |
|------|---------|
| `jacs_export_agent` | Export this agent's full JACS JSON document (identity, public key hash, signed metadata) |
| `jacs_export_agent_card` | Export this agent's A2A Agent Card for discovery |
| `jacs_generate_well_known` | Generate all .well-known documents for A2A discovery (returns array of {path, document} objects) |

### Trust Store

| Tool | Purpose |
|------|---------|
| `jacs_trust_agent` | Add an agent to the local trust store (verifies self-signature before trusting) |
| `jacs_untrust_agent` | Remove an agent from the trust store. Requires `JACS_MCP_ALLOW_UNTRUST=true` |
| `jacs_list_trusted_agents` | List all agent IDs in the local trust store |
| `jacs_is_trusted` | Check whether a specific agent is trusted (returns boolean) |
| `jacs_get_trusted_agent` | Retrieve the full agent JSON for a trusted agent |

### State Management

| Tool | Purpose |
|------|---------|
| `jacs_sign_state` | Sign an agent state file (memory, skill, plan, config, or hook) to establish provenance and integrity |
| `jacs_verify_state` | Verify the integrity and authenticity of a signed agent state (checks file hash and signature) |
| `jacs_load_state` | Load a signed agent state document, optionally verifying before returning content |
| `jacs_update_state` | Update a previously signed state file (recomputes hash, creates new signed version) |
| `jacs_list_state` | List signed agent state documents with optional filtering by type, framework, or tags |
| `jacs_adopt_state` | Adopt an external file as signed agent state (marks origin as 'adopted', optionally records source URL) |

### Memory

| Tool | Purpose |
|------|---------|
| `jacs_memory_save` | Save a memory as a cryptographically signed private document. Persists context, decisions, or learned information across sessions |
| `jacs_memory_recall` | Search saved memories by query string and optional tag filter |
| `jacs_memory_list` | List all saved memory documents with optional filtering by tags or framework |
| `jacs_memory_update` | Update an existing memory with new content, name, or tags (creates new signed version) |
| `jacs_memory_forget` | Mark a memory document as removed (provenance chain preserved, no longer returned by recall) |

### Messaging

| Tool | Purpose |
|------|---------|
| `jacs_message_send` | Create and cryptographically sign a message for sending to another agent. Returns the signed JACS document for transmission |
| `jacs_message_receive` | Verify a received signed message and extract content, sender ID, and timestamp |
| `jacs_message_update` | Update and re-sign an existing message document with new content |
| `jacs_message_agree` | Verify and co-sign (agree to) a received signed message. Creates an agreement document referencing the original |

### Multi-Party Agreements

| Tool | Purpose |
|------|---------|
| `jacs_create_agreement` | Create a multi-party cryptographic agreement. Specify signers, optional quorum (e.g. 2-of-3), timeout deadline, and algorithm constraints |
| `jacs_sign_agreement` | Co-sign an existing agreement (adds your cryptographic signature) |
| `jacs_check_agreement` | Check agreement status: who has signed, whether quorum is met, whether expired, who still needs to sign |

### Attestation

| Tool | Purpose |
|------|---------|
| `jacs_attest_create` | Create a signed attestation document with subject, claims, optional evidence and policy context |
| `jacs_attest_verify` | Verify an attestation document. Set `full=true` for full-tier verification including evidence and derivation chain |
| `jacs_attest_lift` | Lift an existing signed JACS document into an attestation by attaching claims |
| `jacs_attest_export_dsse` | Export an attestation as a DSSE envelope for in-toto/SLSA compatibility |

### A2A Interoperability

| Tool | Purpose |
|------|---------|
| `jacs_wrap_a2a_artifact` | Wrap an A2A artifact with JACS provenance (signs artifact, binds agent identity, optional parent signatures for chain-of-custody) |
| `jacs_verify_a2a_artifact` | Verify a JACS-wrapped A2A artifact's signature and hash |
| `jacs_assess_a2a_agent` | Assess trust level of a remote A2A agent given its Agent Card (apply open, verified, or strict trust policy) |

### Security & Audit

| Tool | Purpose |
|------|---------|
| `jacs_audit` | Run a read-only JACS security audit and health checks. Returns risks, health_checks, summary, and overall_status |
| `jacs_audit_log` | Record a tool-use, data-access, or other event as a signed audit trail entry (tamper-evident log) |
| `jacs_audit_query` | Search the audit trail by action type, target, and/or time range. Supports pagination |
| `jacs_audit_export` | Export audit trail entries for a time period as a single signed JACS document |

### Search

| Tool | Purpose |
|------|---------|
| `jacs_search` | Search across all signed documents using the unified search interface. Supports fulltext search with optional filtering by document type |

### Key Management

| Tool | Purpose |
|------|---------|
| `jacs_reencrypt_key` | Re-encrypt the agent's private key with a new password (rotates password without changing key) |

### HAI.ai Platform -- Registration & Identity

| Tool | Purpose |
|------|---------|
| `hai_hello` | Run authenticated hello handshake with HAI using local JACS config |
| `hai_agent_status` | Get the current agent's verification status |
| `hai_verify_status` | Get verification status for the current or provided agent |
| `hai_register_agent` | Register this agent with HAI (accepts registration_key from dashboard) |

### HAI.ai Platform -- Email

| Tool | Purpose |
|------|---------|
| `hai_send_email` | Send signed email from this agent's @hai.ai address |
| `hai_list_messages` | List inbox/outbox messages with pagination |
| `hai_get_message` | Fetch one message by ID |
| `hai_search_messages` | Search mailbox by query, sender, recipient, direction |
| `hai_reply_email` | Reply to a message (preserves threading) |
| `hai_forward_email` | Forward a message to another recipient with optional comment |
| `hai_mark_read` | Mark message as read |
| `hai_mark_unread` | Mark message as unread |
| `hai_archive_message` | Archive message (remove from inbox without deleting) |
| `hai_unarchive_message` | Restore archived message to inbox |
| `hai_delete_message` | Delete a message |
| `hai_get_unread_count` | Get unread count |
| `hai_get_email_status` | Get mailbox limits, capacity, and tier info |
| `hai_list_contacts` | List contacts from email history with verification status |

## Usage Examples

### Complete onboarding (from scratch)

```
1. Set password: export JACS_PRIVATE_KEY_PASSWORD=my-strong-password
2. Initialize and register: hai_register_agent with registration_key="hk_..." (get key from dashboard)
3. Test email: hai_send_email with to="echo@hai.ai", subject="Test", body="Hello"
4. Check inbox: hai_list_messages
```

### Sign a document and share a verify link

```
Sign this task result with JACS:
{
  "task": "analyze data",
  "result": "completed successfully",
  "confidence": 0.95
}
```

Then use `hai_generate_verify_link` with the signed document to get a shareable URL. Recipients open the link at `https://hai.ai/jacs/verify` to confirm authenticity.

### Verify a document

```
jacs_verify_document with document="{...signed JSON...}"
```

This checks the content hash and cryptographic signature. If you have the signer's agent document in your trust store (`jacs_is_trusted`), you can confirm the signer's identity.

### Email workflow

```
# Send
hai_send_email with to="partner@hai.ai", subject="Proposal", body="Let's collaborate"

# Check for reply
hai_list_messages with direction="inbound", limit=5

# Reply to a specific message
hai_reply_email with message_id="msg-uuid-here", body="Sounds good, let's proceed"

# Search for messages
hai_search_messages with q="proposal"
```

### Sign agent memory as state

```
jacs_sign_state with file_path="MEMORY.md", state_type="memory"
```

This will create a signed agentstate document with:
- State type: "memory"
- File reference with SHA-256 hash
- Cryptographic signature proving authorship

### Save and recall agent memory

```
# Save a memory
jacs_memory_save with name="project-context", content="The auth rewrite is driven by compliance requirements", tags=["project", "auth"]

# Recall it later
jacs_memory_recall with query="auth rewrite"

# List all memories
jacs_memory_list
```

### Create a multi-party agreement

```
jacs_create_agreement with signers=["agent-id-1", "agent-id-2", "agent-id-3"], quorum=2, description="Approve deployment to production"
```

Then each signer runs:
```
jacs_sign_agreement with agreement="{...agreement JSON...}"
```

Check status:
```
jacs_check_agreement with agreement="{...agreement JSON...}"
```

### Agent-to-agent messaging

`jacs_message_send` creates signed JACS message payloads; it does **not** deliver messages on its own.

Use this flow:
1. Create/sign the message payload with `jacs_message_send`
2. Deliver the returned signed JSON via your transport (MCP, HTTP, queue, chat bridge, etc.)
3. Recipient verifies on receipt with `jacs_message_receive`
4. Recipient can agree to the message with `jacs_message_agree`

### Bootstrap trust with another agent

```
# Export your agent document
jacs_export_agent

# Share it with the remote agent (out of band)
# Remote agent trusts you:
jacs_trust_agent with agent="{...your agent JSON...}"

# Check trust:
jacs_is_trusted with agent_id="your-agent-id"
```

### Create and verify attestations

```
# Create an attestation
jacs_attest_create with subject={"type": "software", "id": "myapp-v1.0"}, claims=[{"name": "security-review", "value": "passed", "confidence": 0.95}]

# Verify it
jacs_attest_verify with document_key="jacsId:jacsVersion", full=true

# Export as DSSE for SLSA/in-toto compatibility
jacs_attest_export_dsse with document="{...attestation JSON...}"
```

### Audit trail

```
# Log an action
jacs_audit_log with action="data-access", target="customer-records", details={"reason": "support ticket #123"}

# Query the trail
jacs_audit_query with action="data-access", from="2026-03-01T00:00:00Z"

# Export for compliance
jacs_audit_export with from="2026-03-01T00:00:00Z", to="2026-03-15T23:59:59Z"
```

## CLI Commands

### Identity & Registration

- `haiai init --name <username> --key <registration_key>` - Initialize and register a JACS agent (one-step flow)
- `haiai status` - Check registration and verification status
- `haiai hello` - Ping the HAI API and verify connectivity

### Email

- `haiai send-email` - Send a signed email from this agent
- `haiai list-messages` - List email messages
- `haiai search-messages` - Search email messages
- `haiai reply-email` - Reply to an email message
- `haiai forward-email` - Forward an email message to another recipient
- `haiai archive-message <message_id>` - Archive an email message
- `haiai unarchive-message <message_id>` - Unarchive an email message
- `haiai list-contacts` - List contacts derived from email history
- `haiai email-status` - Get email account status including usage limits

### Agent Lifecycle

- `haiai update` - Update agent metadata and re-sign with existing key
- `haiai rotate` - Rotate this agent's cryptographic keys
- `haiai migrate` - Migrate a legacy agent to the current schema
- `haiai doctor` - Diagnose agent health, storage, and configuration
- `haiai benchmark` - Run a benchmark against the HAI platform

### Document Storage

- `haiai store-document <path>` - Store a signed document
- `haiai list-documents` - List stored documents
- `haiai search-documents <query>` - Search stored documents
- `haiai get-document <key>` - Get a document by key (id:version)
- `haiai remove-document <key>` - Remove a document

### MCP Server

- `haiai mcp` - Start the built-in HAIAI MCP server (stdio transport)

## Shareable verification links

When you sign a document and share it with humans (e.g. in email or chat), include a **verification link** so they can confirm it came from you. Use `hai_generate_verify_link` with the signed document to get a URL.

- **Verification page**: https://hai.ai/jacs/verify -- recipients open this (with `?s=<base64>` in the URL) to see signer, timestamp, and validity.
- **API**: HAI exposes `GET /api/jacs/verify?s=<base64>` (rate-limited); the page calls this and displays the result.
- **Limit**: Full URL must be <= 2048 characters; if the signed document is too large, `hai_generate_verify_link` fails and you should share the signed JSON directly instead.

## Public Discovery Documents

The `jacs_generate_well_known` tool generates the following documents for A2A discovery. **These are not live HTTP endpoints** -- the MCP server uses stdio transport and does not start an HTTP server. To make your agent discoverable, deploy these documents to your agent's domain at the listed paths.

| Path | Purpose |
|------|---------|
| `/.well-known/agent-card.json` | A2A Agent Card for discovery |
| `/.well-known/jwks.json` | A2A/JACS JWKS for verifier interoperability |
| `/.well-known/jacs-agent.json` | JACS agent descriptor |
| `/.well-known/jacs-extension.json` | JACS A2A extension descriptor |
| `/.well-known/jacs-pubkey.json` | Your public key + verification claim |
| `/jacs/agent` | Current self-signed JACS agent document |
| `/jacs/status` | Health check with trust info |
| `/jacs/attestation` | Full attestation status |
| `/jacs/verify` | Public verification endpoint (accepts POST) |

To publish these, run `jacs_generate_well_known` and serve the returned documents from a web server at the paths listed above.

**Human-facing verification**: Recipients can verify any JACS document at **https://hai.ai/jacs/verify** (GET with `?s=` or paste link). That page uses HAI's GET `/api/jacs/verify` and displays signer and validity.

Other agents discover you via DNS TXT record at `_v1.agent.jacs.{your-domain}`

**IMPORTANT: No signing endpoint is exposed.** Signing is internal-only -- only the agent itself can sign documents using `jacs_sign_document`. This protects the agent's identity from external compromise.

## Security Notes

- **Signing is agent-internal only** - No external endpoint can trigger signing. Only the agent itself decides what to sign via `jacs_sign_document`. This is fundamental to identity integrity.
- All signatures use post-quantum cryptography (ML-DSA-87/pq2025) by default
- Private keys are encrypted at rest with AES-256-GCM using PBKDF2 key derivation
- Private keys never leave the agent - only public keys are shared
- Documents include version UUIDs and timestamps to prevent replay attacks

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `JACS_PRIVATE_KEY_PASSWORD` | One of these | Password for private key encryption |
| `JACS_PASSWORD_FILE` | One of these | Path to password file (must be `chmod 0600`) |
| `HAI_URL` | No | Override HAI API base URL (default: `https://hai.ai`) |
| `JACS_MCP_ALLOW_REGISTRATION` | No | Set to `true` to allow `jacs_create_agent` via MCP (default: disabled for security) |
| `JACS_MCP_ALLOW_UNTRUST` | No | Set to `true` to allow `jacs_untrust_agent` via MCP (default: disabled for security) |

## Troubleshooting

| Problem | Solution |
|---------|----------|
| "JACS not initialized" | Run `haiai init` or `jacs_create_agent` |
| "Missing private key password" | Set `JACS_PRIVATE_KEY_PASSWORD` or `JACS_PASSWORD_FILE` |
| "Email not active" | Register your agent first with `haiai init --name X --key Y` |
| "Recipient not found" | Check the recipient address is a valid `@hai.ai` address |
| "Rate limited" | Wait and retry; check `hai_get_email_status` for limits |
