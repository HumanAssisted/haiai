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
}

// cryptoBackend is the package-level crypto backend, set at init time based on
// build tags. See crypto_jacs.go and crypto_fallback.go.
var cryptoBackend CryptoBackend
