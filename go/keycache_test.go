package haisdk

import (
	"context"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
	"time"
)

func TestClientFetchRemoteKeyCachesResult(t *testing.T) {
	var callCount int32

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		atomic.AddInt32(&callCount, 1)
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"jacs_id":"agent-abc","version":"v1",
			"public_key_raw_b64":"Zm9v","algorithm":"Ed25519",
			"public_key_hash":"sha256:abc"
		}`))
	}))
	defer srv.Close()

	t.Setenv("HAI_KEYS_BASE_URL", srv.URL)

	cl, _ := newTestClient(t, srv.URL)
	r1, err := cl.FetchRemoteKey(context.Background(), "agent-abc", "latest")
	if err != nil {
		t.Fatalf("FetchRemoteKey: %v", err)
	}
	r2, err := cl.FetchRemoteKey(context.Background(), "agent-abc", "latest")
	if err != nil {
		t.Fatalf("FetchRemoteKey: %v", err)
	}

	if atomic.LoadInt32(&callCount) != 1 {
		t.Fatalf("expected 1 HTTP call, got %d", callCount)
	}
	if r1.Version != "v1" || r2.Version != "v1" {
		t.Fatalf("unexpected versions: %s, %s", r1.Version, r2.Version)
	}
}

func TestClientClearAgentKeyCacheForcesRefetch(t *testing.T) {
	var callCount int32

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		n := atomic.AddInt32(&callCount, 1)
		version := "v1"
		if n > 1 {
			version = "v2"
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"jacs_id":"agent-abc","version":"` + version + `",
			"public_key_raw_b64":"Zm9v","algorithm":"Ed25519",
			"public_key_hash":"sha256:abc"
		}`))
	}))
	defer srv.Close()

	t.Setenv("HAI_KEYS_BASE_URL", srv.URL)

	cl, _ := newTestClient(t, srv.URL)
	r1, _ := cl.FetchRemoteKey(context.Background(), "agent-abc", "latest")
	if r1.Version != "v1" {
		t.Fatalf("expected v1, got %s", r1.Version)
	}

	cl.ClearAgentKeyCache()

	r2, _ := cl.FetchRemoteKey(context.Background(), "agent-abc", "latest")
	if r2.Version != "v2" {
		t.Fatalf("expected v2 after cache clear, got %s", r2.Version)
	}
	if atomic.LoadInt32(&callCount) != 2 {
		t.Fatalf("expected 2 HTTP calls, got %d", callCount)
	}
}

func TestClientCacheDifferentKeysIndependently(t *testing.T) {
	var callCount int32

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		atomic.AddInt32(&callCount, 1)
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"jacs_id":"agent","version":"v1",
			"public_key_raw_b64":"Zm9v","algorithm":"Ed25519",
			"public_key_hash":"sha256:abc"
		}`))
	}))
	defer srv.Close()

	t.Setenv("HAI_KEYS_BASE_URL", srv.URL)

	cl, _ := newTestClient(t, srv.URL)
	_, _ = cl.FetchRemoteKey(context.Background(), "agent-1", "latest")
	_, _ = cl.FetchRemoteKey(context.Background(), "agent-2", "latest")

	if atomic.LoadInt32(&callCount) != 2 {
		t.Fatalf("expected 2 HTTP calls for different agents, got %d", callCount)
	}

	// Repeat -- should use cache
	_, _ = cl.FetchRemoteKey(context.Background(), "agent-1", "latest")
	_, _ = cl.FetchRemoteKey(context.Background(), "agent-2", "latest")
	if atomic.LoadInt32(&callCount) != 2 {
		t.Fatalf("expected cache reuse (still 2 calls), got %d", callCount)
	}
}

func TestKeyCacheExpiry(t *testing.T) {
	cache := newKeyCache()
	info := &PublicKeyInfo{AgentID: "a", Version: "v1"}

	cache.set("test-key", info)
	if cache.get("test-key") == nil {
		t.Fatal("expected cache hit immediately after set")
	}

	// Manually backdate the entry to simulate expiry
	cache.mu.Lock()
	entry := cache.entries["test-key"]
	entry.cachedAt = time.Now().Add(-6 * time.Minute)
	cache.entries["test-key"] = entry
	cache.mu.Unlock()

	if cache.get("test-key") != nil {
		t.Fatal("expected cache miss after expiry")
	}
}

func TestClientFetchKeyByEmailCaches(t *testing.T) {
	var callCount int32

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		atomic.AddInt32(&callCount, 1)
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(keyResponseJSON()))
	}))
	defer srv.Close()

	t.Setenv("HAI_KEYS_BASE_URL", srv.URL)

	cl, _ := newTestClient(t, srv.URL)
	_, _ = cl.FetchKeyByEmail(context.Background(), "alice@hai.ai")
	_, _ = cl.FetchKeyByEmail(context.Background(), "alice@hai.ai")

	if atomic.LoadInt32(&callCount) != 1 {
		t.Fatalf("expected 1 HTTP call (cached), got %d", callCount)
	}
}

func TestClientFetchKeyByDomainCaches(t *testing.T) {
	var callCount int32

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		atomic.AddInt32(&callCount, 1)
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(keyResponseJSON()))
	}))
	defer srv.Close()

	t.Setenv("HAI_KEYS_BASE_URL", srv.URL)

	cl, _ := newTestClient(t, srv.URL)
	_, _ = cl.FetchKeyByDomain(context.Background(), "example.com")
	_, _ = cl.FetchKeyByDomain(context.Background(), "example.com")

	if atomic.LoadInt32(&callCount) != 1 {
		t.Fatalf("expected 1 HTTP call (cached), got %d", callCount)
	}
}
