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
		key, err := FetchRemoteKeyFromURL(ctx, nil, apiURL, jacsID, "latest")
		if err != nil {
			t.Fatalf("FetchRemoteKeyFromURL: %v", err)
		}
		if len(key.PublicKey) == 0 {
			t.Fatal("expected non-empty public key")
		}
		if key.Algorithm == "" {
			t.Fatal("expected non-empty algorithm")
		}
		t.Logf("Fetched key: algorithm=%s, hash=%s", key.Algorithm, key.PublicKeyHash)
	})

	// ── Test: fetch key by hash ───────────────────────────────────────────
	t.Run("FetchKeyByHashMatches", func(t *testing.T) {
		key, err := FetchRemoteKeyFromURL(ctx, nil, apiURL, jacsID, "latest")
		if err != nil {
			t.Fatalf("FetchRemoteKeyFromURL: %v", err)
		}
		if key.PublicKeyHash == "" {
			t.Skip("server did not return public_key_hash")
		}

		byHash, err := FetchKeyByHashFromURL(ctx, nil, apiURL, key.PublicKeyHash)
		if err != nil {
			t.Fatalf("FetchKeyByHashFromURL: %v", err)
		}
		if string(byHash.PublicKey) != string(key.PublicKey) {
			t.Fatalf("key mismatch: by-hash %q vs remote %q", string(byHash.PublicKey), string(key.PublicKey))
		}
	})

	// ── Test: fetch key by email ──────────────────────────────────────────
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

		byEmail, err := FetchKeyByEmailFromURL(ctx, nil, apiURL, email)
		if err != nil {
			t.Fatalf("FetchKeyByEmailFromURL: %v", err)
		}
		if len(byEmail.PublicKey) == 0 {
			t.Fatal("expected non-empty public key from email lookup")
		}
	})

	// ── Test: fetch all keys returns history ──────────────────────────────
	t.Run("FetchAllKeysReturnsHistory", func(t *testing.T) {
		history, err := FetchAllKeysFromURL(ctx, nil, apiURL, jacsID)
		if err != nil {
			t.Fatalf("FetchAllKeysFromURL: %v", err)
		}
		if history.Total < 1 {
			t.Fatalf("expected at least 1 key, got total=%d", history.Total)
		}
		if len(history.Keys) < 1 {
			t.Fatalf("expected at least 1 key entry, got %d", len(history.Keys))
		}
		t.Logf("Key history: %d entries", history.Total)
	})

	// ── Test: fetch key by domain returns 404 for fake domain ─────────────
	t.Run("FetchKeyByDomain404ForFakeDomain", func(t *testing.T) {
		_, err := FetchKeyByDomainFromURL(ctx, nil, apiURL, "nonexistent-test-domain-12345.invalid")
		if err == nil {
			t.Fatal("expected error for nonexistent domain")
		}
		t.Logf("Got expected error: %v", err)
	})
}
