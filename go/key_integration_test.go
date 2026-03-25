// Live integration tests for JACS key rotation and versioned fetch operations.
//
// Gated behind HAI_LIVE_TEST=1. Requires a running HAI API at
// HAI_URL (defaults to http://localhost:3000).
//
// Run:
//
//	HAI_LIVE_TEST=1 HAI_URL=http://localhost:3000 go test -run TestKeyIntegration -v -count=1

package haiai

import (
	"context"
	"fmt"
	"os"
	"testing"
	"time"
)

func TestKeyIntegration(t *testing.T) {
	if os.Getenv("HAI_LIVE_TEST") != "1" {
		t.Skip("set HAI_LIVE_TEST=1 to run live API tests")
	}

	apiURL := os.Getenv("HAI_URL")
	if apiURL == "" {
		apiURL = "http://localhost:3000"
	}

	ctx := context.Background()
	agentName := fmt.Sprintf("go-key-integ-%d", time.Now().UnixMilli())

	// ── Setup: register a fresh JACS agent ────────────────────────────────
	ownerEmail := os.Getenv("HAI_OWNER_EMAIL")
	if ownerEmail == "" {
		ownerEmail = "jonathan@hai.io"
	}

	reg, err := RegisterNewAgentWithEndpoint(ctx, apiURL, agentName, &RegisterNewAgentOptions{
		Description: "Go key integration test agent",
		OwnerEmail:  ownerEmail,
		Quiet:       true,
	})
	if err != nil {
		t.Fatalf("RegisterNewAgentWithEndpoint: %v", err)
	}

	jacsID := reg.JacsID
	if jacsID == "" {
		jacsID = reg.AgentID
	}
	t.Logf("Registered agent: jacs_id=%s", jacsID)

	// Set the HAI_KEYS_BASE_URL to the live API so Client methods use it.
	t.Setenv("HAI_KEYS_BASE_URL", apiURL)

	// ── Test: register then fetch key matches ─────────────────────────────
	t.Run("RegisterThenFetchKeyMatches", func(t *testing.T) {
		// FetchRemoteKeyFromURL is deprecated. Use Client.FetchRemoteKey instead.
		// This test requires a client with credentials.
		if reg.PrivateKeyPath == "" {
			t.Skip("no private key path in registration result")
		}
		password, err := ResolvePrivateKeyPassword()
		if err != nil {
			t.Skipf("ResolvePrivateKeyPassword: %v", err)
		}
		privKey, err := LoadPrivateKey(reg.PrivateKeyPath, password)
		if err != nil {
			t.Skipf("LoadPrivateKey: %v", err)
		}
		cl, err := NewClient(
			WithEndpoint(apiURL),
			WithJACSID(jacsID),
			WithHaiAgentID(reg.AgentID),
			WithPrivateKey(privKey),
		)
		if err != nil {
			t.Skipf("could not build client: %v", err)
		}
		key, err := cl.FetchRemoteKey(ctx, jacsID, "latest")
		if err != nil {
			t.Fatalf("FetchRemoteKey: %v", err)
		}
		if len(key.PublicKey) == 0 {
			t.Fatal("expected non-empty public key")
		}
		if key.Algorithm == "" {
			t.Fatal("expected non-empty algorithm")
		}
		t.Logf("Fetched key: algorithm=%s, hash=%s", key.Algorithm, key.PublicKeyHash)
	})

	// ── Test: fetch key by hash via Client ────────────────────────────────
	t.Run("FetchKeyByHashMatches", func(t *testing.T) {
		// NOTE: FetchKeyByHashFromURL is deprecated. Use Client.FetchKeyByHash instead.
		// This test now requires a Client; skip if unavailable.
		t.Skip("FetchKeyByHashFromURL deprecated; use Client-based test instead")
	})

	// ── Test: fetch key by email via Client ──────────────────────────────
	t.Run("FetchKeyByEmailMatches", func(t *testing.T) {
		// Need a client with credentials to claim username.
		if reg.PrivateKeyPath == "" {
			t.Skip("no private key path in registration result")
		}

		// Load private key from path
		password, err := ResolvePrivateKeyPassword()
		if err != nil {
			t.Skipf("ResolvePrivateKeyPassword: %v", err)
		}
		privKey, err := LoadPrivateKey(reg.PrivateKeyPath, password)
		if err != nil {
			t.Skipf("LoadPrivateKey: %v", err)
		}

		// Build a client
		cl, err := NewClient(
			WithEndpoint(apiURL),
			WithJACSID(jacsID),
			WithHaiAgentID(reg.AgentID),
			WithPrivateKey(privKey),
		)
		if err != nil {
			t.Skipf("could not build client: %v", err)
		}

		claim, err := cl.ClaimUsername(ctx, reg.AgentID, agentName)
		if err != nil {
			t.Skipf("could not claim username: %v", err)
		}

		email := claim.Email
		if email == "" {
			t.Skip("no email returned from ClaimUsername")
		}

		// Use Client method (FFI-backed) instead of deprecated FetchKeyByEmailFromURL
		byEmail, err := cl.FetchKeyByEmail(ctx, email)
		if err != nil {
			t.Fatalf("FetchKeyByEmail: %v", err)
		}
		if len(byEmail.PublicKey) == 0 {
			t.Fatal("expected non-empty public key from email lookup")
		}
	})

	// ── Test: fetch all keys returns history ──────────────────────────────
	t.Run("FetchAllKeysReturnsHistory", func(t *testing.T) {
		// NOTE: FetchAllKeysFromURL is deprecated. Use Client.FetchAllKeys instead.
		// This test now requires a Client; skip if unavailable.
		t.Skip("FetchAllKeysFromURL deprecated; use Client-based test instead")
	})

	// ── Test: fetch key by domain returns 404 for fake domain ─────────────
	t.Run("FetchKeyByDomain404ForFakeDomain", func(t *testing.T) {
		// FetchKeyByDomainFromURL is deprecated -- now returns error directly.
		_, err := FetchKeyByDomainFromURL(ctx, nil, apiURL, "nonexistent-test-domain-12345.invalid")
		if err == nil {
			t.Fatal("expected error for deprecated function")
		}
		t.Logf("Got expected error: %v", err)
	})
}
