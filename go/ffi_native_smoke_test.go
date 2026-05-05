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
//     response. Verifies the mock saw a `POST /api/v1/records` with
//     signed markdown bytes (plaintext plus the JACS signature footer).
//
//  2. `TestNativeSmokeSaveMemoryLocalPath` (LOCAL) — dev default path
//     (`haiai init` writes `default_storage: "fs"`). Bootstraps a fresh
//     agent, sets `JACS_DEFAULT_STORAGE=fs`, signs locally, writes to disk,
//     and returns a client-side `{jacsId}:{jacsVersion}` key. Verifies the
//     doc round-trips via `GetRecordBytes(key)`.
//
// Together these cover the only two backends production and dev users
// actually exercise.
//
// Gated by the `cgo_smoke` build tag so it only runs when explicitly
// invoked:
//
//	go test -tags cgo_smoke -run NativeSmoke ./go/...
//
// Per PRD docs/haiai/JACS_DOCUMENT_STORE_FFI_PRD.md §5.5: real listener,
// no HTTP-level mock. The traffic is Rust `reqwest` inside libhaiigo,
// which only a real socket can intercept.

package haiai_test

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime"
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

		// The FFI's `save_memory(singleton: true)` first issues a GET to
		// /api/v1/records to check for an existing singleton. Reply with
		// an empty `items` list so the caller takes the
		// "no existing → create" branch and proceeds to POST.
		if r.URL.Path == "/api/v1/records" && r.Method == http.MethodGet {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(`{"items":[],"next_cursor":null}`))
			return
		}
		if r.URL.Path == "/api/v1/records" && r.Method == http.MethodPost {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusCreated)
			_, _ = w.Write([]byte(`{"key":"smoke:v1","id":"smoke","version":"v1","jacsType":"memory","jacsVersionDate":"2026-01-01T00:00:00Z"}`))
			return
		}
		w.WriteHeader(http.StatusNotFound)
	}))
	defer server.Close()

	configPath := bootstrapJacsAgentOrSkip(t, "haiai-smoke-go-remote-")

	cfg := map[string]any{
		"base_url":             server.URL,
		"jacs_config_path":     configPath,
		"jacs_storage_backend": "remote",
		"client_type":          "go",
		"timeout_secs":         5,
		"max_retries":          0,
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
	// The FFI does at least: 1 GET (find_document singleton check) + 1
	// POST (sign+store). Assert the POST is what we expect; GET count can
	// vary with future routing tweaks.
	var posts []map[string]any
	for _, r := range captured {
		if r["method"] == "POST" {
			posts = append(posts, r)
		}
	}
	if len(posts) != 1 {
		t.Fatalf("expected exactly 1 POST to /api/v1/records, got %d (captured methods: %v)",
			len(posts), capturedMethods(captured))
	}
	req := posts[0]
	if req["path"] != "/api/v1/records" {
		t.Errorf("expected /api/v1/records, got %v", req["path"])
	}
	if !strings.Contains(req["content"].(string), "text/markdown") {
		t.Errorf("expected text/markdown content type, got %v", req["content"])
	}
	if !strings.Contains(req["body"].(string), "smoke-content") {
		t.Errorf("expected original plaintext in signed markdown body, got %v", req["body"])
	}
	if !strings.Contains(req["body"].(string), "-----BEGIN JACS SIGNATURE-----") {
		t.Errorf("expected JACS signature footer in signed markdown body, got %v", req["body"])
	}
}

func capturedMethods(reqs []map[string]any) []string {
	out := make([]string, 0, len(reqs))
	for _, r := range reqs {
		out = append(out, r["method"].(string)+" "+r["path"].(string))
	}
	return out
}

// TestNativeSmokeSaveMemoryLocalPath exercises the LOCAL (fs) backend: the
// FFI signs and writes to disk without any HTTP traffic. The pre-baked
// smoke agent already defaults to fs, but pinning the env var here makes
// the test hermetic against future bootstrap changes or a leaking
// parent-shell env var.
func TestNativeSmokeSaveMemoryLocalPath(t *testing.T) {
	t.Setenv("JACS_DEFAULT_STORAGE", "fs")

	configPath := bootstrapFreshJacsAgentOrSkip(t, "haiai-smoke-go-local-")

	// No mock HTTP server: the local path must not make any network
	// calls, and binding the FFI to an unreachable URL surfaces that
	// invariant if the routing decision ever regresses.
	cfg := map[string]any{
		"base_url":             "http://127.0.0.1:1", // unreachable on purpose
		"jacs_config_path":     configPath,
		"jacs_storage_backend": "fs",
		"client_type":          "go",
		"timeout_secs":         5,
		"max_retries":          0,
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

func locateHaiaiCLI() (string, bool) {
	if explicit := os.Getenv("HAIAI_CLI"); explicit != "" && isExecutable(explicit) {
		return explicit, true
	}
	if _, file, _, ok := runtime.Caller(0); ok {
		if cli, ok := walkForHaiaiCLI(filepath.Dir(file)); ok {
			return cli, true
		}
	}
	if wd, err := os.Getwd(); err == nil {
		if cli, ok := walkForHaiaiCLI(wd); ok {
			return cli, true
		}
	}
	if cli, err := exec.LookPath("haiai"); err == nil {
		return cli, true
	}
	return "", false
}

func walkForHaiaiCLI(start string) (string, bool) {
	for dir := start; ; dir = filepath.Dir(dir) {
		candidate := filepath.Join(dir, "rust", "target", "release", haiaiBinaryName())
		if isExecutable(candidate) {
			return candidate, true
		}
		if _, err := os.Stat(filepath.Join(dir, ".git")); err == nil {
			return "", false
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			return "", false
		}
	}
}

func haiaiBinaryName() string {
	if runtime.GOOS == "windows" {
		return "haiai.exe"
	}
	return "haiai"
}

func isExecutable(path string) bool {
	info, err := os.Stat(path)
	return err == nil && !info.IsDir() && info.Mode()&0o111 != 0
}

func bootstrapFreshJacsAgentOrSkip(t *testing.T, tempPrefix string) string {
	t.Helper()

	cli, ok := locateHaiaiCLI()
	if !ok {
		t.Skipf("smoke test skipped: haiai CLI binary not found")
	}

	workdir, err := os.MkdirTemp("", tempPrefix)
	if err != nil {
		t.Fatalf("mkdtemp: %v", err)
	}
	if realWorkdir, err := filepath.EvalSymlinks(workdir); err == nil {
		workdir = realWorkdir
	}
	t.Cleanup(func() {
		_ = os.RemoveAll(workdir)
	})

	password := os.Getenv("_HAISDK_SMOKE_PASSWORD")
	if password == "" {
		password = os.Getenv("JACS_PRIVATE_KEY_PASSWORD")
	}
	if password == "" {
		password = "smoke-password"
		t.Setenv("JACS_PRIVATE_KEY_PASSWORD", password)
	}

	configPath := filepath.Join(workdir, "jacs.config.json")
	cmd := exec.Command(
		cli,
		"init",
		"--quiet",
		"--name",
		"local-smoke-agent",
		"--register",
		"false",
		"--data-dir",
		filepath.Join(workdir, "data"),
		"--key-dir",
		filepath.Join(workdir, "keys"),
		"--config-path",
		configPath,
	)
	cmd.Env = upsertEnv(os.Environ(), "JACS_PRIVATE_KEY_PASSWORD", password)
	output, err := cmd.CombinedOutput()
	if err != nil {
		t.Skipf("smoke test skipped: haiai init failed: %v; output=%s", err, string(output))
	}
	if _, err := os.Stat(configPath); err != nil {
		t.Skipf("smoke test skipped: haiai init did not write %s: %v", configPath, err)
	}
	return configPath
}

func upsertEnv(env []string, key string, value string) []string {
	prefix := key + "="
	for i, entry := range env {
		if strings.HasPrefix(entry, prefix) {
			env[i] = prefix + value
			return env
		}
	}
	return append(env, prefix+value)
}

// bootstrapJacsAgentOrSkip prepares the shared CI JACS agent for the remote
// smoke test or calls `t.Skip`. Local-path smoke tests use
// bootstrapFreshJacsAgentOrSkip so singleton lookups cannot observe state
// written by the remote signer.
//
// Returns the absolute path to jacs.config.json.
func bootstrapJacsAgentOrSkip(t *testing.T, tempPrefix string) string {
	t.Helper()
	_ = tempPrefix

	src := os.Getenv("JACS_SMOKE_AGENT_DIR")
	if src == "" {
		t.Skipf("smoke test skipped: JACS_SMOKE_AGENT_DIR not set — cannot bootstrap JACS agent")
	}
	srcConfig := filepath.Join(src, "jacs.config.json")
	if _, err := os.Stat(srcConfig); err != nil {
		t.Skipf("smoke test skipped: %s missing or unreadable: %v", srcConfig, err)
	}
	if realConfig, err := filepath.EvalSymlinks(srcConfig); err == nil {
		return realConfig
	}
	return srcConfig
}
