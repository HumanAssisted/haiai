package haiai

// CryptoBackend abstracts all cryptographic operations. The SDK delegates
// to the JACS Rust core via CGo. CGo must be enabled (default on macOS/Linux)
// and the jacsgo shared library must be available at link time.
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

	// CanonicalizeJSON produces canonical JSON per RFC 8785 (JCS).
	// jsonStr is any valid JSON string.
	// Returns the canonicalized JSON string.
	CanonicalizeJSON(jsonStr string) (string, error)

	// SignResponse wraps a response payload in a signed JACS document envelope.
	// payloadJSON is the JSON string of the payload to sign.
	// Returns the signed document JSON string.
	SignResponse(payloadJSON string) (string, error)

	// EncodeVerifyPayload encodes a document as URL-safe base64 (no padding)
	// for verification links.
	EncodeVerifyPayload(document string) (string, error)

	// UnwrapSignedEvent unwraps and verifies a signed event using server keys.
	// eventJSON is the signed event JSON string.
	// serverKeysJSON is the server public keys JSON string.
	// Returns the unwrapped event payload as a JSON string.
	UnwrapSignedEvent(eventJSON, serverKeysJSON string) (string, error)

	// BuildAuthHeader constructs the full JACS Authorization header value.
	// Format: "JACS {jacsId}:{timestamp}:{signature_base64}"
	BuildAuthHeader() (string, error)

	// --- A2A Protocol Methods ---

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

// cryptoBackend is the package-level crypto backend, set at init time.
// See crypto_jacs.go.
var cryptoBackend CryptoBackend
