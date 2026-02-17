package haisdk

import (
	"context"
	"crypto/ed25519"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

// newTestClient creates a Client pointing at a test server with a generated key pair.
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
	return cl, pub
}

func TestNewClientWithOptions(t *testing.T) {
	_, priv, _ := GenerateKeyPair()
	cl, err := NewClient(
		WithEndpoint("https://example.com/"),
		WithJACSID("my-agent"),
		WithPrivateKey(priv),
	)
	if err != nil {
		t.Fatalf("NewClient: %v", err)
	}

	if cl.Endpoint() != "https://example.com" {
		t.Errorf("expected endpoint 'https://example.com', got '%s'", cl.Endpoint())
	}
	if cl.JacsID() != "my-agent" {
		t.Errorf("expected jacsID 'my-agent', got '%s'", cl.JacsID())
	}
}

func TestNewClientTrimsTrailingSlash(t *testing.T) {
	_, priv, _ := GenerateKeyPair()
	cl, err := NewClient(
		WithEndpoint("https://api.hai.ai///"),
		WithJACSID("agent"),
		WithPrivateKey(priv),
	)
	if err != nil {
		t.Fatalf("NewClient: %v", err)
	}
	if cl.Endpoint() != "https://api.hai.ai" {
		t.Errorf("expected trimmed endpoint, got '%s'", cl.Endpoint())
	}
}

func TestNewClientRequiresJacsID(t *testing.T) {
	_, priv, _ := GenerateKeyPair()
	_, err := NewClient(
		WithEndpoint("https://api.hai.ai"),
		WithJACSID(""),
		WithPrivateKey(priv),
	)
	// Should fail because jacsID is empty and no config to discover
	if err == nil {
		t.Fatal("expected error when jacsID is empty and no config")
	}
}

func TestTestConnection(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/health" {
			t.Errorf("expected path '/health', got '%s'", r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]string{"status": "ok"})
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	ok, err := cl.TestConnection(context.Background())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !ok {
		t.Error("expected connection test to succeed")
	}
}

func TestTestConnectionFailure(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	_, err := cl.TestConnection(context.Background())
	if err == nil {
		t.Fatal("expected error for 500 response")
	}

	sdkErr, ok := err.(*Error)
	if !ok {
		t.Fatalf("expected *Error, got %T", err)
	}
	if sdkErr.Kind != ErrConnection {
		t.Errorf("expected ErrConnection, got %v", sdkErr.Kind)
	}
}

func TestHello(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/hello" {
			t.Errorf("expected path '/api/v1/hello', got '%s'", r.URL.Path)
		}

		// Verify JACS auth header is present
		auth := r.Header.Get("Authorization")
		if !strings.HasPrefix(auth, "JACS ") {
			t.Errorf("expected JACS auth header, got '%s'", auth)
		}

		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(HelloResult{
			Message: "hello",
			AgentID: "test-agent-id",
		})
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	result, err := cl.Hello(context.Background())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result.Message != "hello" {
		t.Errorf("expected message 'hello', got '%s'", result.Message)
	}
}

func TestStatusSuccess(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// Verify JACS auth
		auth := r.Header.Get("Authorization")
		if !strings.HasPrefix(auth, "JACS ") {
			w.WriteHeader(http.StatusUnauthorized)
			return
		}

		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]interface{}{
			"registered":      true,
			"agent_id":        "test-agent-id",
			"registration_id": "reg-123",
			"registered_at":   "2024-01-15T10:30:00Z",
			"hai_signatures":  []string{"sig-1", "sig-2"},
		})
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	result, err := cl.Status(context.Background())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if !result.Registered {
		t.Error("expected Registered to be true")
	}
	if result.RegistrationID != "reg-123" {
		t.Errorf("expected RegistrationID 'reg-123', got '%s'", result.RegistrationID)
	}
	if len(result.HaiSignatures) != 2 {
		t.Errorf("expected 2 signatures, got %d", len(result.HaiSignatures))
	}
}

func TestStatusNotFound(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNotFound)
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	result, err := cl.Status(context.Background())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if result.Registered {
		t.Error("expected Registered to be false for 404")
	}
}

func TestRegister(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != "POST" {
			t.Errorf("expected POST, got %s", r.Method)
		}
		if r.URL.Path != "/api/v1/agents/register" {
			t.Errorf("expected path '/api/v1/agents/register', got '%s'", r.URL.Path)
		}

		var reqBody struct {
			AgentJSON string `json:"agent_json"`
		}
		json.NewDecoder(r.Body).Decode(&reqBody)

		if reqBody.AgentJSON == "" {
			t.Error("expected non-empty agent_json")
		}

		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]interface{}{
			"agent_id":     "agent-123",
			"jacs_id":      "jacs-456",
			"dns_verified": true,
			"signatures": []map[string]string{
				{
					"key_id":    "key-1",
					"algorithm": "Ed25519",
					"signature": "c2lnbmF0dXJl",
					"signed_at": "2024-01-15T10:30:00Z",
				},
			},
		})
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	result, err := cl.Register(context.Background(), `{"jacsId": "test-agent"}`)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if result.AgentID != "agent-123" {
		t.Errorf("expected AgentID 'agent-123', got '%s'", result.AgentID)
	}
	if result.JacsID != "jacs-456" {
		t.Errorf("expected JacsID 'jacs-456', got '%s'", result.JacsID)
	}
	if !result.DNSVerified {
		t.Error("expected DNSVerified to be true")
	}
	if len(result.Signatures) != 1 {
		t.Errorf("expected 1 signature, got %d", len(result.Signatures))
	}
}

func TestBenchmark(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != "POST" {
			t.Errorf("expected POST, got %s", r.Method)
		}
		if r.URL.Path != "/api/v1/benchmarks/run" {
			t.Errorf("expected path '/api/v1/benchmarks/run', got '%s'", r.URL.Path)
		}

		var reqBody struct {
			AgentID string `json:"agent_id"`
			Suite   string `json:"suite"`
		}
		json.NewDecoder(r.Body).Decode(&reqBody)

		if reqBody.Suite != "free_chaotic" {
			t.Errorf("expected suite 'free_chaotic', got '%s'", reqBody.Suite)
		}

		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]interface{}{
			"run_id":       "run-123",
			"suite":        "free_chaotic",
			"score":        0.85,
			"completed_at": "2024-01-15T10:30:00Z",
			"results": []map[string]interface{}{
				{"name": "test-1", "passed": true, "score": 1.0},
			},
		})
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	result, err := cl.FreeChaoticRun(context.Background())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if result.RunID != "run-123" {
		t.Errorf("expected RunID 'run-123', got '%s'", result.RunID)
	}
	if result.Score != 0.85 {
		t.Errorf("expected Score 0.85, got %f", result.Score)
	}
}

func TestSubmitResponse(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != "POST" {
			t.Errorf("expected POST, got %s", r.Method)
		}
		if !strings.HasPrefix(r.URL.Path, "/api/v1/agents/jobs/") {
			t.Errorf("unexpected path: %s", r.URL.Path)
		}

		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(JobResponseResult{
			Success: true,
			JobID:   "job-42",
			Message: "Response accepted",
		})
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	result, err := cl.SubmitResponse(context.Background(), "job-42", ModerationResponse{
		Message: "Let us take a moment to understand each other.",
	})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if !result.Success {
		t.Error("expected Success to be true")
	}
	if result.JobID != "job-42" {
		t.Errorf("expected JobID 'job-42', got '%s'", result.JobID)
	}
}

func TestFetchRemoteKey(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		expectedPath := "/jacs/v1/agents/test-agent/keys/latest"
		if r.URL.Path != expectedPath {
			t.Errorf("expected path '%s', got '%s'", expectedPath, r.URL.Path)
		}

		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]string{
			"public_key":      "dGVzdC1rZXk=",
			"algorithm":       "ed25519",
			"public_key_hash": "abc123",
			"agent_id":        "test-agent",
			"version":         "1",
		})
	}))
	defer server.Close()

	result, err := FetchRemoteKeyFromURL(context.Background(), nil, server.URL, "test-agent", "latest")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if string(result.PublicKey) != "test-key" {
		t.Errorf("expected PublicKey 'test-key', got '%s'", string(result.PublicKey))
	}
	if result.Algorithm != "ed25519" {
		t.Errorf("expected Algorithm 'ed25519', got '%s'", result.Algorithm)
	}
}

func TestFetchRemoteKeyNotFound(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNotFound)
	}))
	defer server.Close()

	_, err := FetchRemoteKeyFromURL(context.Background(), nil, server.URL, "unknown", "latest")
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

func TestHTTPErrorClassification(t *testing.T) {
	tests := []struct {
		status int
		kind   ErrorKind
	}{
		{401, ErrAuthRequired},
		{403, ErrForbidden},
		{404, ErrNotFound},
		{429, ErrRateLimited},
		{500, ErrInvalidResponse},
	}

	for _, tt := range tests {
		err := classifyHTTPError(tt.status, []byte("test"))
		if err.Kind != tt.kind {
			t.Errorf("status %d: expected %v, got %v", tt.status, tt.kind, err.Kind)
		}
	}
}

func TestContextCancellation(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		time.Sleep(5 * time.Second)
		w.WriteHeader(http.StatusOK)
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)

	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancel()

	_, err := cl.Hello(ctx)
	if err == nil {
		t.Fatal("expected error from cancelled context")
	}
}
