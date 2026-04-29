package haiai

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"strconv"
	"testing"
	"time"
)

// testAgentEmail is the default agent email used by test clients.
const testAgentEmail = "testagent@hai.ai"

// newTestClient creates a Client pointing at a test server with a generated key pair.
// The client uses the mockFFIClient to bridge HTTP test servers to the FFI interface.
// All signing is delegated to the mock FFI client.
func newTestClient(t *testing.T, serverURL string) (*Client, ed25519.PublicKey) {
	t.Helper()
	pub, priv, err := GenerateKeyPair()
	if err != nil {
		t.Fatalf("GenerateKeyPair: %v", err)
	}

	// Build an auth header using the test key for the mock FFI client
	authHeader := buildTestAuthHeader(priv, "test-agent-id")

	mockFFI := newMockFFIClient(serverURL, "test-agent-id", authHeader)
	mockFFI.buildAuthHeaderFn = func() (string, error) {
		return buildTestAuthHeader(priv, "test-agent-id"), nil
	}
	// Wire up SignMessage to sign with the test key
	mockFFI.signMessageFn = func(message string) (string, error) {
		sig := ed25519.Sign(priv, []byte(message))
		return base64.StdEncoding.EncodeToString(sig), nil
	}
	mockFFI.signResponseFn = func(payloadJSON string) (json.RawMessage, error) {
		var payload map[string]interface{}
		if err := json.Unmarshal([]byte(payloadJSON), &payload); err != nil {
			return nil, err
		}
		doc := map[string]interface{}{
			"version":       "1.0.0",
			"document_type": "job_response",
			"data":          payload,
			"metadata": map[string]interface{}{
				"issuer":      "test-agent-id",
				"document_id": "doc-1",
				"created_at":  "2026-01-01T00:00:00Z",
				"hash":        "hash-from-ffi",
			},
			"jacsSignature": map[string]interface{}{
				"agentID":   "test-agent-id",
				"date":      "2026-01-01T00:00:00Z",
				"signature": "sig-from-ffi",
			},
		}
		docJSON, err := json.Marshal(doc)
		if err != nil {
			return nil, err
		}
		return json.Marshal(map[string]interface{}{
			"signed_document": string(docJSON),
			"agent_jacs_id":   "test-agent-id",
		})
	}

	cl, err := NewClient(
		WithEndpoint(serverURL),
		WithJACSID("test-agent-id"),
		WithFFIClient(mockFFI),
	)
	if err != nil {
		t.Fatalf("NewClient: %v", err)
	}
	cl.SetAgentEmail(testAgentEmail)

	return cl, pub
}

// buildTestAuthHeader constructs a JACS auth header using a raw ed25519 key.
// This is test-only code for creating auth headers in unit tests.
func buildTestAuthHeader(privateKey ed25519.PrivateKey, jacsID string) string {
	timestamp := strconv.FormatInt(time.Now().Unix(), 10)
	message := fmt.Sprintf("%s:%s", jacsID, timestamp)
	sig := ed25519.Sign(privateKey, []byte(message))
	sigB64 := base64.StdEncoding.EncodeToString(sig)
	return fmt.Sprintf("JACS %s:%s:%s", jacsID, timestamp, sigB64)
}
