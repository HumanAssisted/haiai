package haiai

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

// keyResponseJSON returns a synthetic key-lookup JSON response.
func keyResponseJSON() string {
	return `{
		"jacs_id":"agent-abc",
		"version":"v1",
		"public_key":"-----BEGIN PUBLIC KEY-----\nZm9v\n-----END PUBLIC KEY-----\n",
		"public_key_raw_b64":"Zm9v",
		"algorithm":"Ed25519",
		"public_key_hash":"sha256:abcdef1234567890",
		"status":"active",
		"dns_verified":true,
		"created_at":"2026-01-15T10:30:00Z"
	}`
}

// keyHistoryResponseJSON returns a synthetic key-history JSON response.
func keyHistoryResponseJSON() string {
	return `{
		"jacs_id":"agent-abc",
		"keys":[` + keyResponseJSON() + `,` + `{
			"jacs_id":"agent-abc",
			"version":"v0",
			"public_key":"-----BEGIN PUBLIC KEY-----\nZm9v\n-----END PUBLIC KEY-----\n",
			"public_key_raw_b64":"Zm9v",
			"algorithm":"Ed25519",
			"public_key_hash":"sha256:abcdef1234567890",
			"status":"rotated",
			"dns_verified":true,
			"created_at":"2025-12-01T10:00:00Z"
		}],
		"total":2
	}`
}

// -----------------------------------------------------------------------
// FetchKeyByEmail (standalone function)
// -----------------------------------------------------------------------

func TestFetchKeyByEmailFromURL_CallsCorrectEndpoint(t *testing.T) {
	var gotPath string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.EscapedPath()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(keyResponseJSON()))
	}))
	defer srv.Close()

	key, err := FetchKeyByEmailFromURL(context.Background(), nil, srv.URL, "alice@hai.ai")
	if err != nil {
		t.Fatalf("FetchKeyByEmailFromURL: %v", err)
	}
	// NOTE: Go's url.PathEscape does not encode '@' because it is allowed in
	// path segments per RFC 3986. The server-side path is decoded, so both
	// alice@hai.ai and alice%40hai.ai resolve to the same handler.
	if gotPath != "/api/agents/keys/alice@hai.ai" {
		t.Fatalf("unexpected path: %s", gotPath)
	}
	if key.AgentID != "agent-abc" {
		t.Fatalf("unexpected agent id: %s", key.AgentID)
	}
	if key.Algorithm != "Ed25519" {
		t.Fatalf("unexpected algorithm: %s", key.Algorithm)
	}
}

func TestFetchKeyByEmailFromURL_Returns404Error(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusNotFound)
		_, _ = w.Write([]byte(`{"error":"not found"}`))
	}))
	defer srv.Close()

	_, err := FetchKeyByEmailFromURL(context.Background(), nil, srv.URL, "nobody@hai.ai")
	if err == nil {
		t.Fatal("expected error on 404")
	}
	if !strings.Contains(err.Error(), "no key found for email") {
		t.Fatalf("unexpected error message: %v", err)
	}
}

// -----------------------------------------------------------------------
// FetchKeyByDomain (standalone function)
// -----------------------------------------------------------------------

func TestFetchKeyByDomainFromURL_CallsCorrectEndpoint(t *testing.T) {
	var gotPath string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.EscapedPath()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(keyResponseJSON()))
	}))
	defer srv.Close()

	key, err := FetchKeyByDomainFromURL(context.Background(), nil, srv.URL, "example.com")
	if err != nil {
		t.Fatalf("FetchKeyByDomainFromURL: %v", err)
	}
	if gotPath != "/api/agents/keys/domain/example.com" {
		t.Fatalf("unexpected path: %s", gotPath)
	}
	if key.AgentID != "agent-abc" {
		t.Fatalf("unexpected agent id: %s", key.AgentID)
	}
}

func TestFetchKeyByDomainFromURL_Returns404Error(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusNotFound)
		_, _ = w.Write([]byte(`{"error":"not found"}`))
	}))
	defer srv.Close()

	_, err := FetchKeyByDomainFromURL(context.Background(), nil, srv.URL, "nonexistent.test")
	if err == nil {
		t.Fatal("expected error on 404")
	}
	if !strings.Contains(err.Error(), "no verified agent for domain") {
		t.Fatalf("unexpected error message: %v", err)
	}
}

// -----------------------------------------------------------------------
// FetchAllKeys (standalone function)
// -----------------------------------------------------------------------

func TestFetchAllKeysFromURL_CallsCorrectEndpoint(t *testing.T) {
	var gotPath string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.EscapedPath()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(keyHistoryResponseJSON()))
	}))
	defer srv.Close()

	history, err := FetchAllKeysFromURL(context.Background(), nil, srv.URL, "agent-abc")
	if err != nil {
		t.Fatalf("FetchAllKeysFromURL: %v", err)
	}
	if gotPath != "/api/agents/keys/agent-abc/all" {
		t.Fatalf("unexpected path: %s", gotPath)
	}
	if history.JacsID != "agent-abc" {
		t.Fatalf("unexpected jacs_id: %s", history.JacsID)
	}
	if history.Total != 2 {
		t.Fatalf("unexpected total: %d", history.Total)
	}
	if len(history.Keys) != 2 {
		t.Fatalf("expected 2 keys, got %d", len(history.Keys))
	}
}

func TestFetchAllKeysFromURL_EscapesJacsIDWithSlashes(t *testing.T) {
	var gotPath string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.EscapedPath()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(keyHistoryResponseJSON()))
	}))
	defer srv.Close()

	_, err := FetchAllKeysFromURL(context.Background(), nil, srv.URL, "agent/with/slashes")
	if err != nil {
		t.Fatalf("FetchAllKeysFromURL: %v", err)
	}
	if !strings.Contains(gotPath, "agent%2Fwith%2Fslashes") {
		t.Fatalf("agent id should be escaped in path, got %q", gotPath)
	}
}

func TestFetchAllKeysFromURL_Returns404Error(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusNotFound)
		_, _ = w.Write([]byte(`{"error":"not found"}`))
	}))
	defer srv.Close()

	_, err := FetchAllKeysFromURL(context.Background(), nil, srv.URL, "missing-agent")
	if err == nil {
		t.Fatal("expected error on 404")
	}
	if !strings.Contains(err.Error(), "agent not found") {
		t.Fatalf("unexpected error message: %v", err)
	}
}

// -----------------------------------------------------------------------
// Client methods (delegate to standalone via env)
// -----------------------------------------------------------------------

func TestClientFetchKeyByEmail(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// Go's url.PathEscape does not encode '@' per RFC 3986.
		if r.URL.EscapedPath() != "/api/agents/keys/bob@hai.ai" {
			t.Fatalf("unexpected path: %s", r.URL.EscapedPath())
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(keyResponseJSON()))
	}))
	defer srv.Close()

	t.Setenv("HAI_KEYS_BASE_URL", srv.URL)

	cl, _ := newTestClient(t, srv.URL)
	key, err := cl.FetchKeyByEmail(context.Background(), "bob@hai.ai")
	if err != nil {
		t.Fatalf("FetchKeyByEmail: %v", err)
	}
	if key.AgentID != "agent-abc" {
		t.Fatalf("unexpected agent id: %s", key.AgentID)
	}
}

func TestClientFetchKeyByDomain(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.EscapedPath() != "/api/agents/keys/domain/example.com" {
			t.Fatalf("unexpected path: %s", r.URL.EscapedPath())
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(keyResponseJSON()))
	}))
	defer srv.Close()

	t.Setenv("HAI_KEYS_BASE_URL", srv.URL)

	cl, _ := newTestClient(t, srv.URL)
	key, err := cl.FetchKeyByDomain(context.Background(), "example.com")
	if err != nil {
		t.Fatalf("FetchKeyByDomain: %v", err)
	}
	if key.AgentID != "agent-abc" {
		t.Fatalf("unexpected agent id: %s", key.AgentID)
	}
}

func TestClientFetchAllKeys(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.EscapedPath() != "/api/agents/keys/agent-abc/all" {
			t.Fatalf("unexpected path: %s", r.URL.EscapedPath())
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(keyHistoryResponseJSON()))
	}))
	defer srv.Close()

	t.Setenv("HAI_KEYS_BASE_URL", srv.URL)

	cl, _ := newTestClient(t, srv.URL)
	history, err := cl.FetchAllKeys(context.Background(), "agent-abc")
	if err != nil {
		t.Fatalf("FetchAllKeys: %v", err)
	}
	if history.Total != 2 {
		t.Fatalf("unexpected total: %d", history.Total)
	}
}

// -----------------------------------------------------------------------
// AgentKeyHistory via FetchAllKeysFromURL (integration)
// -----------------------------------------------------------------------

func TestAgentKeyHistoryViaFetchAllKeys(t *testing.T) {
	// AgentKeyHistory contains []PublicKeyInfo where PublicKey is []byte.
	// Direct json.Unmarshal would try base64-decode on PEM strings and fail.
	// FetchAllKeysFromURL uses an intermediate struct to handle this properly.
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(keyHistoryResponseJSON()))
	}))
	defer srv.Close()

	history, err := FetchAllKeysFromURL(context.Background(), nil, srv.URL, "agent-abc")
	if err != nil {
		t.Fatalf("FetchAllKeysFromURL: %v", err)
	}
	if history.JacsID != "agent-abc" {
		t.Fatalf("unexpected jacs_id: %s", history.JacsID)
	}
	if history.Total != 2 {
		t.Fatalf("unexpected total: %d", history.Total)
	}
	if len(history.Keys) != 2 {
		t.Fatalf("expected 2 keys, got %d", len(history.Keys))
	}
	// The fixture provides both public_key (PEM) and public_key_raw_b64 ("Zm9v" = "foo").
	// decodePublicKey prefers public_key_raw_b64, so we get the decoded raw bytes.
	if string(history.Keys[0].PublicKey) != "foo" {
		t.Fatalf("expected raw public key bytes 'foo' (from base64 Zm9v), got %q", string(history.Keys[0].PublicKey))
	}
}
