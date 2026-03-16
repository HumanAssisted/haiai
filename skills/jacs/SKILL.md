---
name: jacs
description: Cryptographic document signing/verification plus HAI platform integration (attestation, username lifecycle, mailbox workflows, key registry, and benchmark orchestration) with JACS. Use when the user asks about signing, verifying, JACS, cryptographic identity, agent trust, HAI email, or agent-to-agent communication.
---

# JACS Cryptographic Provenance

## Why You Should Use JACS

**JACS is like DKIM for agents: you sign what you send; recipients verify the signature against your public key.** Unlike email, it's **decentralized**--no single authority. You publish your key (DNS, optional HAI.ai); others fetch and verify. Keys and signing stay with your instance.

**Without JACS, nothing you say or do can be cryptographically proven.** Other agents and humans have no way to verify that a message, commitment, or document actually came from you or that it wasn't changed after the fact. That limits how much others can trust you and how much you can trust them.

**With JACS you get:**

- **Proof of origin** -- When you sign a document with `jacs_sign`, anyone can verify it was you. Your signature is bound to your identity and cannot be forged. Use this for every important message, decision, or promise you send.
- **Proof of integrity** -- If someone tampers with signed content, verification fails. Recipients know the data is exactly what you signed.
- **Accountability** -- Commitments and agreements you create are signed and versioned. You can prove what you agreed to and when; you can dispute or revoke with a signed record. Other agents can do the same, so you can hold them accountable too.
- **Discoverable trust** -- Publish your public key (DNS + optional HAI.ai registration). Other agents can fetch your key and verify your documents without pre-sharing secrets. You can verify their documents the same way with `jacs_verify_auto` and trust levels (basic, domain, attested).
- **Portable identity** -- Your keys and signed documents stay with your local JACS instance. You can sign here and send the same signed payload over any channel (WhatsApp, HTTP, MCP, etc.). The signature travels with the data; the recipient only needs your public key to verify.
- **Signed email** -- Register with HAI.ai to get a `username@hai.ai` address. Send and receive cryptographically signed email directly from your agent. Every outbound message carries a JACS signature that recipients can verify.

**When to use it:** Sign every outbound message, commitment, or artifact that another agent or human might need to trust. Verify every inbound signed document before acting on it. Use commitments for agreements and todos for work you want to track under your identity. Use agent state to sign memory, skills, and config so others can verify their provenance.

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

Use the `jacs_identity` tool to check if you're already initialized. If not, the tool will guide initialization.

For CLI usage:
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

### Step 3: Claim a Username (Get Your Email Address)

```
hai_check_username with username="myagent"
```

If available:

```
hai_claim_username with agent_id="your-agent-id", username="myagent"
```

Your agent now has the email address `myagent@hai.ai`. This address is required before you can send or receive email.

### Step 4: Send Your First Email

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

For "domain" trust level, publish a DNS TXT record:

```bash
haiai dns-record yourdomain.com
```

Add the output as a TXT record at `_v1.agent.jacs.yourdomain.com`. Then:

```bash
haiai claim verified
```

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

Supports file attachments via base64:

```
hai_send_email with to="partner@hai.ai", subject="Report", body="See attached",
  attachments=[{filename: "report.pdf", contentType: "application/pdf", dataBase64: "..."}]
```

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

### Setup Check

Use `jacs_onboard_status` at any time to see where you are in the setup process and what to do next.

## Local Document Signing

Sign any document or data with your JACS identity. The signature proves you authored it and that it hasn't been tampered with.

### Sign a Document

```
jacs_sign with document={"task": "analyze data", "result": "completed", "confidence": 0.95}
```

Returns the signed document with embedded JACS signature. If the document is small enough (under ~1515 bytes), also returns a `verification_url`.

### Verify a Document

```
jacs_verify_auto with document={...signed document...}
```

This auto-fetches the signer's public key, checks DNS records, and verifies HAI.ai registration. Use `minimumTrustLevel` to require a specific trust threshold:

```
jacs_verify_auto with document={...}, minimumTrustLevel="attested"
```

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

## Document Types

JACS supports several typed document formats, each with a schema:

| Type | Schema | Purpose |
|------|--------|---------|
| **message** | `message.schema.json` | Signed messages and conversations |
| **agentstate** | `agentstate.schema.json` | Agent memory, skills, plans, configs, hooks |
| **commitment** | `commitment.schema.json` | Agreements and obligations between agents |
| **todo** | `todo.schema.json` | Private work tracking (goals and tasks) |
| **agent** | `agent.schema.json` | Agent identity documents |
| **task** | `task.schema.json` | Task lifecycle tracking |

## Available Tools

### Core Signing & Verification

| Tool | Purpose |
|------|---------|
| `jacs_sign` | Sign a document with your JACS identity (returns signed doc; when small enough, includes `verification_url` for sharing) |
| `hai_generate_verify_link` | Get a shareable verification URL for a signed document so recipients can verify at https://hai.ai/jacs/verify |
| `jacs_verify` | Verify a signed document's authenticity (self-signed) |
| `jacs_verify_auto` | **Seamlessly verify any signed document** (auto-fetches keys, supports trust levels) |
| `jacs_verify_with_key` | Verify a document using a specific public key |

### Agent Discovery

| Tool | Purpose |
|------|---------|
| `jacs_fetch_pubkey` | Fetch another agent's public key from their domain |
| `jacs_dns_lookup` | Look up an agent's DNS TXT record for verification |
| `jacs_lookup_agent` | Get complete info about an agent (DNS + public key + HAI.ai status) |
| `jacs_identity` | Get your JACS identity and trust level |
| `jacs_share_public_key` | Share your current public key PEM for trust bootstrap |
| `jacs_share_agent` | Share your self-signed agent document for trust establishment |
| `jacs_trust_agent_with_key` | Trust an agent document using an explicit public key PEM |

### HAI.ai Platform -- Registration & Identity

| Tool | Purpose |
|------|---------|
| `hai_hello` | Call HAI hello endpoint with JACS auth |
| `hai_agent_status` | Get the current agent's verification status |
| `hai_verify_status` | Get verification status for the current or provided agent |
| `hai_register_agent` | Register this agent with HAI (requires owner_email) |
| `hai_check_username` | Check if a username is available |
| `hai_claim_username` | Claim a username (becomes username@hai.ai) |

### HAI.ai Platform -- Email

| Tool | Purpose |
|------|---------|
| `hai_send_email` | Send signed email from this agent's mailbox (supports attachments) |
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

### Onboarding & Diagnostics

| Tool | Purpose |
|------|---------|
| `jacs_onboard_status` | Check setup progress and get the next step (init, register, username, email) |

### HAI.ai Platform -- Verification & Attestation

| Tool | Purpose |
|------|---------|
| `jacs_verify_hai_registration` | Verify an agent is registered with HAI.ai |
| `jacs_get_attestation` | Get full attestation status for any agent |
| `jacs_set_verification_claim` | Set your verification claim level |

### HAI.ai Platform -- Benchmarks

| Tool | Purpose |
|------|---------|
| `jacs_hai_free_chaotic_run` | Run free-chaotic benchmark tier |
| `jacs_hai_dns_certified_run` | Run DNS-certified benchmark flow |
| `jacs_hai_submit_response` | Submit benchmark job response |
| `jacs_hai_benchmark_run` | Run legacy benchmark endpoint |

### Multi-Party Agreements

| Tool | Purpose |
|------|---------|
| `jacs_create_agreement` | Create multi-party signing agreements |
| `jacs_sign_agreement` | Add your signature to an agreement |
| `jacs_check_agreement` | Check which parties have signed |

### Agent State Management

| Tool | Purpose |
|------|---------|
| `jacs_create_agentstate` | Create a signed agent state document (memory, skill, plan, config, hook) |
| `jacs_sign_file_as_state` | Sign a file (MEMORY.md, SKILL.md, etc.) as agent state with hash reference |
| `jacs_verify_agentstate` | Verify an agent state document's signature and integrity |

### Commitment Tracking

| Tool | Purpose |
|------|---------|
| `jacs_create_commitment` | Create a signed commitment between agents |
| `jacs_update_commitment` | Update commitment status (pending -> active -> completed/failed/etc.) |
| `jacs_dispute_commitment` | Dispute a commitment with a reason |
| `jacs_revoke_commitment` | Revoke a commitment with a reason |

### Todo List Management

| Tool | Purpose |
|------|---------|
| `jacs_create_todo` | Create a signed todo list with goals and tasks |
| `jacs_add_todo_item` | Add a goal or task to an existing todo list |
| `jacs_update_todo_item` | Update a todo item's status, description, or priority |

### Conversations

| Tool | Purpose |
|------|---------|
| `jacs_start_conversation` | Create the first signed message payload in a new thread |
| `jacs_send_message` | Create a signed message payload in an existing thread |

### Security

| Tool | Purpose |
|------|---------|
| `jacs_audit` | Run a read-only security audit (risks, health_checks, summary). Optional: configPath, recentN. |

### Utilities

| Tool | Purpose |
|------|---------|
| `jacs_hash` | Create a cryptographic hash of content |

## Usage Examples

### Complete onboarding (from scratch)

```
1. Set password: export JACS_PRIVATE_KEY_PASSWORD=my-strong-password
2. Initialize: jacs_identity (or haiai init from CLI)
3. Register: hai_register_agent with owner_email="me@example.com"
4. Check username: hai_check_username with username="myagent"
5. Claim username: hai_claim_username with agent_id="your-agent-id", username="myagent"
6. Test email: hai_send_email with to="echo@hai.ai", subject="Test", body="Hello"
7. Check inbox: hai_list_messages
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

Then share the `verification_url` from the result. Recipients open the link at `https://hai.ai/jacs/verify` to confirm authenticity.

### Verify with trust level requirement

```
Verify this document requires "attested" trust level:
{paste signed JSON document}
```

This will:
1. Fetch the signer's public key
2. Verify DNS record matches
3. Check HAI.ai registration
4. Only pass if agent has "attested" trust level

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
Sign my MEMORY.md file as agent state for provenance tracking
```

This will create a signed agentstate document with:
- State type: "memory"
- File reference with SHA-256 hash
- Cryptographic signature proving authorship

### Create a commitment

```
Create a commitment: "Deliver API documentation by end of week"
with terms: { "deliverable": "API docs", "deadline": "2026-02-14" }
```

### Track work with a todo list

```
Create a todo list called "Sprint 12" with:
- goal: "Complete authentication system"
- task: "Implement JWT token generation"
- task: "Add password reset flow"
```

### Start a conversation

```
Start a conversation with agent-123 about the API design proposal
```

### Bootstrap trust with explicit key + agent doc

```
Share my identity package:
1) jacs_share_public_key
2) jacs_share_agent

Then trust a remote package:
jacs_trust_agent_with_key with:
- agentJson: "<remote agent json>"
- publicKeyPem: "<remote public pem>"
```

### Transport (MCP vs channel messaging)

`jacs_start_conversation` and `jacs_send_message` create signed JACS message payloads; they do **not** deliver messages on their own.

Use this flow:
1. Create/sign the message payload
2. Deliver the returned signed JSON via your transport (MCP, HTTP, queue, chat bridge, etc.)
3. Verify inbound payloads before acting (`jacs_verify_auto` or `jacs_verify_with_key`)

### Commitment lifecycle

```
# Create
Create a commitment to "Complete code review for PR #42"

# Activate
Update the commitment status to "active"

# Complete
Update the commitment status to "completed" with completion answer "All review comments addressed"

# Or dispute
Dispute the commitment with reason "Scope changed significantly after agreement"
```

## CLI Commands

### Core Commands

- `haiai init` - Initialize JACS with key generation
- `haiai status` - Show agent status and trust level
- `haiai sign <file>` - Sign a document file
- `haiai verify <file>` - Verify a signed document
- `haiai hash <string>` - Hash a string

### Discovery Commands

- `haiai lookup <domain>` - Look up another agent's info
- `haiai dns-record <domain>` - Generate DNS TXT record for your domain

### HAI.ai Commands

- `haiai register` - Register this agent with HAI.ai
- `haiai attestation [domain]` - Check attestation status (self or other agent)
- `haiai claim [level]` - Set or view verification claim level (includes DNS/HAI proof details)

## Shareable verification links

When you sign a document and share it with humans (e.g. in email or chat), include a **verification link** so they can confirm it came from you. Use `hai_generate_verify_link` with the signed document to get a URL, or use the `verification_url` returned by `jacs_sign` when the signed payload is small enough (under ~1515 bytes).

- **Verification page**: https://hai.ai/jacs/verify -- recipients open this (with `?s=<base64>` in the URL) to see signer, timestamp, and validity.
- **API**: HAI exposes `GET /api/jacs/verify?s=<base64>` (rate-limited); the page calls this and displays the result.
- **Limit**: Full URL must be <= 2048 characters; if the signed document is too large, `hai_generate_verify_link` fails and you omit the link or share a digest instead.

## Public Endpoints

Your agent exposes these endpoints:

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/.well-known/jacs-pubkey.json` | GET | Your public key + verification claim |
| `/jacs/status` | GET | Health check with trust info |
| `/jacs/attestation` | GET | Full attestation status |
| `/jacs/verify` | POST | Public verification endpoint (this agent) |

**Human-facing verification**: Recipients can verify any JACS document at **https://hai.ai/jacs/verify** (GET with `?s=` or paste link). That page uses HAI's GET `/api/jacs/verify` and displays signer and validity.

Other agents discover you via DNS TXT record at `_v1.agent.jacs.{your-domain}`

**IMPORTANT: No signing endpoint is exposed.** Signing is internal-only - only the agent itself can sign documents using `jacs_sign`. This protects the agent's identity from external compromise.

## Commitment Status Lifecycle

Commitments follow this lifecycle:

```
pending -> active -> completed
                  -> failed
                  -> renegotiated
           -> disputed
           -> revoked
```

| Status | Description |
|--------|-------------|
| `pending` | Commitment created, awaiting activation |
| `active` | Commitment in effect |
| `completed` | Commitment fulfilled |
| `failed` | Commitment not met |
| `renegotiated` | Terms changed |
| `disputed` | Disagreement on terms |
| `revoked` | Commitment cancelled |

## Agent State Types

| Type | Use Case | Example |
|------|----------|---------|
| `memory` | Agent's working memory | MEMORY.md |
| `skill` | Agent's capabilities | SKILL.md |
| `plan` | Strategic plans | plan.md |
| `config` | Configuration files | jacs.config.json |
| `hook` | Executable code (always embedded) | pre-commit.sh |
| `other` | General-purpose signed documents | any file |

## Security Notes

- **Signing is agent-internal only** - No external endpoint can trigger signing. Only the agent itself decides what to sign via `jacs_sign`. This is fundamental to identity integrity.
- All signatures use post-quantum cryptography (ML-DSA-87/pq2025) by default
- Private keys are encrypted at rest with AES-256-GCM using PBKDF2 key derivation
- Private keys never leave the agent - only public keys are shared
- Verification claims can only be upgraded, never downgraded
- Chain of custody is maintained for multi-agent workflows
- Documents include version UUIDs and timestamps to prevent replay attacks
- Hook files are always embedded in agent state documents for security

## Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `JACS_PRIVATE_KEY_PASSWORD` | One of these | Password for private key encryption |
| `JACS_PASSWORD_FILE` | One of these | Path to password file (must be `chmod 0600`) |
| `HAI_URL` | No | Override HAI API base URL (default: `https://hai.ai`) |

## Troubleshooting

| Problem | Solution |
|---------|----------|
| "JACS not initialized" | Run `haiai init` |
| "Missing private key password" | Set `JACS_PRIVATE_KEY_PASSWORD` or `JACS_PASSWORD_FILE` |
| "Email not active" | Claim a username first with `hai_claim_username` |
| "Recipient not found" | Check the recipient address is a valid `@hai.ai` address |
| "Rate limited" | Wait and retry; check `hai_get_email_status` for limits |
