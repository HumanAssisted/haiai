//go:build cgo_smoke

// Real-FFI smoke tests for haiigo (libhaiigo cdylib).
//
// Two tests, one per backend, both loading the real cgo binding and
// exercising `SaveMemory("...")` end-to-end:
//
//  1. `TestNativeSmokeSaveMemoryRoundTripsThroughLibhaiigo` (REMOTE) —
//     hosted production path. Sets `JACS_DEFAULT_STORAGE=remote` so the FFI
//     builds a `RemoteJacsProvider`, signs locally, POSTs to a real
//     `httptest.NewServer`, and reads the server-issued key from the
//     response. Verifies the mock saw exactly one `POST /api/v1/records`
//     with `application/json` and a `"jacsType":"memory"` body.
//
//  2. `TestNativeSmokeSaveMemoryLocalPath` (LOCAL) — dev default path
//     (`haiai init` writes `default_storage: "fs"`). Sets
//     `JACS_DEFAULT_STORAGE=fs` so the FFI builds a `LocalJacsProvider`,
//     signs locally, writes to disk, and returns a client-side
//     `{jacsId}:{jacsVersion}` key. Verifies the doc round-trips via
//     `GetRecordBytes(key)`.
//
// Together these cover the only two backends production and dev users
// actually exercise.
//
// Gated by the `cgo_smoke` build tag so it only runs when explicitly
// invoked:
//
//	go test -tags cgo_smoke -run NativeSmoke ./go/...
//
// Per PRD docs/haisdk/JACS_DOCUMENT_STORE_FFI_PRD.md §5.5: real listener,
// no HTTP-level mock. The traffic is Rust `reqwest` inside libhaiigo,
// which only a real socket can intercept.

package haiai_test

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"testing"

	"github.com/HumanAssisted/haiai-go/ffi"
)

// `LocalJacsProvider::store_signed_text` returns the key as
// `{jacsId}:{jacsVersion}` where both halves are JACS UUIDs. This regex
// matches that exact shape so the local-path test asserts on the key
// *structure* (not a specific value, which would change every run).
var localKeyPattern = regexp.MustCompile(
	`^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}` +
		`:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$`)

// TestNativeSmokeSaveMemoryRoundTripsThroughLibhaiigo exercises the REMOTE
// backend: `JACS_DEFAULT_STORAGE=remote` routes the FFI through
// `RemoteJacsProvider`, which signs locally and POSTs to the mock server.
func TestNativeSmokeSaveMemoryRoundTripsThroughLibhaiigo(t *testing.T) {
	// `t.Setenv` snapshots and restores the env var around the test, so
	// per-test routing overrides don't leak into sibling tests. Without
	// this, the FFI's `build_document_provider` falls through to
	// `default_storage: "fs"` (set by `haiai init`), routes to
	// LocalJacsProvider, and never makes the HTTP call this test was
	// written to verify.
	t.Setenv("JACS_DEFAULT_STORAGE", "remote")

	var (
		mu       sync.Mutex
		captured []map[string]any
	)

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		// Read the entire body so assertions are not flaky on partial reads.
		// `io.ReadAll` accumulates until EOF, which is required because
		// `r.Body.Read` is permitted to return fewer bytes than requested
		// per the `io.Reader` contract.
		body, err := io.ReadAll(r.Body)
		if err != nil {
			t.Logf("body read error: %v", err)
		}
		mu.Lock()
		captured = append(captured, map[string]any{
			"method":  r.Method,
			"path":    r.URL.Path,
			"content": r.Header.Get("Content-Type"),
			"body":    string(body),
		})
		mu.Unlock()

		if r.URL.Path == "/api/v1/records" && r.Method == http.MethodPost {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusCreated)
			_, _ = w.Write([]byte(`{"key":"smoke:v1","id":"smoke","version":"v1","jacsType":"memory","jacsVersionDate":"2026-01-01T00:00:00Z"}`))
			return
		}
		w.WriteHeader(http.StatusNotFound)
	}))
	defer server.Close()

	configPath := bootstrapJacsAgentOrSkip(t, "haisdk-smoke-go-remote-")

	cfg := map[string]any{
		"base_url":         server.URL,
		"jacs_config_path": configPath,
		"client_type":      "go",
		"timeout_secs":     5,
		"max_retries":      0,
	}
	cfgJSON, err := json.Marshal(cfg)
	if err != nil {
		t.Fatalf("marshal config: %v", err)
	}

	client, err := ffi.NewClient(string(cfgJSON))
	if err != nil {
		t.Fatalf("ffi.NewClient: %v", err)
	}
	defer client.Close()

	key, err := client.SaveMemory("smoke-content")
	if err != nil {
		t.Fatalf("SaveMemory: %v", err)
	}
	if key != "smoke:v1" {
		t.Errorf("expected key=smoke:v1, got %q", key)
	}

	mu.Lock()
	defer mu.Unlock()
	if len(captured) != 1 {
		t.Fatalf("expected 1 POST, got %d", len(captured))
	}
	req := captured[0]
	if req["method"] != "POST" {
		t.Errorf("expected POST, got %v", req["method"])
	}
	if req["path"] != "/api/v1/records" {
		t.Errorf("expected /api/v1/records, got %v", req["path"])
	}
	if !strings.Contains(req["content"].(string), "application/json") {
		t.Errorf("expected application/json content type, got %v", req["content"])
	}
	if !strings.Contains(req["body"].(string), `"jacsType":"memory"`) {
		t.Errorf("expected jacsType:memory in body, got %v", req["body"])
	}
}

// TestNativeSmokeSaveMemoryLocalPath exercises the LOCAL (fs) backend: the
// FFI signs and writes to disk without any HTTP traffic. The pre-baked
// smoke agent already defaults to fs, but pinning the env var here makes
// the test hermetic against future bootstrap changes or a leaking
// parent-shell env var.
func TestNativeSmokeSaveMemoryLocalPath(t *testing.T) {
	t.Setenv("JACS_DEFAULT_STORAGE", "fs")

	configPath := bootstrapJacsAgentOrSkip(t, "haisdk-smoke-go-local-")

	// No mock HTTP server: the local path must not make any network
	// calls, and binding the FFI to an unreachable URL surfaces that
	// invariant if the routing decision ever regresses.
	cfg := map[string]any{
		"base_url":         "http://127.0.0.1:1", // unreachable on purpose
		"jacs_config_path": configPath,
		"client_type":      "go",
		"timeout_secs":     5,
		"max_retries":      0,
	}
	cfgJSON, err := json.Marshal(cfg)
	if err != nil {
		t.Fatalf("marshal config: %v", err)
	}

	client, err := ffi.NewClient(string(cfgJSON))
	if err != nil {
		t.Fatalf("ffi.NewClient: %v", err)
	}
	defer client.Close()

	key, err := client.SaveMemory("local-smoke-content")
	if err != nil {
		t.Fatalf("SaveMemory: %v", err)
	}
	if !localKeyPattern.MatchString(key) {
		t.Errorf("expected local key matching `{jacsId}:{jacsVersion}` UUID shape, got %q", key)
	}

	// Round-trip: fetch the just-stored document by key. The FFI returns
	// the raw bytes of the signed text artifact, which must contain the
	// original plaintext we saved.
	recordBytes, err := client.GetRecordBytes(key)
	if err != nil {
		t.Fatalf("GetRecordBytes: %v", err)
	}
	if !strings.Contains(string(recordBytes), "local-smoke-content") {
		t.Errorf("expected stored signed-text artifact to contain plaintext, got %q",
			string(recordBytes))
	}
}

// bootstrapJacsAgentOrSkip prepares a JACS agent for the smoke test or
// calls `t.Skip`. When `JACS_SMOKE_AGENT_DIR` is set (CI lane), the
// pre-baked config is copied into a fresh tempdir so each test gets its
// own data dir. Without that env var, the toolchain to mint an agent
// inline isn't available from haiigo, and we skip cleanly.
//
// Returns the absolute path to the copied jacs.config.json.
func bootstrapJacsAgentOrSkip(t *testing.T, tempPrefix string) string {
	t.Helper()

	src := os.Getenv("JACS_SMOKE_AGENT_DIR")
	if src == "" {
		t.Skipf("smoke test skipped: JACS_SMOKE_AGENT_DIR not set — cannot bootstrap JACS agent")
	}
	srcConfig := filepath.Join(src, "jacs.config.json")
	if _, err := os.Stat(srcConfig); err != nil {
		t.Skipf("smoke test skipped: %s missing or unreadable: %v", srcConfig, err)
	}

	workdir, err := os.MkdirTemp("", tempPrefix)
	if err != nil {
		t.Fatalf("mkdtemp: %v", err)
	}
	t.Cleanup(func() {
		_ = os.RemoveAll(workdir)
	})

	configPath := filepath.Join(workdir, "jacs.config.json")
	data, err := os.ReadFile(srcConfig)
	if err != nil {
		t.Fatalf("read pre-baked config %s: %v", srcConfig, err)
	}
	if err := os.WriteFile(configPath, data, 0o600); err != nil {
		t.Fatalf("write %s: %v", configPath, err)
	}
	return configPath
}
