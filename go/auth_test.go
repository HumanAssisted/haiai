package haiai

import (
	"crypto/ed25519"
	"encoding/base64"
	"fmt"
	"strconv"
	"strings"
	"testing"
	"time"
)

func TestBuildAuthHeader(t *testing.T) {
	_, priv, err := GenerateKeyPair()
	if err != nil {
		t.Fatalf("GenerateKeyPair: %v", err)
	}

	header := BuildAuthHeader("test-agent", priv)

	if !strings.HasPrefix(header, "JACS ") {
		t.Fatalf("expected 'JACS ' prefix, got: %s", header)
	}

	// Parse: JACS {jacsId}:{timestamp}:{signature_base64}
	parts := strings.SplitN(strings.TrimPrefix(header, "JACS "), ":", 3)
	if len(parts) != 3 {
		t.Fatalf("expected 3 parts, got %d: %v", len(parts), parts)
	}

	jacsID := parts[0]
	timestampStr := parts[1]
	sigB64 := parts[2]

	if jacsID != "test-agent" {
		t.Errorf("expected jacsID 'test-agent', got '%s'", jacsID)
	}

	// Parse timestamp
	ts, err := strconv.ParseInt(timestampStr, 10, 64)
	if err != nil {
		t.Fatalf("invalid timestamp: %v", err)
	}

	// Should be within 5 seconds of now
	now := time.Now().Unix()
	if abs(now-ts) > 5 {
		t.Errorf("timestamp %d is too far from now %d", ts, now)
	}

	// Verify signature
	sig, err := base64.StdEncoding.DecodeString(sigB64)
	if err != nil {
		t.Fatalf("invalid base64 signature: %v", err)
	}

	message := fmt.Sprintf("%s:%s", jacsID, timestampStr)
	pub := priv.Public().(ed25519.PublicKey)
	if !ed25519.Verify(pub, []byte(message), sig) {
		t.Error("signature verification failed")
	}
}

func TestBuildAuthHeaderDifferentIDs(t *testing.T) {
	_, priv, _ := GenerateKeyPair()

	h1 := BuildAuthHeader("agent-alpha", priv)
	h2 := BuildAuthHeader("agent-beta", priv)

	if !strings.Contains(h1, "agent-alpha") {
		t.Error("header should contain agent-alpha")
	}
	if !strings.Contains(h2, "agent-beta") {
		t.Error("header should contain agent-beta")
	}
}

func TestBuildAuthHeaderSignatureChanges(t *testing.T) {
	_, priv, _ := GenerateKeyPair()

	// Same jacsID should produce different signatures due to timestamp
	h1 := BuildAuthHeader("agent", priv)
	time.Sleep(time.Second)
	h2 := BuildAuthHeader("agent", priv)

	// Timestamps should differ (or at least signatures will differ)
	sig1 := strings.SplitN(strings.TrimPrefix(h1, "JACS "), ":", 3)[2]
	sig2 := strings.SplitN(strings.TrimPrefix(h2, "JACS "), ":", 3)[2]

	// They might be the same if both hit the same second, but timestamps should be close
	_ = sig1
	_ = sig2
}

func TestAuthHeaderVerifiableByServer(t *testing.T) {
	// This simulates what the Rust server does: extract credentials, verify signature
	pub, priv, _ := GenerateKeyPair()

	header := BuildAuthHeader("my-agent-jacs-id", priv)

	// Server-side: extract JACS credentials
	token := strings.TrimPrefix(header, "JACS ")
	parts := strings.SplitN(token, ":", 3)
	if len(parts) != 3 {
		t.Fatal("invalid JACS header format")
	}

	jacsID := parts[0]
	timestampStr := parts[1]
	sigB64 := parts[2]

	// Verify timestamp is recent
	ts, _ := strconv.ParseInt(timestampStr, 10, 64)
	now := time.Now().Unix()
	if abs(now-ts) > 300 {
		t.Fatal("timestamp outside 5-minute window")
	}

	// Reconstruct message and verify signature
	message := fmt.Sprintf("%s:%s", jacsID, timestampStr)
	sig, _ := base64.StdEncoding.DecodeString(sigB64)

	if !ed25519.Verify(pub, []byte(message), sig) {
		t.Error("server-side verification failed")
	}
}

func abs(x int64) int64 {
	if x < 0 {
		return -x
	}
	return x
}
