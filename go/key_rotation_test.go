package haisdk

import (
	"context"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
)

// setupRotationAgent creates a temp directory with a valid agent config and keys,
// and returns a Client configured to use them.
func setupRotationAgent(t *testing.T) (*Client, string) {
	t.Helper()

	tmpDir := t.TempDir()
	keyDir := filepath.Join(tmpDir, "keys")
	if err := os.MkdirAll(keyDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Generate keypair and write to disk
	pub, priv, err := GenerateKeyPair()
	if err != nil {
		t.Fatal(err)
	}

	privDER, err := x509.MarshalPKCS8PrivateKey(priv)
	if err != nil {
		t.Fatal(err)
	}
	privPEM := pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: privDER})

	pubDER, err := x509.MarshalPKIXPublicKey(pub)
	if err != nil {
		t.Fatal(err)
	}
	pubPEM := pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: pubDER})

	privPath := filepath.Join(keyDir, "agent_private_key.pem")
	pubPath := filepath.Join(keyDir, "agent_public_key.pem")
	if err := os.WriteFile(privPath, privPEM, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(pubPath, pubPEM, 0o644); err != nil {
		t.Fatal(err)
	}

	// Write config
	cfg := map[string]string{
		"jacsAgentName":    "test-rotation-agent",
		"jacsAgentVersion": "v1-original",
		"jacsKeyDir":       keyDir,
		"jacsId":           "test-jacs-id-12345",
	}
	cfgData, _ := json.MarshalIndent(cfg, "", "  ")
	cfgPath := filepath.Join(tmpDir, "jacs.config.json")
	if err := os.WriteFile(cfgPath, cfgData, 0o644); err != nil {
		t.Fatal(err)
	}

	cl, err := NewClient(
		WithEndpoint("https://hai.example"),
		WithJACSID("test-jacs-id-12345"),
		WithPrivateKey(priv),
	)
	if err != nil {
		t.Fatal(err)
	}

	return cl, cfgPath
}

func boolPtr(v bool) *bool { return &v }

func TestRotateKeysGeneratesNewKeysAndArchivesOld(t *testing.T) {
	cl, cfgPath := setupRotationAgent(t)

	result, err := cl.RotateKeys(context.Background(), &RotateKeysOptions{
		RegisterWithHai: boolPtr(false),
		ConfigPath:      cfgPath,
	})
	if err != nil {
		t.Fatalf("RotateKeys: %v", err)
	}

	cfg, err := LoadConfig(cfgPath)
	if err != nil {
		t.Fatal(err)
	}
	keyDir := cfg.JacsKeyDir

	// New key files should exist
	privPath := filepath.Join(keyDir, "agent_private_key.pem")
	pubPath := filepath.Join(keyDir, "agent_public_key.pem")
	if _, err := os.Stat(privPath); err != nil {
		t.Error("new private key should exist")
	}
	if _, err := os.Stat(pubPath); err != nil {
		t.Error("new public key should exist")
	}

	// Old private key should be archived
	archivePriv := filepath.Join(keyDir, "agent_private_key.v1-original.pem")
	if _, err := os.Stat(archivePriv); err != nil {
		t.Error("archived private key should exist")
	}

	_ = result
}

func TestRotateKeysReturnsValidResult(t *testing.T) {
	cl, cfgPath := setupRotationAgent(t)

	result, err := cl.RotateKeys(context.Background(), &RotateKeysOptions{
		RegisterWithHai: boolPtr(false),
		ConfigPath:      cfgPath,
	})
	if err != nil {
		t.Fatalf("RotateKeys: %v", err)
	}

	if result.JacsID != "test-jacs-id-12345" {
		t.Errorf("JacsID = %q, want %q", result.JacsID, "test-jacs-id-12345")
	}
	if result.OldVersion != "v1-original" {
		t.Errorf("OldVersion = %q, want %q", result.OldVersion, "v1-original")
	}
	if result.NewVersion == "v1-original" || result.NewVersion == "" {
		t.Error("NewVersion should be set and different from old")
	}
	if len(result.NewPublicKeyHash) != 64 {
		t.Errorf("NewPublicKeyHash length = %d, want 64 (SHA-256 hex)", len(result.NewPublicKeyHash))
	}
	if result.RegisteredWithHai {
		t.Error("RegisteredWithHai should be false")
	}
	if result.SignedAgentJSON == "" {
		t.Error("SignedAgentJSON should not be empty")
	}

	// Parse signed document
	var doc map[string]interface{}
	if err := json.Unmarshal([]byte(result.SignedAgentJSON), &doc); err != nil {
		t.Fatalf("SignedAgentJSON is not valid JSON: %v", err)
	}
	if doc["jacsId"] != "test-jacs-id-12345" {
		t.Errorf("doc jacsId = %v, want test-jacs-id-12345", doc["jacsId"])
	}
	if doc["jacsVersion"] != result.NewVersion {
		t.Errorf("doc jacsVersion = %v, want %s", doc["jacsVersion"], result.NewVersion)
	}
	if doc["jacsPreviousVersion"] != "v1-original" {
		t.Errorf("doc jacsPreviousVersion = %v, want v1-original", doc["jacsPreviousVersion"])
	}
}

func TestRotateKeysUpdatesConfigFile(t *testing.T) {
	cl, cfgPath := setupRotationAgent(t)

	result, err := cl.RotateKeys(context.Background(), &RotateKeysOptions{
		RegisterWithHai: boolPtr(false),
		ConfigPath:      cfgPath,
	})
	if err != nil {
		t.Fatalf("RotateKeys: %v", err)
	}

	updatedCfg, err := LoadConfig(cfgPath)
	if err != nil {
		t.Fatal(err)
	}
	if updatedCfg.JacsAgentVersion != result.NewVersion {
		t.Errorf("config version = %q, want %q", updatedCfg.JacsAgentVersion, result.NewVersion)
	}
	if updatedCfg.JacsID != "test-jacs-id-12345" {
		t.Errorf("config jacsId changed; got %q", updatedCfg.JacsID)
	}
}

func TestRotateKeysRegistersWithHai(t *testing.T) {
	cl, cfgPath := setupRotationAgent(t)

	var registerCalled bool
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.Contains(r.URL.Path, "/api/v1/agents/register") {
			registerCalled = true
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusCreated)
			_, _ = w.Write([]byte(`{"agent_id":"hai-uuid","jacs_id":"test-jacs-id-12345"}`))
			return
		}
		w.WriteHeader(http.StatusNotFound)
	}))
	defer srv.Close()

	cl.endpoint = srv.URL

	result, err := cl.RotateKeys(context.Background(), &RotateKeysOptions{
		RegisterWithHai: boolPtr(true),
		ConfigPath:      cfgPath,
	})
	if err != nil {
		t.Fatalf("RotateKeys: %v", err)
	}

	if !registerCalled {
		t.Error("Register should have been called")
	}
	if !result.RegisteredWithHai {
		t.Error("RegisteredWithHai should be true")
	}
}

func TestRotateKeysHaiFailurePreservesLocal(t *testing.T) {
	cl, cfgPath := setupRotationAgent(t)

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
		_, _ = w.Write([]byte(`{"error":"server down","status":500,"message":"server down"}`))
	}))
	defer srv.Close()

	cl.endpoint = srv.URL

	result, err := cl.RotateKeys(context.Background(), &RotateKeysOptions{
		RegisterWithHai: boolPtr(true),
		ConfigPath:      cfgPath,
	})
	if err != nil {
		t.Fatalf("RotateKeys should succeed locally even if HAI fails: %v", err)
	}

	if result.NewVersion == "v1-original" {
		t.Error("NewVersion should differ from old")
	}
	if result.RegisteredWithHai {
		t.Error("RegisteredWithHai should be false on HAI failure")
	}
}

func TestRotateKeysErrorsWithoutJacsID(t *testing.T) {
	pub, priv, err := GenerateKeyPair()
	if err != nil {
		t.Fatal(err)
	}
	_ = pub

	cl := &Client{
		endpoint:   "https://hai.example",
		privateKey: priv,
		httpClient: http.DefaultClient,
		agentKeys:  newKeyCache(),
	}

	_, err = cl.RotateKeys(context.Background(), &RotateKeysOptions{
		RegisterWithHai: boolPtr(false),
	})
	if err == nil {
		t.Fatal("expected error for missing jacsId")
	}
	if !strings.Contains(err.Error(), "jacsId") {
		t.Errorf("error should mention jacsId, got: %v", err)
	}
}

func TestRotateKeysNewKeySignsCorrectly(t *testing.T) {
	cl, cfgPath := setupRotationAgent(t)

	result, err := cl.RotateKeys(context.Background(), &RotateKeysOptions{
		RegisterWithHai: boolPtr(false),
		ConfigPath:      cfgPath,
	})
	if err != nil {
		t.Fatalf("RotateKeys: %v", err)
	}

	// Read the new public key from disk
	cfg, _ := LoadConfig(cfgPath)
	pubKeyPath := ResolvePublicKeyPath(cfg, cfgPath)
	pubPEM, err := os.ReadFile(pubKeyPath)
	if err != nil {
		t.Fatalf("failed to read new public key: %v", err)
	}
	newPub, err := ParsePublicKey(pubPEM)
	if err != nil {
		t.Fatalf("failed to parse new public key: %v", err)
	}

	// Extract signature from the signed agent JSON
	var doc map[string]interface{}
	if err := json.Unmarshal([]byte(result.SignedAgentJSON), &doc); err != nil {
		t.Fatal(err)
	}
	sigBlock := doc["jacsSignature"].(map[string]interface{})
	sigB64 := sigBlock["signature"].(string)
	sig, err := base64.StdEncoding.DecodeString(sigB64)
	if err != nil {
		t.Fatal(err)
	}

	// Remove .signature from the doc, re-marshal to get canonical form
	delete(sigBlock, "signature")
	doc["jacsSignature"] = sigBlock
	canonical, _ := json.Marshal(doc)

	if !Verify(newPub, canonical, sig) {
		t.Error("signature should be valid against new public key")
	}
}

func TestRotateKeysPublicKeyHashMatchesDisk(t *testing.T) {
	cl, cfgPath := setupRotationAgent(t)

	result, err := cl.RotateKeys(context.Background(), &RotateKeysOptions{
		RegisterWithHai: boolPtr(false),
		ConfigPath:      cfgPath,
	})
	if err != nil {
		t.Fatalf("RotateKeys: %v", err)
	}

	// Read the new public key and compute hash independently
	cfg, _ := LoadConfig(cfgPath)
	pubKeyPath := ResolvePublicKeyPath(cfg, cfgPath)
	pubPEMData, _ := os.ReadFile(pubKeyPath)
	block, _ := pem.Decode(pubPEMData)
	hash := fmt.Sprintf("%x", sha256.Sum256(block.Bytes))

	if hash != result.NewPublicKeyHash {
		t.Errorf("hash mismatch: computed %s, result %s", hash, result.NewPublicKeyHash)
	}
}

func TestRotateKeysNewVersionIsUUID(t *testing.T) {
	cl, cfgPath := setupRotationAgent(t)

	result, err := cl.RotateKeys(context.Background(), &RotateKeysOptions{
		RegisterWithHai: boolPtr(false),
		ConfigPath:      cfgPath,
	})
	if err != nil {
		t.Fatalf("RotateKeys: %v", err)
	}

	uuidRegex := regexp.MustCompile(`^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$`)
	if !uuidRegex.MatchString(result.NewVersion) {
		t.Errorf("NewVersion %q is not a valid UUID v4", result.NewVersion)
	}
}

func TestRotateKeysTwiceArchivesBothVersions(t *testing.T) {
	cl, cfgPath := setupRotationAgent(t)

	// First rotation: V1 -> V2
	result1, err := cl.RotateKeys(context.Background(), &RotateKeysOptions{
		RegisterWithHai: boolPtr(false),
		ConfigPath:      cfgPath,
	})
	if err != nil {
		t.Fatalf("First RotateKeys: %v", err)
	}
	v2 := result1.NewVersion

	// Second rotation: V2 -> V3
	result2, err := cl.RotateKeys(context.Background(), &RotateKeysOptions{
		RegisterWithHai: boolPtr(false),
		ConfigPath:      cfgPath,
	})
	if err != nil {
		t.Fatalf("Second RotateKeys: %v", err)
	}

	cfg, _ := LoadConfig(cfgPath)
	keyDir := cfg.JacsKeyDir

	// Current key files should exist
	privPath := filepath.Join(keyDir, "agent_private_key.pem")
	if _, err := os.Stat(privPath); err != nil {
		t.Error("current private key should exist")
	}

	// V1 archive
	archiveV1 := filepath.Join(keyDir, "agent_private_key.v1-original.pem")
	if _, err := os.Stat(archiveV1); err != nil {
		t.Error("V1 archived private key should exist")
	}

	// V2 archive
	archiveV2 := filepath.Join(keyDir, fmt.Sprintf("agent_private_key.%s.pem", v2))
	if _, err := os.Stat(archiveV2); err != nil {
		t.Error("V2 archived private key should exist")
	}

	// Version chain
	if result1.OldVersion != "v1-original" {
		t.Errorf("first old version = %q, want v1-original", result1.OldVersion)
	}
	if result1.NewVersion != result2.OldVersion {
		t.Error("first new version should equal second old version")
	}
	if result2.NewVersion == result2.OldVersion {
		t.Error("second new version should differ from old")
	}
}

func TestRotateKeysFixtureContract(t *testing.T) {
	fixturePath := filepath.Join("..", "fixtures", "rotation_result.json")
	data, err := os.ReadFile(fixturePath)
	if err != nil {
		t.Skip("shared fixture not found")
	}

	var fixture map[string]interface{}
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatalf("failed to parse fixture: %v", err)
	}

	expectedFields := []string{
		"jacs_id", "old_version", "new_version",
		"new_public_key_hash", "registered_with_hai", "signed_agent_json",
	}
	for _, field := range expectedFields {
		if _, ok := fixture[field]; !ok {
			t.Errorf("fixture missing field %q", field)
		}
	}
	if len(fixture) != len(expectedFields) {
		t.Errorf("fixture has %d fields, expected %d", len(fixture), len(expectedFields))
	}
}

func TestRotateKeysSendsCorrectRegisterPayload(t *testing.T) {
	cl, cfgPath := setupRotationAgent(t)

	var capturedBody []byte
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.Contains(r.URL.Path, "/api/v1/agents/register") {
			var err error
			capturedBody, err = os.ReadFile("/dev/stdin")
			if err != nil {
				// Fall back to reading from request body
			}
			buf := make([]byte, 1<<20)
			n, _ := r.Body.Read(buf)
			capturedBody = buf[:n]

			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusCreated)
			_, _ = w.Write([]byte(`{"agent_id":"hai-uuid","jacs_id":"test-jacs-id-12345"}`))
			return
		}
		w.WriteHeader(http.StatusNotFound)
	}))
	defer srv.Close()

	cl.endpoint = srv.URL

	result, err := cl.RotateKeys(context.Background(), &RotateKeysOptions{
		RegisterWithHai: boolPtr(true),
		ConfigPath:      cfgPath,
	})
	if err != nil {
		t.Fatalf("RotateKeys: %v", err)
	}

	if len(capturedBody) == 0 {
		t.Fatal("register request body should not be empty")
	}

	var payload map[string]interface{}
	if err := json.Unmarshal(capturedBody, &payload); err != nil {
		t.Fatalf("failed to parse register payload: %v", err)
	}

	agentJSONStr, ok := payload["agent_json"].(string)
	if !ok {
		t.Fatal("payload should contain agent_json string")
	}

	var agentDoc map[string]interface{}
	if err := json.Unmarshal([]byte(agentJSONStr), &agentDoc); err != nil {
		t.Fatalf("failed to parse agent_json: %v", err)
	}

	if agentDoc["jacsVersion"] != result.NewVersion {
		t.Errorf("agent_json jacsVersion = %v, want %s", agentDoc["jacsVersion"], result.NewVersion)
	}
	if agentDoc["jacsId"] != "test-jacs-id-12345" {
		t.Errorf("agent_json jacsId = %v, want test-jacs-id-12345", agentDoc["jacsId"])
	}
}
