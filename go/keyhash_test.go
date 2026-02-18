package haisdk

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"testing"
)

// ===========================================================================
// FetchKeyByHash tests
// ===========================================================================

func TestFetchKeyByHashSuccess(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		expectedPath := "/jacs/v1/keys/by-hash/abc123"
		if r.URL.Path != expectedPath {
			t.Errorf("expected path '%s', got '%s'", expectedPath, r.URL.Path)
		}
		if r.Method != http.MethodGet {
			t.Errorf("expected GET, got %s", r.Method)
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]string{
			"public_key":      "dGVzdC1rZXk=",
			"algorithm":       "ed25519",
			"public_key_hash": "abc123",
			"agent_id":        "agent-1",
			"version":         "1",
		})
	}))
	defer server.Close()

	result, err := FetchKeyByHashFromURL(context.Background(), nil, server.URL, "abc123")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if string(result.PublicKey) != "test-key" {
		t.Errorf("expected PublicKey 'test-key', got '%s'", string(result.PublicKey))
	}
	if result.Algorithm != "ed25519" {
		t.Errorf("expected Algorithm 'ed25519', got '%s'", result.Algorithm)
	}
	if result.PublicKeyHash != "abc123" {
		t.Errorf("expected PublicKeyHash 'abc123', got '%s'", result.PublicKeyHash)
	}
	if result.AgentID != "agent-1" {
		t.Errorf("expected AgentID 'agent-1', got '%s'", result.AgentID)
	}
}

func TestFetchKeyByHashNotFound(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNotFound)
	}))
	defer server.Close()

	_, err := FetchKeyByHashFromURL(context.Background(), nil, server.URL, "unknown-hash")
	if err == nil {
		t.Fatal("expected error for 404")
	}
	sdkErr, ok := err.(*Error)
	if !ok {
		t.Fatalf("expected *Error, got %T", err)
	}
	if sdkErr.Kind != ErrKeyNotFound {
		t.Errorf("expected ErrKeyNotFound, got %v", sdkErr.Kind)
	}
}

func TestFetchKeyByHash500(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
		w.Write([]byte("internal error"))
	}))
	defer server.Close()

	_, err := FetchKeyByHashFromURL(context.Background(), nil, server.URL, "hash")
	if err == nil {
		t.Fatal("expected error for 500")
	}
	sdkErr := err.(*Error)
	if sdkErr.Kind != ErrConnection {
		t.Errorf("expected ErrConnection, got %v", sdkErr.Kind)
	}
}

func TestFetchKeyByHashInvalidBase64(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]string{
			"public_key": "not-valid-base64!!!",
			"algorithm":  "ed25519",
		})
	}))
	defer server.Close()

	_, err := FetchKeyByHashFromURL(context.Background(), nil, server.URL, "hash")
	if err == nil {
		t.Fatal("expected error for invalid base64")
	}
	sdkErr := err.(*Error)
	if sdkErr.Kind != ErrInvalidResponse {
		t.Errorf("expected ErrInvalidResponse, got %v", sdkErr.Kind)
	}
}

func TestFetchKeyByHashTrimsTrailingSlash(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/jacs/v1/keys/by-hash/h1" {
			t.Errorf("unexpected path: %s", r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]string{
			"public_key": "dGVzdA==",
			"algorithm":  "ed25519",
		})
	}))
	defer server.Close()

	_, err := FetchKeyByHashFromURL(context.Background(), nil, server.URL+"/", "h1")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestFetchKeyByHashEnvDefault(t *testing.T) {
	// FetchKeyByHash (no URL variant) reads from HAI_KEYS_BASE_URL env.
	// We can't easily test it without a running server, but we can verify
	// the function signature exists by calling with a server.
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]string{
			"public_key": "dGVzdA==",
			"algorithm":  "ed25519",
		})
	}))
	defer server.Close()

	old := os.Getenv("HAI_KEYS_BASE_URL")
	defer func() {
		if old != "" {
			os.Setenv("HAI_KEYS_BASE_URL", old)
		} else {
			os.Unsetenv("HAI_KEYS_BASE_URL")
		}
	}()
	os.Setenv("HAI_KEYS_BASE_URL", server.URL)

	result, err := FetchKeyByHash(context.Background(), nil, "some-hash")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result.Algorithm != "ed25519" {
		t.Errorf("expected 'ed25519', got '%s'", result.Algorithm)
	}
}

func TestFetchKeyByHashNilClient(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]string{
			"public_key": "dGVzdA==",
			"algorithm":  "ed25519",
		})
	}))
	defer server.Close()

	// nil httpClient should use a default
	result, err := FetchKeyByHashFromURL(context.Background(), nil, server.URL, "h")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result == nil {
		t.Fatal("expected non-nil result")
	}
}
