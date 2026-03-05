package haisdk

// CryptoBackend abstracts all cryptographic operations so that the SDK can
// delegate to the JACS Rust core via CGo when available, or fall back to a
// pure-Go Ed25519 implementation.
//
// Build with tags "cgo" and "jacs" to use the JACS backend:
//
//	go build -tags jacs ./...
//
// Without those tags the SDK compiles as pure Go using crypto/ed25519.
type CryptoBackend interface {
	// SignString signs an arbitrary message string and returns the base64-encoded signature.
	SignString(message string) (string, error)

	// SignBytes signs raw bytes and returns the raw signature bytes.
	SignBytes(data []byte) ([]byte, error)

	// VerifyBytes verifies a raw signature over data using the given public key PEM.
	// Returns nil on success, non-nil on failure.
	VerifyBytes(data, signature []byte, publicKeyPEM string) error

	// SignRequest wraps a JSON payload in a signed JACS document envelope.
	// Returns the full signed document JSON string.
	SignRequest(payloadJSON string) (string, error)

	// VerifyResponse verifies a signed JACS response document.
	// Returns the verified payload JSON string.
	VerifyResponse(documentJSON string) (string, error)

	// GenerateKeyPair generates a new keypair.
	// Returns (publicKeyPEM, privateKeySeed, error).
	// The private key seed can be used to reconstruct the full key.
	GenerateKeyPair() (publicKeyPEM []byte, privateKeyPEM []byte, err error)

	// Algorithm returns the signing algorithm name (e.g., "Ed25519", "pq2025").
	Algorithm() string

	// --- A2A Protocol Methods ---
	// These methods delegate to the JACS Rust core for A2A operations.
	// Fallback backends return descriptive errors since A2A requires JACS core.

	// SignA2AArtifact wraps an artifact with a JACS signature for A2A exchange.
	// artifactJSON is the JSON payload to sign, artifactType identifies the artifact kind
	// (e.g., "task", "task-result"). Returns the signed wrapped artifact JSON.
	SignA2AArtifact(artifactJSON string, artifactType string) (string, error)

	// VerifyA2AArtifact verifies a JACS-wrapped A2A artifact (crypto-only).
	// wrappedJSON is the full signed wrapper. Returns the verification result JSON.
	VerifyA2AArtifact(wrappedJSON string) (string, error)

	// VerifyA2AArtifactWithPolicy verifies a JACS-wrapped artifact with trust policy.
	// agentCardJSON is the remote agent's card, policyJSON is the trust policy to enforce.
	// Returns the verification result JSON.
	VerifyA2AArtifactWithPolicy(wrappedJSON, agentCardJSON, policyJSON string) (string, error)

	// AssessA2AAgent evaluates a remote agent's trustworthiness against a policy.
	// agentCardJSON is the agent card to assess, policyJSON is the trust policy.
	// Returns the assessment result JSON.
	AssessA2AAgent(agentCardJSON, policyJSON string) (string, error)

	// ExportAgentCard exports an A2A Agent Card for the loaded agent.
	// agentDataJSON provides optional agent metadata to include in the card.
	// Returns the agent card JSON.
	ExportAgentCard(agentDataJSON string) (string, error)
}

// cryptoBackend is the package-level crypto backend, set at init time based on
// build tags. See crypto_jacs.go and crypto_fallback.go.
var cryptoBackend CryptoBackend
