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
// Deprecated FromURL functions now return errors (no native HTTP fallback)
// -----------------------------------------------------------------------

func TestFetchKeyByEmailFromURL_Deprecated(t *testing.T) {
	_, err := FetchKeyByEmailFromURL(context.Background(), nil, "http://unused", "alice@hai.ai")
	if err == nil {
		t.Fatal("expected error from deprecated FetchKeyByEmailFromURL")
	}
	if !strings.Contains(err.Error(), "deprecated") {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestFetchKeyByDomainFromURL_Deprecated(t *testing.T) {
	_, err := FetchKeyByDomainFromURL(context.Background(), nil, "http://unused", "example.com")
	if err == nil {
		t.Fatal("expected error from deprecated FetchKeyByDomainFromURL")
	}
	if !strings.Contains(err.Error(), "deprecated") {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestFetchAllKeysFromURL_Deprecated(t *testing.T) {
	_, err := FetchAllKeysFromURL(context.Background(), nil, "http://unused", "agent-abc")
	if err == nil {
		t.Fatal("expected error from deprecated FetchAllKeysFromURL")
	}
	if !strings.Contains(err.Error(), "deprecated") {
		t.Fatalf("unexpected error: %v", err)
	}
}

// -----------------------------------------------------------------------
// Standalone functions without client return error
// -----------------------------------------------------------------------

func TestFetchKeyByEmail_NilClient_ReturnsError(t *testing.T) {
	_, err := FetchKeyByEmail(context.Background(), nil, "alice@hai.ai")
	if err == nil {
		t.Fatal("expected error when client is nil")
	}
	if !strings.Contains(err.Error(), "Client required") {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestFetchKeyByDomain_NilClient_ReturnsError(t *testing.T) {
	_, err := FetchKeyByDomain(context.Background(), nil, "example.com")
	if err == nil {
		t.Fatal("expected error when client is nil")
	}
	if !strings.Contains(err.Error(), "Client required") {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestFetchKeyByHash_NilClient_ReturnsError(t *testing.T) {
	_, err := FetchKeyByHash(context.Background(), nil, "sha256:abc")
	if err == nil {
		t.Fatal("expected error when client is nil")
	}
	if !strings.Contains(err.Error(), "Client required") {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestFetchAllKeys_NilClient_ReturnsError(t *testing.T) {
	_, err := FetchAllKeys(context.Background(), nil, "agent-abc")
	if err == nil {
		t.Fatal("expected error when client is nil")
	}
	if !strings.Contains(err.Error(), "Client required") {
		t.Fatalf("unexpected error: %v", err)
	}
}

// -----------------------------------------------------------------------
// Client methods (delegate to FFI via client)
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
