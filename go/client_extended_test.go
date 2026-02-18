package haisdk

import (
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"
)

// ===========================================================================
// Hello endpoint tests
// ===========================================================================

func TestHelloAuthHeaderFormat(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		auth := r.Header.Get("Authorization")
		if !strings.HasPrefix(auth, "JACS ") {
			t.Errorf("expected JACS prefix, got '%s'", auth)
		}
		parts := strings.SplitN(strings.TrimPrefix(auth, "JACS "), ":", 3)
		if len(parts) != 3 {
			t.Errorf("expected 3 colon-separated parts, got %d", len(parts))
		}
		if parts[0] != "test-agent-id" {
			t.Errorf("expected jacsID 'test-agent-id', got '%s'", parts[0])
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(HelloResult{Message: "ok"})
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	_, err := cl.Hello(context.Background())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestHelloNoBearer(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		auth := r.Header.Get("Authorization")
		if strings.Contains(auth, "Bearer") {
			t.Error("Authorization header should NOT contain Bearer")
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(HelloResult{Message: "ok"})
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	_, _ = cl.Hello(context.Background())
}

func TestHello401(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusUnauthorized)
		w.Write([]byte(`{"error":"invalid signature"}`))
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	_, err := cl.Hello(context.Background())
	if err == nil {
		t.Fatal("expected error for 401")
	}
	sdkErr, ok := err.(*Error)
	if !ok {
		t.Fatalf("expected *Error, got %T", err)
	}
	if sdkErr.Kind != ErrAuthRequired {
		t.Errorf("expected ErrAuthRequired, got %v", sdkErr.Kind)
	}
}

func TestHello429(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusTooManyRequests)
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	_, err := cl.Hello(context.Background())
	if err == nil {
		t.Fatal("expected error for 429")
	}
	sdkErr := err.(*Error)
	if sdkErr.Kind != ErrRateLimited {
		t.Errorf("expected ErrRateLimited, got %v", sdkErr.Kind)
	}
}

// ===========================================================================
// Register endpoint tests
// ===========================================================================

func TestRegister401(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusUnauthorized)
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	_, err := cl.Register(context.Background(), `{"jacsId":"test"}`)
	if err == nil {
		t.Fatal("expected error for 401")
	}
	sdkErr := err.(*Error)
	if sdkErr.Kind != ErrAuthRequired {
		t.Errorf("expected ErrAuthRequired, got %v", sdkErr.Kind)
	}
}

func TestRegisterContentType(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		ct := r.Header.Get("Content-Type")
		if ct != "application/json" {
			t.Errorf("expected 'application/json', got '%s'", ct)
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(RegistrationResult{AgentID: "a"})
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	_, _ = cl.Register(context.Background(), `{}`)
}

// ===========================================================================
// Status endpoint tests
// ===========================================================================

func TestStatus401(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusUnauthorized)
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	_, err := cl.Status(context.Background())
	if err == nil {
		t.Fatal("expected error for 401")
	}
	sdkErr := err.(*Error)
	if sdkErr.Kind != ErrAuthRequired {
		t.Errorf("expected ErrAuthRequired, got %v", sdkErr.Kind)
	}
}

func TestStatus500(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
		w.Write([]byte("internal error"))
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	_, err := cl.Status(context.Background())
	if err == nil {
		t.Fatal("expected error for 500")
	}
	sdkErr := err.(*Error)
	if sdkErr.Kind != ErrInvalidResponse {
		t.Errorf("expected ErrInvalidResponse, got %v", sdkErr.Kind)
	}
}

func TestStatusSetsAgentIDIfEmpty(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]interface{}{
			"registered": true,
		})
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	result, err := cl.Status(context.Background())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result.AgentID != "test-agent-id" {
		t.Errorf("expected AgentID to default to jacsID, got '%s'", result.AgentID)
	}
}

// ===========================================================================
// Benchmark endpoint tests
// ===========================================================================

func TestBaselineRun(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body struct {
			Suite string `json:"suite"`
		}
		json.NewDecoder(r.Body).Decode(&body)
		if body.Suite != "baseline" {
			t.Errorf("expected suite 'baseline', got '%s'", body.Suite)
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(BenchmarkResult{RunID: "run-bl"})
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	result, err := cl.BaselineRun(context.Background())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result.RunID != "run-bl" {
		t.Errorf("expected RunID 'run-bl', got '%s'", result.RunID)
	}
}

func TestBenchmark403(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusForbidden)
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	_, err := cl.Benchmark(context.Background(), "certified")
	if err == nil {
		t.Fatal("expected error for 403")
	}
	sdkErr := err.(*Error)
	if sdkErr.Kind != ErrForbidden {
		t.Errorf("expected ErrForbidden, got %v", sdkErr.Kind)
	}
}

// ===========================================================================
// SubmitResponse endpoint tests
// ===========================================================================

func TestSubmitResponseVerifiesPath(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		expectedPath := "/api/v1/agents/jobs/job-99/response"
		if r.URL.Path != expectedPath {
			t.Errorf("expected path '%s', got '%s'", expectedPath, r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(JobResponseResult{Success: true, JobID: "job-99"})
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	_, _ = cl.SubmitResponse(context.Background(), "job-99", ModerationResponse{Message: "ok"})
}

func TestSubmitResponse404(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNotFound)
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	_, err := cl.SubmitResponse(context.Background(), "nonexistent", ModerationResponse{Message: "x"})
	if err == nil {
		t.Fatal("expected error for 404")
	}
	sdkErr := err.(*Error)
	if sdkErr.Kind != ErrNotFound {
		t.Errorf("expected ErrNotFound, got %v", sdkErr.Kind)
	}
}

// ===========================================================================
// GetAgentAttestation endpoint tests
// ===========================================================================

func TestGetAgentAttestationSuccess(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		expectedPath := "/api/v1/agents/test-agent-id/attestation"
		if r.URL.Path != expectedPath {
			t.Errorf("expected path '%s', got '%s'", expectedPath, r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(AttestationResult{
			AgentID: "test-agent-id",
			Signatures: []HaiSignature{
				{KeyID: "k1", Algorithm: "Ed25519"},
			},
		})
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	result, err := cl.GetAgentAttestation(context.Background())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result.AgentID != "test-agent-id" {
		t.Errorf("expected AgentID, got '%s'", result.AgentID)
	}
	if len(result.Signatures) != 1 {
		t.Errorf("expected 1 signature, got %d", len(result.Signatures))
	}
}

func TestGetAgentAttestation404(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNotFound)
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	_, err := cl.GetAgentAttestation(context.Background())
	if err == nil {
		t.Fatal("expected error for 404")
	}
}

// ===========================================================================
// VerifyAgent endpoint tests
// ===========================================================================

func TestVerifyAgentSuccess(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if !strings.Contains(r.URL.Path, "/other-agent/verify") {
			t.Errorf("unexpected path: %s", r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(VerifyResult{Valid: true, AgentID: "other-agent"})
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	result, err := cl.VerifyAgent(context.Background(), "other-agent")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !result.Valid {
		t.Error("expected Valid to be true")
	}
}

func TestVerifyAgentNotFound(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNotFound)
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	_, err := cl.VerifyAgent(context.Background(), "unknown")
	if err == nil {
		t.Fatal("expected error for unknown agent")
	}
}

// ===========================================================================
// Client configuration tests
// ===========================================================================

func TestDefaultEndpoint(t *testing.T) {
	if DefaultEndpoint != "https://api.hai.ai" {
		t.Errorf("unexpected default endpoint: %s", DefaultEndpoint)
	}
}

func TestWithTimeout(t *testing.T) {
	_, priv, _ := GenerateKeyPair()
	cl, err := NewClient(
		WithEndpoint("https://example.com"),
		WithJACSID("agent"),
		WithPrivateKey(priv),
		WithTimeout(5*time.Second),
	)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if cl.httpClient.Timeout != 5*time.Second {
		t.Errorf("expected 5s timeout, got %v", cl.httpClient.Timeout)
	}
}

func TestWithCustomHTTPClient(t *testing.T) {
	_, priv, _ := GenerateKeyPair()
	custom := &http.Client{Timeout: 99 * time.Second}
	cl, err := NewClient(
		WithEndpoint("https://example.com"),
		WithJACSID("agent"),
		WithPrivateKey(priv),
		WithHTTPClient(custom),
	)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if cl.httpClient != custom {
		t.Error("expected custom HTTP client to be used")
	}
}

func TestClientAccessors(t *testing.T) {
	_, priv, _ := GenerateKeyPair()
	cl, _ := NewClient(
		WithEndpoint("https://test.hai.ai"),
		WithJACSID("my-agent"),
		WithPrivateKey(priv),
	)
	if cl.Endpoint() != "https://test.hai.ai" {
		t.Errorf("expected endpoint, got '%s'", cl.Endpoint())
	}
	if cl.JacsID() != "my-agent" {
		t.Errorf("expected jacsID, got '%s'", cl.JacsID())
	}
}

// ===========================================================================
// Context cancellation / timeout tests
// ===========================================================================

func TestStatusContextCancellation(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		time.Sleep(5 * time.Second)
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()

	_, err := cl.Status(ctx)
	if err == nil {
		t.Fatal("expected error from cancelled context")
	}
}

func TestRegisterContextCancellation(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		time.Sleep(5 * time.Second)
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()

	_, err := cl.Register(ctx, `{}`)
	if err == nil {
		t.Fatal("expected error from cancelled context")
	}
}

// ===========================================================================
// FetchRemoteKey edge cases
// ===========================================================================

func TestFetchRemoteKey500(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
		w.Write([]byte("server error"))
	}))
	defer server.Close()

	_, err := FetchRemoteKeyFromURL(context.Background(), nil, server.URL, "agent", "v1")
	if err == nil {
		t.Fatal("expected error for 500")
	}
}

func TestFetchRemoteKeyInvalidBase64(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]string{
			"public_key": "not-valid-base64!!!",
			"algorithm":  "ed25519",
		})
	}))
	defer server.Close()

	_, err := FetchRemoteKeyFromURL(context.Background(), nil, server.URL, "agent", "v1")
	if err == nil {
		t.Fatal("expected error for invalid base64")
	}
	sdkErr := err.(*Error)
	if sdkErr.Kind != ErrInvalidResponse {
		t.Errorf("expected ErrInvalidResponse, got %v", sdkErr.Kind)
	}
}

func TestFetchRemoteKeyNilHTTPClient(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]string{
			"public_key": "dGVzdA==",
			"algorithm":  "ed25519",
		})
	}))
	defer server.Close()

	result, err := FetchRemoteKeyFromURL(context.Background(), nil, server.URL, "agent", "v1")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result.Algorithm != "ed25519" {
		t.Errorf("expected algorithm 'ed25519', got '%s'", result.Algorithm)
	}
}

// ===========================================================================
// classifyHTTPError comprehensive
// ===========================================================================

func TestClassifyHTTPError200(t *testing.T) {
	// 200 should not be classified since it's not an error, but test the range
	err := classifyHTTPError(502, []byte("bad gateway"))
	if err.Kind != ErrInvalidResponse {
		t.Errorf("expected ErrInvalidResponse for 502, got %v", err.Kind)
	}
}

func TestClassifyHTTPErrorMessageContainsBody(t *testing.T) {
	err := classifyHTTPError(401, []byte("auth failed"))
	if !strings.Contains(err.Message, "auth failed") {
		t.Errorf("error message should contain response body, got: %s", err.Message)
	}
}

// ===========================================================================
// Type serialization / round-trip tests
// ===========================================================================

func TestRegistrationResultJSON(t *testing.T) {
	r := RegistrationResult{
		AgentID:     "a1",
		JacsID:      "j1",
		DNSVerified: true,
		Signatures:  []HaiSignature{{KeyID: "k1", Algorithm: "Ed25519", Signature: "sig", SignedAt: "2025-01-01T00:00:00Z"}},
	}
	data, err := json.Marshal(r)
	if err != nil {
		t.Fatalf("marshal failed: %v", err)
	}
	var r2 RegistrationResult
	if err := json.Unmarshal(data, &r2); err != nil {
		t.Fatalf("unmarshal failed: %v", err)
	}
	if r2.AgentID != r.AgentID || r2.JacsID != r.JacsID || !r2.DNSVerified || len(r2.Signatures) != 1 {
		t.Errorf("roundtrip mismatch: %+v", r2)
	}
}

func TestStatusResultJSON(t *testing.T) {
	s := StatusResult{
		Registered:     true,
		AgentID:        "a1",
		RegistrationID: "reg-1",
		RegisteredAt:   "2025-01-01",
		HaiSignatures:  []string{"sig1", "sig2"},
	}
	data, err := json.Marshal(s)
	if err != nil {
		t.Fatalf("marshal failed: %v", err)
	}
	var s2 StatusResult
	if err := json.Unmarshal(data, &s2); err != nil {
		t.Fatalf("unmarshal failed: %v", err)
	}
	if !s2.Registered || s2.AgentID != "a1" || s2.RegistrationID != "reg-1" || len(s2.HaiSignatures) != 2 {
		t.Errorf("roundtrip mismatch: %+v", s2)
	}
}

func TestBenchmarkResultJSON(t *testing.T) {
	br := BenchmarkResult{
		RunID: "run-1",
		Suite: "baseline",
		Score: 0.85,
		Results: []BenchmarkTestResult{
			{Name: "test1", Passed: true, Score: 1.0},
			{Name: "test2", Passed: false, Score: 0.0, Message: "failed"},
		},
		CompletedAt: "2025-01-01T00:00:00Z",
	}
	data, _ := json.Marshal(br)
	var br2 BenchmarkResult
	json.Unmarshal(data, &br2)
	if br2.RunID != "run-1" || br2.Score != 0.85 || len(br2.Results) != 2 {
		t.Errorf("roundtrip mismatch: %+v", br2)
	}
	if br2.Results[1].Message != "failed" {
		t.Errorf("expected message 'failed', got '%s'", br2.Results[1].Message)
	}
}

func TestJobResponseResultJSON(t *testing.T) {
	j := JobResponseResult{Success: true, JobID: "j1", Message: "ok"}
	data, _ := json.Marshal(j)
	var j2 JobResponseResult
	json.Unmarshal(data, &j2)
	if !j2.Success || j2.JobID != "j1" || j2.Message != "ok" {
		t.Errorf("roundtrip mismatch: %+v", j2)
	}
}

func TestVerifyResultJSON(t *testing.T) {
	v := VerifyResult{
		Valid:   false,
		AgentID: "a1",
		Errors:  []string{"expired", "revoked"},
	}
	data, _ := json.Marshal(v)
	var v2 VerifyResult
	json.Unmarshal(data, &v2)
	if v2.Valid || v2.AgentID != "a1" || len(v2.Errors) != 2 {
		t.Errorf("roundtrip mismatch: %+v", v2)
	}
}

func TestPublicKeyInfoJSON(t *testing.T) {
	p := PublicKeyInfo{
		PublicKey:     []byte{1, 2, 3},
		Algorithm:     "ed25519",
		PublicKeyHash: "abc123",
		AgentID:       "a1",
		Version:       "v1",
	}
	data, _ := json.Marshal(p)
	var p2 PublicKeyInfo
	json.Unmarshal(data, &p2)
	if p2.Algorithm != "ed25519" || p2.AgentID != "a1" || len(p2.PublicKey) != 3 {
		t.Errorf("roundtrip mismatch: %+v", p2)
	}
}

// ===========================================================================
// Auth header signature verification tests
// ===========================================================================

func TestBuildAuthHeaderSignatureIsVerifiable(t *testing.T) {
	pub, priv, _ := GenerateKeyPair()
	jacsID := "test-agent"
	header := BuildAuthHeader(jacsID, priv)

	parts := strings.SplitN(strings.TrimPrefix(header, "JACS "), ":", 3)
	if len(parts) != 3 {
		t.Fatalf("expected 3 parts, got %d", len(parts))
	}

	message := fmt.Sprintf("%s:%s", parts[0], parts[1])
	sigBytes, err := base64.StdEncoding.DecodeString(parts[2])
	if err != nil {
		t.Fatalf("invalid base64 signature: %v", err)
	}

	if !ed25519.Verify(pub, []byte(message), sigBytes) {
		t.Error("signature verification failed")
	}
}

func TestBuildAuthHeaderTimestampIsRecent(t *testing.T) {
	_, priv, _ := GenerateKeyPair()
	header := BuildAuthHeader("agent", priv)
	parts := strings.SplitN(strings.TrimPrefix(header, "JACS "), ":", 3)

	ts, err := strconv.ParseInt(parts[1], 10, 64)
	if err != nil {
		t.Fatalf("invalid timestamp: %v", err)
	}

	now := time.Now().Unix()
	diff := now - ts
	if diff < 0 || diff > 2 {
		t.Errorf("timestamp should be within 2 seconds of now, diff=%d", diff)
	}
}

// ===========================================================================
// Config loading tests
// ===========================================================================

func TestLoadConfigSnakeCaseFields(t *testing.T) {
	tmpDir := t.TempDir()
	cfgPath := filepath.Join(tmpDir, "jacs.config.json")
	os.WriteFile(cfgPath, []byte(`{
		"jacsAgentName": "my-bot",
		"jacsAgentVersion": "2.0",
		"jacsKeyDir": "/keys",
		"jacsId": "my-id"
	}`), 0644)

	cfg, err := LoadConfig(cfgPath)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if cfg.JacsAgentName != "my-bot" {
		t.Errorf("expected 'my-bot', got '%s'", cfg.JacsAgentName)
	}
	if cfg.JacsID != "my-id" {
		t.Errorf("expected 'my-id', got '%s'", cfg.JacsID)
	}
}

func TestResolveKeyPathCustomDir(t *testing.T) {
	cfg := &Config{
		JacsAgentName: "mybot",
		JacsKeyDir:    "/custom/keys",
	}
	path := ResolveKeyPath(cfg, "/etc/jacs.config.json")
	expected := "/custom/keys/mybot.private.pem"
	if path != expected {
		t.Errorf("expected '%s', got '%s'", expected, path)
	}
}

func TestDiscoverConfigEnvVar(t *testing.T) {
	tmpDir := t.TempDir()
	cfgPath := filepath.Join(tmpDir, "jacs.config.json")
	os.WriteFile(cfgPath, []byte(`{"jacsAgentName":"env-bot","jacsId":"e1"}`), 0644)

	old := os.Getenv("JACS_CONFIG_PATH")
	defer func() {
		if old != "" {
			os.Setenv("JACS_CONFIG_PATH", old)
		} else {
			os.Unsetenv("JACS_CONFIG_PATH")
		}
	}()
	os.Setenv("JACS_CONFIG_PATH", cfgPath)

	cfg, err := DiscoverConfig()
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if cfg.JacsAgentName != "env-bot" {
		t.Errorf("expected 'env-bot', got '%s'", cfg.JacsAgentName)
	}
}

// ===========================================================================
// Error kind classification tests
// ===========================================================================

func TestClassifyHTTPError403(t *testing.T) {
	err := classifyHTTPError(403, []byte("forbidden"))
	if err.Kind != ErrForbidden {
		t.Errorf("expected ErrForbidden, got %v", err.Kind)
	}
}

func TestClassifyHTTPError404(t *testing.T) {
	err := classifyHTTPError(404, []byte("not found"))
	if err.Kind != ErrNotFound {
		t.Errorf("expected ErrNotFound, got %v", err.Kind)
	}
}

func TestClassifyHTTPError429(t *testing.T) {
	err := classifyHTTPError(429, []byte("too many"))
	if err.Kind != ErrRateLimited {
		t.Errorf("expected ErrRateLimited, got %v", err.Kind)
	}
}

func TestErrorUnwrap(t *testing.T) {
	inner := fmt.Errorf("inner error")
	err := wrapError(ErrConnection, inner, "outer")
	if err.Unwrap() != inner {
		t.Error("Unwrap should return inner error")
	}
}

func TestErrorMessageFormat(t *testing.T) {
	inner := fmt.Errorf("database timeout")
	err := wrapError(ErrConnection, inner, "connection failed")
	msg := err.Error()
	if !strings.Contains(msg, "connection failed") || !strings.Contains(msg, "database timeout") {
		t.Errorf("error message should contain both messages, got: %s", msg)
	}
}

// ===========================================================================
// FreeChaoticRun test
// ===========================================================================

func TestFreeChaoticRun(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body struct {
			Suite string `json:"suite"`
		}
		json.NewDecoder(r.Body).Decode(&body)
		if body.Suite != "free_chaotic" {
			t.Errorf("expected suite 'free_chaotic', got '%s'", body.Suite)
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(BenchmarkResult{RunID: "run-fc"})
	}))
	defer server.Close()

	cl, _ := newTestClient(t, server.URL)
	result, err := cl.FreeChaoticRun(context.Background())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result.RunID != "run-fc" {
		t.Errorf("expected RunID 'run-fc', got '%s'", result.RunID)
	}
}

// ===========================================================================
// Additional signing edge-case tests
// ===========================================================================

func TestSignEmptyMessage(t *testing.T) {
	_, priv, _ := GenerateKeyPair()
	sig := Sign(priv, []byte{})
	if len(sig) != ed25519.SignatureSize {
		t.Errorf("expected %d byte signature, got %d", ed25519.SignatureSize, len(sig))
	}
}

func TestVerifyEmptyPublicKey(t *testing.T) {
	if Verify(nil, []byte("msg"), []byte("sig")) {
		t.Error("Verify should return false for nil public key")
	}
}

func TestVerifyShortPublicKey(t *testing.T) {
	if Verify([]byte{1, 2, 3}, []byte("msg"), []byte("sig")) {
		t.Error("Verify should return false for short public key")
	}
}
