package haisdk

import (
	"crypto/ed25519"
	"testing"
)

// testAgentEmail is the default agent email used by test clients.
const testAgentEmail = "testagent@hai.ai"

// newTestClient creates a Client pointing at a test server with a generated key pair.
// The client's agentEmail is set to testAgentEmail so that email-sending methods work.
func newTestClient(t *testing.T, serverURL string) (*Client, ed25519.PublicKey) {
	t.Helper()
	pub, priv, err := GenerateKeyPair()
	if err != nil {
		t.Fatalf("GenerateKeyPair: %v", err)
	}

	cl, err := NewClient(
		WithEndpoint(serverURL),
		WithJACSID("test-agent-id"),
		WithPrivateKey(priv),
	)
	if err != nil {
		t.Fatalf("NewClient: %v", err)
	}
	cl.SetAgentEmail(testAgentEmail)
	return cl, pub
}
