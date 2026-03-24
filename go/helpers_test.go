package haiai

import (
	"crypto/ed25519"
	"encoding/base64"
	"fmt"
	"strconv"
	"testing"
	"time"
)

// testAgentEmail is the default agent email used by test clients.
const testAgentEmail = "testagent@hai.ai"

func init() {
	// Override newClientCryptoBackend for all tests so that code paths
	// which create clients internally (RegisterNewAgentWithEndpoint,
	// RotateKeys, etc.) get a working Ed25519 backend without needing
	// a real JACS agent loaded.
	newClientCryptoBackendOverride = func(privateKey ed25519.PrivateKey, jacsID string) CryptoBackend {
		return &testCryptoBackend{
			privateKey: privateKey,
			jacsID:     jacsID,
		}
	}
}

// testCryptoBackend implements CryptoBackend using a raw Ed25519 key pair
// for unit tests that don't need a real JACS agent. This is test-only code
// and never ships in production builds.
type testCryptoBackend struct {
	privateKey ed25519.PrivateKey
	jacsID     string
}

func (b *testCryptoBackend) SignString(message string) (string, error) {
	sig := ed25519.Sign(b.privateKey, []byte(message))
	return base64.StdEncoding.EncodeToString(sig), nil
}

func (b *testCryptoBackend) SignBytes(data []byte) ([]byte, error) {
	return ed25519.Sign(b.privateKey, data), nil
}

func (b *testCryptoBackend) VerifyBytes(data, signature []byte, publicKeyPEM string) error {
	pubKey, err := ParsePublicKey([]byte(publicKeyPEM))
	if err != nil {
		return fmt.Errorf("test backend: invalid public key: %w", err)
	}
	if !ed25519.Verify(pubKey, data, signature) {
		return fmt.Errorf("test backend: signature verification failed")
	}
	return nil
}

func (b *testCryptoBackend) SignRequest(payloadJSON string) (string, error) {
	return "", fmt.Errorf("test backend: SignRequest not implemented")
}

func (b *testCryptoBackend) VerifyResponse(documentJSON string) (string, error) {
	return "", fmt.Errorf("test backend: VerifyResponse not implemented")
}

func (b *testCryptoBackend) GenerateKeyPair() ([]byte, []byte, error) {
	return cryptoBackend.GenerateKeyPair()
}

func (b *testCryptoBackend) Algorithm() string {
	return "Ed25519-test"
}

func (b *testCryptoBackend) CanonicalizeJSON(jsonStr string) (string, error) {
	return jsonStr, nil
}

func (b *testCryptoBackend) SignResponse(payloadJSON string) (string, error) {
	// Minimal signed response envelope for tests
	sig := ed25519.Sign(b.privateKey, []byte(payloadJSON))
	sigB64 := base64.StdEncoding.EncodeToString(sig)
	return fmt.Sprintf(`{"response":%s,"jacsSignature":{"signature":"%s","agentID":"%s"}}`,
		payloadJSON, sigB64, b.jacsID), nil
}

func (b *testCryptoBackend) EncodeVerifyPayload(document string) (string, error) {
	return base64.RawURLEncoding.EncodeToString([]byte(document)), nil
}

func (b *testCryptoBackend) UnwrapSignedEvent(eventJSON, serverKeysJSON string) (string, error) {
	return "", fmt.Errorf("test backend: UnwrapSignedEvent not implemented")
}

func (b *testCryptoBackend) BuildAuthHeader() (string, error) {
	timestamp := strconv.FormatInt(time.Now().Unix(), 10)
	message := fmt.Sprintf("%s:%s", b.jacsID, timestamp)
	sig := ed25519.Sign(b.privateKey, []byte(message))
	sigB64 := base64.StdEncoding.EncodeToString(sig)
	return fmt.Sprintf("JACS %s:%s:%s", b.jacsID, timestamp, sigB64), nil
}

func (b *testCryptoBackend) SignA2AArtifact(artifactJSON string, artifactType string) (string, error) {
	return "", fmt.Errorf("test backend: SignA2AArtifact not implemented")
}

func (b *testCryptoBackend) VerifyA2AArtifact(wrappedJSON string) (string, error) {
	return "", fmt.Errorf("test backend: VerifyA2AArtifact not implemented")
}

func (b *testCryptoBackend) VerifyA2AArtifactWithPolicy(wrappedJSON, agentCardJSON, policyJSON string) (string, error) {
	return "", fmt.Errorf("test backend: VerifyA2AArtifactWithPolicy not implemented")
}

func (b *testCryptoBackend) AssessA2AAgent(agentCardJSON, policyJSON string) (string, error) {
	return "", fmt.Errorf("test backend: AssessA2AAgent not implemented")
}

func (b *testCryptoBackend) ExportAgentCard(agentDataJSON string) (string, error) {
	return "", fmt.Errorf("test backend: ExportAgentCard not implemented")
}

// newTestClient creates a Client pointing at a test server with a generated key pair.
// The client uses the mockFFIClient to bridge HTTP test servers to the FFI interface.
func newTestClient(t *testing.T, serverURL string) (*Client, ed25519.PublicKey) {
	t.Helper()
	pub, priv, err := GenerateKeyPair()
	if err != nil {
		t.Fatalf("GenerateKeyPair: %v", err)
	}

	// Build an auth header for the mock FFI client
	testBackend := &testCryptoBackend{
		privateKey: priv,
		jacsID:     "test-agent-id",
	}
	authHeader, err := testBackend.BuildAuthHeader()
	if err != nil {
		t.Fatalf("BuildAuthHeader: %v", err)
	}

	mockFFI := newMockFFIClient(serverURL, "test-agent-id", authHeader)
	mockFFI.buildAuthHeaderFn = testBackend.BuildAuthHeader

	cl, err := NewClient(
		WithEndpoint(serverURL),
		WithJACSID("test-agent-id"),
		WithPrivateKey(priv),
		WithFFIClient(mockFFI),
	)
	if err != nil {
		t.Fatalf("NewClient: %v", err)
	}
	cl.SetAgentEmail(testAgentEmail)

	// Override crypto backend with test-only Ed25519 backend
	cl.crypto = testBackend

	return cl, pub
}
