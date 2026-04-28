//go:build cgo_smoke

// Real-FFI smoke test for haiigo (libhaiigo cdylib).
//
// Loads the real cgo binding and round-trips `SaveMemory("smoke")` against a
// local `httptest.NewServer`. This is the one test that would have caught the
// regression where the Go FFI surface declared 20 methods that returned
// `notWiredThroughLibhaiigo` instead of dispatching to libhaiigo.
//
// Gated by the `cgo_smoke` build tag so it only runs when explicitly invoked:
//
//	go test -tags cgo_smoke -run NativeSmoke ./go/...
//
// Per PRD docs/haisdk/JACS_DOCUMENT_STORE_FFI_PRD.md §5.5: real listener, no
// HTTP-level mock. The traffic is Rust `reqwest` inside libhaiigo, which
// only a real socket can intercept.

package haiai_test

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"

	"github.com/HumanAssisted/haiai-go/ffi"
)

func TestNativeSmokeSaveMemoryRoundTripsThroughLibhaiigo(t *testing.T) {
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

	workdir, err := os.MkdirTemp("", "haisdk-smoke-go-")
	if err != nil {
		t.Fatalf("mkdtemp: %v", err)
	}
	defer os.RemoveAll(workdir)

	configPath := filepath.Join(workdir, "jacs.config.json")
	// Skip cleanly if the JACS toolchain isn't available — the smoke test is
	// opt-in; agent creation requires the JACS crate's keygen helpers, which
	// are not part of the haiigo build surface.
	if !canBootstrapJacsAgent() {
		t.Skipf("smoke test skipped: cannot bootstrap JACS agent in this environment")
	}
	if err := bootstrapJacsAgent(workdir, configPath); err != nil {
		t.Skipf("smoke test skipped: JACS agent creation failed: %v", err)
	}

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

// canBootstrapJacsAgent reports whether this environment can create a JACS
// agent inline. The JACS Go bindings are not always available alongside the
// haiigo cdylib build; if they aren't, the smoke test skips.
func canBootstrapJacsAgent() bool {
	// Best-effort check: a working JACS toolchain is presumed available when
	// the test runner explicitly enables the build tag and provides
	// `JACS_SMOKE_AGENT_DIR` pointing at a pre-baked agent dir, OR the
	// caller has set up agent creation via the go-jacs binding. Default
	// behavior in CI is to skip.
	if dir := os.Getenv("JACS_SMOKE_AGENT_DIR"); dir != "" {
		if _, err := os.Stat(filepath.Join(dir, "jacs.config.json")); err == nil {
			return true
		}
	}
	return false
}

// bootstrapJacsAgent prepares a JACS agent in workdir and writes
// jacs.config.json at configPath. When `JACS_SMOKE_AGENT_DIR` is set, the
// pre-baked config is copied into place. Otherwise, returns an error so the
// caller can `t.Skip`.
func bootstrapJacsAgent(workdir, configPath string) error {
	src := os.Getenv("JACS_SMOKE_AGENT_DIR")
	if src == "" {
		return os.ErrNotExist
	}
	srcConfig := filepath.Join(src, "jacs.config.json")
	data, err := os.ReadFile(srcConfig)
	if err != nil {
		return err
	}
	return os.WriteFile(configPath, data, 0o600)
}
