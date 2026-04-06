package haiai

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"
)

// ===========================================================================
// CRITICAL #1: ProRun uses wrong endpoints
// ===========================================================================

func TestProRunUsesCorrectPurchaseEndpoint(t *testing.T) {
	var gotPurchasePath string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.HasPrefix(r.URL.Path, "/api/benchmark/purchase") {
			gotPurchasePath = r.URL.Path
			w.Header().Set("Content-Type", "application/json")
			// Return already_paid=true so we skip checkout flow
			_, _ = w.Write([]byte(`{"checkout_url":"","session_id":"sess-1","already_paid":true}`))
			return
		}
		if r.URL.Path == "/api/benchmark/run" {
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"benchmark_id":"bench-1","status":"running"}`))
			return
		}
		t.Fatalf("unexpected path: %s", r.URL.Path)
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.ProRun(context.Background(), nil)
	if err != nil {
		t.Fatalf("ProRun: %v", err)
	}
	if gotPurchasePath != "/api/benchmark/purchase" {
		t.Fatalf("expected /api/benchmark/purchase, got %q", gotPurchasePath)
	}
}

func TestProRunPollsCorrectPaymentStatusEndpoint(t *testing.T) {
	var gotStatusPath string
	callCount := 0

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/benchmark/purchase" {
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"checkout_url":"http://example.com/pay","session_id":"sess-123","already_paid":false}`))
			return
		}
		if strings.HasPrefix(r.URL.Path, "/api/benchmark/payments/") {
			gotStatusPath = r.URL.Path
			callCount++
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"paid":true}`))
			return
		}
		if r.URL.Path == "/api/benchmark/run" {
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"benchmark_id":"bench-1","status":"running"}`))
			return
		}
		t.Fatalf("unexpected path: %s", r.URL.Path)
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.ProRun(context.Background(), &ProRunOptions{
		OnCheckoutURL: func(url string) {
			// Test captures the URL but doesn't open a browser.
		},
		PollInterval: 100 * time.Millisecond,
	})
	if err != nil {
		t.Fatalf("ProRun: %v", err)
	}
	expected := "/api/benchmark/payments/sess-123/status"
	if gotStatusPath != expected {
		t.Fatalf("expected %q, got %q", expected, gotStatusPath)
	}
}

// ===========================================================================
// CRITICAL #2: Attestation path uses singular
// ===========================================================================

func TestGetAgentAttestationUsesPlural(t *testing.T) {
	var gotPath string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.EscapedPath()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"agent_id":"test","attestations":[]}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.GetAgentAttestation(context.Background())
	if err != nil {
		t.Fatalf("GetAgentAttestation: %v", err)
	}
	// GetAgentAttestation now delegates to VerifyStatus (ffi.VerifyStatus)
	// which calls /api/v1/agents/{id}/verify
	expected := "/api/v1/agents/test-agent-id/verify"
	if gotPath != expected {
		t.Fatalf("expected %q, got %q", expected, gotPath)
	}
}

// ===========================================================================
// HIGH #4: HTTP retry logic
// Tests removed: retry logic is now handled by the Rust FFI layer.
// ===========================================================================

// ===========================================================================
// HIGH #6: Free benchmark missing transport field
// ===========================================================================

func TestFreeBenchmarkIncludesTransportField(t *testing.T) {
	var gotBody map[string]interface{}

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/benchmark/run" {
			body, _ := io.ReadAll(r.Body)
			_ = json.Unmarshal(body, &gotBody)
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"benchmark_id":"b1","status":"running"}`))
			return
		}
		t.Fatalf("unexpected path: %s", r.URL.Path)
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.Benchmark(context.Background(), "free")
	if err != nil {
		t.Fatalf("Benchmark: %v", err)
	}
	transport, ok := gotBody["transport"]
	if !ok {
		t.Fatal("expected 'transport' field in request body, but it was missing")
	}
	if transport != "sse" {
		t.Fatalf("expected transport='sse', got %q", transport)
	}
}

// ===========================================================================
// HIGH #7: limitedReadAll helper
// ===========================================================================

func TestLimitedReadAllWithinLimit(t *testing.T) {
	data := bytes.Repeat([]byte("a"), 1000)
	result, err := limitedReadAll(io.NopCloser(bytes.NewReader(data)))
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(result) != 1000 {
		t.Fatalf("expected 1000 bytes, got %d", len(result))
	}
}

func TestLimitedReadAllExceedsLimit(t *testing.T) {
	data := bytes.Repeat([]byte("a"), maxResponseSize+1)
	_, err := limitedReadAll(io.NopCloser(bytes.NewReader(data)))
	if err == nil {
		t.Fatal("expected error when exceeding size limit")
	}
	if !strings.Contains(err.Error(), "exceeds") {
		t.Fatalf("unexpected error message: %v", err)
	}
}

// ===========================================================================
// MEDIUM #15: SSE connection via FFI (transport config moved to Rust)
// ===========================================================================

func TestSSEConnectionViaFFI(t *testing.T) {
	// SSE transport configuration (timeouts, headers) is now handled by the
	// Rust FFI layer. Verify that ConnectSSE delegates to FFI.
	mock := newMockFFIClient("http://localhost", "test-jacs-id", "JACS test:123:sig")
	cl := &Client{ffi: mock}

	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()

	// The default mock returns an error for ConnectSSE, confirming delegation.
	_, err := cl.ConnectSSE(ctx)
	if err == nil {
		t.Fatal("expected error from mock ConnectSSE")
	}
}

// ===========================================================================
// MEDIUM #16: Mutex protection for mutable fields
// ===========================================================================

func TestConcurrentAccessToMutableFields(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"status":"ok"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)

	var wg sync.WaitGroup
	wg.Add(4)

	// Concurrent reads/writes to haiAgentID and agentEmail
	go func() {
		defer wg.Done()
		for i := 0; i < 100; i++ {
			cl.SetHaiAgentID(fmt.Sprintf("id-%d", i))
		}
	}()
	go func() {
		defer wg.Done()
		for i := 0; i < 100; i++ {
			_ = cl.HaiAgentID()
		}
	}()
	go func() {
		defer wg.Done()
		for i := 0; i < 100; i++ {
			cl.SetAgentEmail(fmt.Sprintf("email-%d@hai.ai", i))
		}
	}()
	go func() {
		defer wg.Done()
		for i := 0; i < 100; i++ {
			_ = cl.AgentEmail()
		}
	}()

	wg.Wait()
}

// ===========================================================================
// MEDIUM #18: Query parameter gaps (since/until in SearchMessages)
// ===========================================================================

func TestSearchMessagesSendsDateFilters(t *testing.T) {
	var gotQuery string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotQuery = r.URL.RawQuery
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"messages":[],"total":0,"unread":0}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.SearchMessages(context.Background(), SearchOptions{
		Q:     "test",
		Since: "2026-01-01",
		Until: "2026-03-01",
	})
	if err != nil {
		t.Fatalf("SearchMessages: %v", err)
	}
	if !strings.Contains(gotQuery, "since=2026-01-01") {
		t.Fatalf("expected since parameter in query, got %q", gotQuery)
	}
	if !strings.Contains(gotQuery, "until=2026-03-01") {
		t.Fatalf("expected until parameter in query, got %q", gotQuery)
	}
}

func TestListMessagesSendsDateFilters(t *testing.T) {
	var gotQuery string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotQuery = r.URL.RawQuery
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"messages":[],"total":0,"unread":0}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.ListMessages(context.Background(), ListMessagesOptions{
		Limit: 10,
		Since: "2026-01-01",
		Until: "2026-03-01",
	})
	if err != nil {
		t.Fatalf("ListMessages: %v", err)
	}
	if !strings.Contains(gotQuery, "since=2026-01-01") {
		t.Fatalf("expected since parameter in query, got %q", gotQuery)
	}
	if !strings.Contains(gotQuery, "until=2026-03-01") {
		t.Fatalf("expected until parameter in query, got %q", gotQuery)
	}
}

// ===========================================================================
// MEDIUM #19: Key lookups use DefaultEndpoint (DefaultKeysEndpoint was removed)
// ===========================================================================

func TestFetchKeyByEmailDefaultsToMainEndpoint(t *testing.T) {
	// The standalone function requires a Client (FFI) -- passing nil returns an error.
	// Verify the function works when a client is provided.
	var gotPath string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.EscapedPath()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(keyResponseJSON()))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := FetchKeyByEmail(context.Background(), cl, "alice@hai.ai")
	if err != nil {
		t.Fatalf("FetchKeyByEmail: %v", err)
	}
	// The path should start with /api/agents/keys/ (not /jacs/v1/...)
	if !strings.HasPrefix(gotPath, "/api/agents/keys/") {
		t.Fatalf("expected /api/agents/keys/ prefix, got %q", gotPath)
	}
}

func TestFetchKeyByDomainFromURL_ReturnsDeprecatedError(t *testing.T) {
	// FetchKeyByDomainFromURL is deprecated -- native HTTP fallback removed.
	// Verify it returns a deprecation error.
	_, err := FetchKeyByDomainFromURL(context.Background(), nil, "http://unused", "example.com")
	if err == nil {
		t.Fatal("expected error from deprecated FetchKeyByDomainFromURL")
	}
	if !strings.Contains(err.Error(), "deprecated") {
		t.Fatalf("expected deprecation error, got: %v", err)
	}
}

func TestFetchKeyByHashFromURL_ReturnsDeprecatedError(t *testing.T) {
	// FetchKeyByHashFromURL is deprecated -- native HTTP fallback removed.
	_, err := FetchKeyByHashFromURL(context.Background(), nil, "http://unused", "sha256:abcdef")
	if err == nil {
		t.Fatal("expected error from deprecated FetchKeyByHashFromURL")
	}
	if !strings.Contains(err.Error(), "deprecated") {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestFetchAllKeysFromURL_ReturnsDeprecatedError(t *testing.T) {
	// FetchAllKeysFromURL is deprecated -- native HTTP fallback removed.
	_, err := FetchAllKeysFromURL(context.Background(), nil, "http://unused", "agent-abc")
	if err == nil {
		t.Fatal("expected error from deprecated FetchAllKeysFromURL")
	}
	if !strings.Contains(err.Error(), "deprecated") {
		t.Fatalf("unexpected error: %v", err)
	}
}
