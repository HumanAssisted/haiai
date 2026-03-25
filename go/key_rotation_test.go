package haiai

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// setupRotationAgent creates a temp directory with a valid agent config and keys,
// and returns a Client configured to use them with a mock FFI client.
func setupRotationAgent(t *testing.T) (*Client, string, *mockFFIClient) {
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
	_ = pub

	// Write a dummy private key PEM for the config to reference
	privPath := filepath.Join(keyDir, "agent_private_key.pem")
	pubPath := filepath.Join(keyDir, "agent_public_key.pem")
	if err := os.WriteFile(privPath, []byte("dummy"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(pubPath, []byte("dummy"), 0o644); err != nil {
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

	// Create mock FFI that returns a valid RotationResult
	mockFFI := newMockFFIClient("https://hai.example", "test-jacs-id-12345", "JACS test:123:sig")
	mockFFI.rotateKeysFn = func(optionsJSON string) (json.RawMessage, error) {
		var opts map[string]interface{}
		_ = json.Unmarshal([]byte(optionsJSON), &opts)

		registerWithHai := true
		if v, ok := opts["register_with_hai"].(bool); ok {
			registerWithHai = v
		}

		result := RotationResult{
			JacsID:            "test-jacs-id-12345",
			OldVersion:        "v1-original",
			NewVersion:        "v2-rotated-uuid",
			NewPublicKeyHash:  "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
			RegisteredWithHai: registerWithHai,
			SignedAgentJSON:   `{"jacsId":"test-jacs-id-12345","jacsVersion":"v2-rotated-uuid","jacsPreviousVersion":"v1-original"}`,
		}
		data, _ := json.Marshal(result)
		return data, nil
	}

	cl, err := NewClient(
		WithEndpoint("https://hai.example"),
		WithJACSID("test-jacs-id-12345"),
		WithPrivateKey(priv),
		WithFFIClient(mockFFI),
	)
	if err != nil {
		t.Fatal(err)
	}

	return cl, cfgPath, mockFFI
}

func boolPtr(v bool) *bool { return &v }

func TestRotateKeysDelegatesToFFI(t *testing.T) {
	cl, _, mockFFI := setupRotationAgent(t)

	var capturedOpts string
	mockFFI.rotateKeysFn = func(optionsJSON string) (json.RawMessage, error) {
		capturedOpts = optionsJSON
		result := RotationResult{
			JacsID:            "test-jacs-id-12345",
			OldVersion:        "v1-original",
			NewVersion:        "v2-rotated",
			NewPublicKeyHash:  "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
			RegisteredWithHai: false,
			SignedAgentJSON:   `{"jacsId":"test-jacs-id-12345"}`,
		}
		data, _ := json.Marshal(result)
		return data, nil
	}

	_, err := cl.RotateKeys(context.Background(), &RotateKeysOptions{
		RegisterWithHai: boolPtr(false),
	})
	if err != nil {
		t.Fatalf("RotateKeys: %v", err)
	}

	if capturedOpts == "" {
		t.Fatal("FFI RotateKeys should have been called")
	}

	var opts map[string]interface{}
	if err := json.Unmarshal([]byte(capturedOpts), &opts); err != nil {
		t.Fatalf("invalid options JSON: %v", err)
	}
	if opts["register_with_hai"] != false {
		t.Errorf("register_with_hai = %v, want false", opts["register_with_hai"])
	}
}

func TestRotateKeysReturnsValidResult(t *testing.T) {
	cl, _, _ := setupRotationAgent(t)

	result, err := cl.RotateKeys(context.Background(), &RotateKeysOptions{
		RegisterWithHai: boolPtr(false),
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
	if result.NewVersion == "" {
		t.Error("NewVersion should not be empty")
	}
	if result.NewPublicKeyHash == "" {
		t.Error("NewPublicKeyHash should not be empty")
	}
	if result.RegisteredWithHai {
		t.Error("RegisteredWithHai should be false")
	}
	if result.SignedAgentJSON == "" {
		t.Error("SignedAgentJSON should not be empty")
	}
}

func TestRotateKeysWithRegistration(t *testing.T) {
	cl, _, _ := setupRotationAgent(t)

	result, err := cl.RotateKeys(context.Background(), &RotateKeysOptions{
		RegisterWithHai: boolPtr(true),
	})
	if err != nil {
		t.Fatalf("RotateKeys: %v", err)
	}

	if !result.RegisteredWithHai {
		t.Error("RegisteredWithHai should be true when register_with_hai=true")
	}
}

func TestRotateKeysDefaultsToRegisterTrue(t *testing.T) {
	cl, _, mockFFI := setupRotationAgent(t)

	var capturedOpts string
	mockFFI.rotateKeysFn = func(optionsJSON string) (json.RawMessage, error) {
		capturedOpts = optionsJSON
		result := RotationResult{
			JacsID:            "test-jacs-id-12345",
			OldVersion:        "v1-original",
			NewVersion:        "v2",
			NewPublicKeyHash:  "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
			RegisteredWithHai: true,
			SignedAgentJSON:   `{}`,
		}
		data, _ := json.Marshal(result)
		return data, nil
	}

	_, err := cl.RotateKeys(context.Background(), nil)
	if err != nil {
		t.Fatalf("RotateKeys: %v", err)
	}

	var opts map[string]interface{}
	_ = json.Unmarshal([]byte(capturedOpts), &opts)
	if opts["register_with_hai"] != true {
		t.Errorf("default register_with_hai = %v, want true", opts["register_with_hai"])
	}
}

func TestRotateKeysErrorsWithoutJacsID(t *testing.T) {
	_, priv, err := GenerateKeyPair()
	if err != nil {
		t.Fatal(err)
	}

	mockFFI := newMockFFIClient("https://hai.example", "", "")
	cl := &Client{
		endpoint:  "https://hai.example",
		privateKey: priv,
		agentKeys: newKeyCache(),
		ffi:       mockFFI,
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
