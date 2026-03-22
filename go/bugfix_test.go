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
	expected := "/api/v1/agents/test-agent-id/attestations"
	if gotPath != expected {
		t.Fatalf("expected %q, got %q", expected, gotPath)
	}
}

// ===========================================================================
// HIGH #4: HTTP retry logic
// ===========================================================================

func TestDoRequestRetriesOnRetryableStatusCodes(t *testing.T) {
	retryableCodes := []int{429, 500, 502, 503, 504}

	for _, code := range retryableCodes {
		t.Run(fmt.Sprintf("status_%d", code), func(t *testing.T) {
			attempts := 0
			srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				attempts++
				if attempts < 3 {
					w.WriteHeader(code)
					_, _ = w.Write([]byte(`{"error":"temporary"}`))
					return
				}
				w.Header().Set("Content-Type", "application/json")
				_, _ = w.Write([]byte(`{"status":"ok"}`))
			}))
			defer srv.Close()

			cl, _ := newTestClient(t, srv.URL)
			var result map[string]string
			err := cl.doRequest(context.Background(), http.MethodGet, "/test", nil, &result)
			if err != nil {
				t.Fatalf("expected success after retries, got: %v", err)
			}
			if attempts != 3 {
				t.Fatalf("expected 3 attempts, got %d", attempts)
			}
		})
	}
}

func TestDoRequestDoesNotRetryNonRetryableCodes(t *testing.T) {
	nonRetryable := []int{400, 401, 403, 404, 405}

	for _, code := range nonRetryable {
		t.Run(fmt.Sprintf("status_%d", code), func(t *testing.T) {
			attempts := 0
			srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				attempts++
				w.WriteHeader(code)
				_, _ = w.Write([]byte(`{"error":"permanent"}`))
			}))
			defer srv.Close()

			cl, _ := newTestClient(t, srv.URL)
			_ = cl.doRequest(context.Background(), http.MethodGet, "/test", nil, nil)
			if attempts != 1 {
				t.Fatalf("expected 1 attempt for status %d, got %d", code, attempts)
			}
		})
	}
}

func TestDoRequestExhaustsRetriesThenFails(t *testing.T) {
	attempts := 0
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		attempts++
		w.WriteHeader(503)
		_, _ = w.Write([]byte(`{"error":"always failing"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	err := cl.doRequest(context.Background(), http.MethodGet, "/test", nil, nil)
	if err == nil {
		t.Fatal("expected error after exhausting retries")
	}
	// Default 3 retries = 1 initial + 3 retries = 4 total
	if attempts != 4 {
		t.Fatalf("expected 4 attempts (1 + 3 retries), got %d", attempts)
	}
}

func TestWithMaxRetriesOption(t *testing.T) {
	attempts := 0
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		attempts++
		w.WriteHeader(503)
		_, _ = w.Write([]byte(`{"error":"always failing"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	cl.maxRetries = 5
	err := cl.doRequest(context.Background(), http.MethodGet, "/test", nil, nil)
	if err == nil {
		t.Fatal("expected error after exhausting retries")
	}
	// 1 initial + 5 retries = 6 total
	if attempts != 6 {
		t.Fatalf("expected 6 attempts (1 + 5 retries), got %d", attempts)
	}
}

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
	if !strings.Contains(err.Error(), "response body exceeds") {
		t.Fatalf("unexpected error message: %v", err)
	}
}

// ===========================================================================
// MEDIUM #15: SSE http.Client has ResponseHeaderTimeout
// ===========================================================================

func TestSSEClientHasResponseHeaderTimeout(t *testing.T) {
	// We can't easily test the internals of ConnectSSE without a real server,
	// but we can verify the transport configuration function exists and
	// returns the right config.
	transport := newSSETransport()
	if transport.ResponseHeaderTimeout != 30*time.Second {
		t.Fatalf("expected ResponseHeaderTimeout of 30s, got %v", transport.ResponseHeaderTimeout)
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
// MEDIUM #19: Key lookups should use DefaultEndpoint, not DefaultKeysEndpoint
// ===========================================================================

func TestFetchKeyByEmailDefaultsToMainEndpoint(t *testing.T) {
	// The standalone function should default to DefaultEndpoint (beta.hai.ai)
	// when HAI_KEYS_BASE_URL is not set. We override via env for test isolation.
	var gotPath string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.EscapedPath()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(keyResponseJSON()))
	}))
	defer srv.Close()

	t.Setenv("HAI_KEYS_BASE_URL", srv.URL)
	_, err := FetchKeyByEmail(context.Background(), nil, "alice@hai.ai")
	if err != nil {
		t.Fatalf("FetchKeyByEmail: %v", err)
	}
	// The path should start with /api/agents/keys/ (not /jacs/v1/...)
	if !strings.HasPrefix(gotPath, "/api/agents/keys/") {
		t.Fatalf("expected /api/agents/keys/ prefix, got %q", gotPath)
	}
}

func TestFetchKeyByDomainFromURL_UsesAPIAgentsKeysPath(t *testing.T) {
	var gotPath string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.EscapedPath()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(keyResponseJSON()))
	}))
	defer srv.Close()

	_, err := FetchKeyByDomainFromURL(context.Background(), nil, srv.URL, "example.com")
	if err != nil {
		t.Fatalf("FetchKeyByDomainFromURL: %v", err)
	}
	expected := "/api/agents/keys/domain/example.com"
	if gotPath != expected {
		t.Fatalf("expected %q, got %q", expected, gotPath)
	}
}

func TestFetchKeyByHashFromURL_UsesAPIAgentsKeysPath(t *testing.T) {
	var gotPath string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.EscapedPath()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"public_key":"Zm9v",
			"algorithm":"Ed25519",
			"public_key_hash":"sha256:abcdef",
			"agent_id":"agent-abc",
			"version":"v1"
		}`))
	}))
	defer srv.Close()

	_, err := FetchKeyByHashFromURL(context.Background(), nil, srv.URL, "sha256:abcdef")
	if err != nil {
		t.Fatalf("FetchKeyByHashFromURL: %v", err)
	}
	expected := "/api/agents/keys/hash/sha256:abcdef"
	if gotPath != expected {
		t.Fatalf("expected %q, got %q", expected, gotPath)
	}
}

func TestFetchAllKeysFromURL_UsesAPIAgentsKeysPath(t *testing.T) {
	var gotPath string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.EscapedPath()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(keyHistoryResponseJSON()))
	}))
	defer srv.Close()

	_, err := FetchAllKeysFromURL(context.Background(), nil, srv.URL, "agent-abc")
	if err != nil {
		t.Fatalf("FetchAllKeysFromURL: %v", err)
	}
	expected := "/api/agents/keys/agent-abc/all"
	if gotPath != expected {
		t.Fatalf("expected %q, got %q", expected, gotPath)
	}
}
