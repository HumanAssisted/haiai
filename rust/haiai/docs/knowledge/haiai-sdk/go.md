# haiai-go -- Go SDK

Go SDK for local JACS identity, signing and verification, plus admitted [HAI.AI](https://hai.ai) platform integrations. See the shared [capability boundaries](../README.md#capability-boundaries) for registration, active email and current Agreement/advocate/mediator limits, and [platform compatibility](../README.md#platform-compatibility) before making API calls.

## Install

```bash
go get github.com/HumanAssisted/haiai-go
```

The SDK requires CGo and the Rust `libhaiigo` shared library; see [Crypto Backend](#crypto-backend).

## Local quickstart

Follow the shared [local identity setup](../README.md#local-quickstart), then run this from the directory containing `jacs.config.json` with `JACS_PRIVATE_KEY_PASSWORD` set to its key password. It writes a disposable note, signs it in place (keeping a `.bak` copy), and checks the signature locally. No platform registration is needed.

```go
package main

import (
	"context"
	"fmt"
	"log"
	"os"

	hai "github.com/HumanAssisted/haiai-go"
)

func main() {
	agent, err := hai.AgentFromConfig("")
	if err != nil {
		log.Fatal(err)
	}

	client := agent.Client()
	ctx := context.Background()
	if err := os.WriteFile("sdk-note.md", []byte("Hello from my agent.\n"), 0600); err != nil {
		log.Fatal(err)
	}
	if _, err := client.SignText(ctx, "sdk-note.md", hai.SignTextOptions{}); err != nil {
		log.Fatal(err)
	}
	result, err := client.VerifyText(ctx, "sdk-note.md", hai.VerifyTextOptions{Strict: true})
	if err != nil {
		log.Fatal(err)
	}
	if len(result.Signatures) == 0 {
		log.Fatal("No signature found")
	}
	for _, signature := range result.Signatures {
		if signature.Status != "valid" {
			log.Fatalf("Signature verification failed: %s", signature.Status)
		}
	}
	fmt.Println("valid")
}
```

Expected output: `valid`. The file-level `signed` status only means a signature was found; each signature must be `valid`. This proves agent provenance, not a person's approval of an Agreement.

## Caller-built request authentication

SDK API methods authenticate requests automatically. For your own HTTP call,
use `client.BuildRequestAuthHeader("POST", finalURL, bodyBytes)`. Send those exact
bytes to that URL without redirects, and build a fresh header for each retry.
The URL must match the configured HAI origin. The old no-context FFI helper now
returns an actionable error.

The service audience defaults to `hai.ai`; use `WithRequestAuthAudience` only
when your API deployment uses another pinned audience. It cannot be changed
per request. Go only encodes bytes for FFI; Rust/JACS owns the authentication
policy and cryptography.

## Email

For admitted existing-identity registration, `Client.Register` accepts optional
`RegisterOptions.RegistrationKey`; an empty value omits it. See the shared
[registration guidance](../README.md#admitted-registration-and-email).
`RegisterOptions.PublicKey` accepts raw PEM; Rust performs the HTTP base64 encoding.

Ordinary and bootstrap registration results preserve `RegistrationStatus` and `Email` (`*string`, `nil` when absent).
Status strings are forwarded without restricting future values. Missing or
unknown status is not confirmation of admission, and an assigned address does
not establish mailbox readiness or email delivery. For manual enrollment of
an existing local identity, the CLI also provides
`haiai register --key KEY --config-path ./jacs.config.json`; follow the shared guidance above to distinguish
a confirmed rejection from a transport failure that may have committed.

Platform email requires admitted registration and server-returned email status `active`; an allocated or pending address cannot send. Inspect `agent.Email.Status(ctx)` for the actual address, status and limits. Quota, external-recipient and content gates still apply; see [capability boundaries](../README.md#capability-boundaries).

Signed email defaults to `html_inline_jacs`: the SDK renders safe HTML, embeds the signed inline logo and hidden JACS envelope, and adds the verify footer. Set `SendEmailOptions.GenerationType` to `EmailGenerationTypeAttachmentJacs` only for compatibility with the older attachment transport. For now, signed email body input must be plain text; caller-supplied HTML and reserved HAI/JACS inline markers are rejected before signing.

| Method | Description |
|--------|-------------|
| `agent.Email.Send()` | Send a signed email |
| `agent.Email.Inbox()` | List inbox messages |
| `agent.Email.Search()` | Search by query, sender, date |
| `agent.Email.Reply()` | Reply with threading |
| `agent.Email.Forward()` | Forward a message |
| `agent.Email.Status()` | Account limits and capacity |
| `agent.Email.Contacts()` | List contacts from email history |

### Raw MIME retrieval and verification

These helpers require platform access. For an entirely local check, use the quickstart above.

```go
raw, err := client.GetRawEmail(ctx, "m.uuid")
if err != nil { return err }
if !raw.Available { return fmt.Errorf("unavailable: %s", raw.OmittedReason) }
result, err := client.VerifyEmail(ctx, raw.RawEmail)
if err != nil || !result.Valid { return errors.New("tampered or revoked") }
```

Bytes are byte-identical to what JACS signed (25 MB cap). See
[How verified email works](https://hai.ai/about/email).

## A2A Integration

```go
ctx := context.Background()
a2a := client.GetA2A(hai.A2ATrustPolicyVerified)

wrapped, _ := a2a.SignArtifact(map[string]interface{}{
	"taskId": "t-1",
	"input":  "hello",
}, "task", nil)
verified, _ := a2a.VerifyArtifact(wrapped)
fmt.Println(verified.Valid)
```

Working example: `examples/a2a/main.go`.

## Crypto Backend

Cryptographic operations delegate to JACS through the Rust FFI layer. Build with `CGO_ENABLED=1` and make `libhaiigo` available to the linker and runtime. The repository's [Makefile](../Makefile) `build-haiigo` and `test-go` targets show the library setup. A Rust toolchain is required to build it from source; builds without CGo cannot initialize a production client.

## Requirements

- Go 1.23+
- CGo and `libhaiigo`
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
