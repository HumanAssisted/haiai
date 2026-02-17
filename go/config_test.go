package haisdk

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestLoadConfig(t *testing.T) {
	tmpDir := t.TempDir()
	configPath := filepath.Join(tmpDir, "jacs.config.json")

	cfg := Config{
		JacsAgentName:    "test-agent",
		JacsAgentVersion: "1.0.0",
		JacsKeyDir:       "/keys",
		JacsID:           "jacs-id-123",
	}
	data, _ := json.Marshal(cfg)
	if err := os.WriteFile(configPath, data, 0644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	loaded, err := LoadConfig(configPath)
	if err != nil {
		t.Fatalf("LoadConfig: %v", err)
	}

	if loaded.JacsAgentName != "test-agent" {
		t.Errorf("expected JacsAgentName 'test-agent', got '%s'", loaded.JacsAgentName)
	}
	if loaded.JacsID != "jacs-id-123" {
		t.Errorf("expected JacsID 'jacs-id-123', got '%s'", loaded.JacsID)
	}
	if loaded.JacsKeyDir != "/keys" {
		t.Errorf("expected JacsKeyDir '/keys', got '%s'", loaded.JacsKeyDir)
	}
}

func TestLoadConfigNotFound(t *testing.T) {
	_, err := LoadConfig("/nonexistent/jacs.config.json")
	if err == nil {
		t.Fatal("expected error for missing config")
	}

	sdkErr, ok := err.(*Error)
	if !ok {
		t.Fatalf("expected *Error, got %T", err)
	}
	if sdkErr.Kind != ErrConfigNotFound {
		t.Errorf("expected ErrConfigNotFound, got %v", sdkErr.Kind)
	}
}

func TestLoadConfigInvalidJSON(t *testing.T) {
	tmpDir := t.TempDir()
	configPath := filepath.Join(tmpDir, "jacs.config.json")
	if err := os.WriteFile(configPath, []byte("not json{{{"), 0644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	_, err := LoadConfig(configPath)
	if err == nil {
		t.Fatal("expected error for invalid JSON")
	}

	sdkErr, ok := err.(*Error)
	if !ok {
		t.Fatalf("expected *Error, got %T", err)
	}
	if sdkErr.Kind != ErrConfigInvalid {
		t.Errorf("expected ErrConfigInvalid, got %v", sdkErr.Kind)
	}
}

func TestDiscoverConfigFromEnv(t *testing.T) {
	tmpDir := t.TempDir()
	configPath := filepath.Join(tmpDir, "jacs.config.json")

	cfg := Config{
		JacsAgentName: "env-agent",
		JacsID:        "env-id",
	}
	data, _ := json.Marshal(cfg)
	os.WriteFile(configPath, data, 0644)

	t.Setenv("JACS_CONFIG_PATH", configPath)

	loaded, err := DiscoverConfig()
	if err != nil {
		t.Fatalf("DiscoverConfig: %v", err)
	}
	if loaded.JacsAgentName != "env-agent" {
		t.Errorf("expected 'env-agent', got '%s'", loaded.JacsAgentName)
	}
}

func TestDiscoverConfigNotFound(t *testing.T) {
	// Clear env and ensure no config in cwd or home
	t.Setenv("JACS_CONFIG_PATH", "")

	// Change to a temp dir with no config
	tmpDir := t.TempDir()
	origDir, _ := os.Getwd()
	os.Chdir(tmpDir)
	defer os.Chdir(origDir)

	_, err := DiscoverConfig()
	if err == nil {
		t.Fatal("expected error when no config exists")
	}
}

func TestResolveKeyPath(t *testing.T) {
	cfg := &Config{
		JacsAgentName: "my-agent",
		JacsKeyDir:    "/custom/keys",
	}

	path := ResolveKeyPath(cfg, "/some/dir/jacs.config.json")
	expected := "/custom/keys/my-agent.private.pem"
	if path != expected {
		t.Errorf("expected '%s', got '%s'", expected, path)
	}
}

func TestResolveKeyPathDefaultDir(t *testing.T) {
	cfg := &Config{
		JacsAgentName: "my-agent",
		JacsKeyDir:    "", // empty -> use config dir
	}

	path := ResolveKeyPath(cfg, "/home/user/.jacs/jacs.config.json")
	expected := "/home/user/.jacs/my-agent.private.pem"
	if path != expected {
		t.Errorf("expected '%s', got '%s'", expected, path)
	}
}
