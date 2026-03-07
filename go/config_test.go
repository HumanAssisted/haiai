package haisdk

import (
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"strings"
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
	if err := os.WriteFile(configPath, data, 0o644); err != nil {
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
	if err := os.WriteFile(configPath, []byte("not json{{{"), 0o644); err != nil {
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
	_ = os.WriteFile(configPath, data, 0o644)

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
	_ = os.Chdir(tmpDir)
	defer func() { _ = os.Chdir(origDir) }()

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
	expected := "/custom/keys/agent_private_key.pem"
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
	expected := "/home/user/.jacs/agent_private_key.pem"
	if path != expected {
		t.Errorf("expected '%s', got '%s'", expected, path)
	}
}

func TestResolveKeyPathRelativeDirUsesConfigLocation(t *testing.T) {
	cfg := &Config{
		JacsAgentName: "my-agent",
		JacsKeyDir:    "keys",
	}

	path := ResolveKeyPath(cfg, "/home/user/.jacs/jacs.config.json")
	expected := "/home/user/.jacs/keys/agent_private_key.pem"
	if path != expected {
		t.Errorf("expected '%s', got '%s'", expected, path)
	}
}

func TestResolvePrivateKeyPasswordFromEnv(t *testing.T) {
	t.Setenv("JACS_PRIVATE_KEY_PASSWORD", "dev-password")
	t.Setenv("JACS_PASSWORD_FILE", "")

	pwd, err := ResolvePrivateKeyPassword()
	if err != nil {
		t.Fatalf("ResolvePrivateKeyPassword: %v", err)
	}
	if string(pwd) != "dev-password" {
		t.Fatalf("unexpected password: %q", string(pwd))
	}
}

func TestResolvePrivateKeyPasswordFromFileWhenEnvDisabled(t *testing.T) {
	tmpDir := t.TempDir()
	passwordFile := filepath.Join(tmpDir, "password.txt")
	if err := os.WriteFile(passwordFile, []byte("file-password\n"), 0o600); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	t.Setenv("JACS_PRIVATE_KEY_PASSWORD", "dev-password")
	t.Setenv("JACS_PASSWORD_FILE", passwordFile)
	t.Setenv("JACS_DISABLE_PASSWORD_ENV", "1")

	pwd, err := ResolvePrivateKeyPassword()
	if err != nil {
		t.Fatalf("ResolvePrivateKeyPassword: %v", err)
	}
	if string(pwd) != "file-password" {
		t.Fatalf("unexpected password: %q", string(pwd))
	}
}

func TestResolvePrivateKeyPasswordFailsOnMultipleSources(t *testing.T) {
	tmpDir := t.TempDir()
	passwordFile := filepath.Join(tmpDir, "password.txt")
	if err := os.WriteFile(passwordFile, []byte("file-password\n"), 0o600); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	t.Setenv("JACS_PRIVATE_KEY_PASSWORD", "dev-password")
	t.Setenv("JACS_PASSWORD_FILE", passwordFile)

	_, err := ResolvePrivateKeyPassword()
	if err == nil {
		t.Fatal("expected conflict error for multiple password sources")
	}
}

func TestResolvePrivateKeyPasswordFailsWhenMissing(t *testing.T) {
	t.Setenv("JACS_PRIVATE_KEY_PASSWORD", "")
	t.Setenv("JACS_PASSWORD_FILE", "")

	_, err := ResolvePrivateKeyPassword()
	if err == nil {
		t.Fatal("expected missing password source error")
	}
}

func TestResolvePrivateKeyPasswordFailsOnInsecureFilePermissions(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("permission-mode checks are unix-specific")
	}

	tmpDir := t.TempDir()
	passwordFile := filepath.Join(tmpDir, "password.txt")
	if err := os.WriteFile(passwordFile, []byte("file-password\n"), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}
	if err := os.Chmod(passwordFile, 0o644); err != nil {
		t.Fatalf("Chmod: %v", err)
	}

	t.Setenv("JACS_PRIVATE_KEY_PASSWORD", "")
	t.Setenv("JACS_PASSWORD_FILE", passwordFile)
	t.Setenv("JACS_DISABLE_PASSWORD_ENV", "1")

	_, err := ResolvePrivateKeyPassword()
	if err == nil {
		t.Fatal("expected insecure permissions error")
	}
	if !strings.Contains(err.Error(), "insecure permissions") {
		t.Fatalf("expected insecure permissions error, got: %v", err)
	}
}

func TestResolvePrivateKeyPasswordFailsOnSymlinkFile(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("symlink checks are unix-specific in CI/dev")
	}

	tmpDir := t.TempDir()
	targetFile := filepath.Join(tmpDir, "password-target.txt")
	linkFile := filepath.Join(tmpDir, "password-link.txt")
	if err := os.WriteFile(targetFile, []byte("file-password\n"), 0o600); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}
	if err := os.Chmod(targetFile, 0o600); err != nil {
		t.Fatalf("Chmod: %v", err)
	}
	if err := os.Symlink(targetFile, linkFile); err != nil {
		t.Fatalf("Symlink: %v", err)
	}

	t.Setenv("JACS_PRIVATE_KEY_PASSWORD", "")
	t.Setenv("JACS_PASSWORD_FILE", linkFile)
	t.Setenv("JACS_DISABLE_PASSWORD_ENV", "1")

	_, err := ResolvePrivateKeyPassword()
	if err == nil {
		t.Fatal("expected symlink rejection error")
	}
	if !strings.Contains(err.Error(), "must not be a symlink") {
		t.Fatalf("expected symlink rejection error, got: %v", err)
	}
}
