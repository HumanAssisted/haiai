package haisdk

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
)

// Config holds JACS agent configuration loaded from jacs.config.json.
type Config struct {
	JacsAgentName    string `json:"jacsAgentName"`
	JacsAgentVersion string `json:"jacsAgentVersion"`
	JacsKeyDir       string `json:"jacsKeyDir"`
	JacsID           string `json:"jacsId"`
}

// LoadConfig loads a JACS config from the given path.
func LoadConfig(path string) (*Config, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, newError(ErrConfigNotFound, "config file not found: %s", path)
		}
		return nil, wrapError(ErrConfigInvalid, err, "failed to read config: %s", path)
	}

	var cfg Config
	if err := json.Unmarshal(data, &cfg); err != nil {
		return nil, wrapError(ErrConfigInvalid, err, "failed to parse config: %s", path)
	}

	return &cfg, nil
}

// DiscoverConfig searches for jacs.config.json using the following precedence:
//  1. JACS_CONFIG_PATH environment variable
//  2. ./jacs.config.json (current directory)
//  3. ~/.jacs/jacs.config.json (home directory)
func DiscoverConfig() (*Config, error) {
	// 1. Environment variable
	if envPath := os.Getenv("JACS_CONFIG_PATH"); envPath != "" {
		return LoadConfig(envPath)
	}

	// 2. Current directory
	if cfg, err := LoadConfig("jacs.config.json"); err == nil {
		return cfg, nil
	}

	// 3. Home directory
	home, err := os.UserHomeDir()
	if err == nil {
		homePath := filepath.Join(home, ".jacs", "jacs.config.json")
		if cfg, err := LoadConfig(homePath); err == nil {
			return cfg, nil
		}
	}

	return nil, newError(ErrConfigNotFound,
		"jacs.config.json not found. Set JACS_CONFIG_PATH or place it in . or ~/.jacs/")
}

// ResolveKeyPath resolves a private key file path relative to the config's key directory.
// If jacsKeyDir is empty, it defaults to the directory containing the config file.
//
// Searches in priority order for cross-SDK compatibility:
//  1. agent_private_key.pem (standard, matches Python SDK)
//  2. {agentName}.private.pem (legacy Go naming)
//  3. private_key.pem (legacy short name)
func ResolveKeyPath(cfg *Config, configPath string) string {
	keyDir := cfg.JacsKeyDir
	if keyDir == "" {
		keyDir = filepath.Dir(configPath)
	}

	candidates := []string{
		filepath.Join(keyDir, "agent_private_key.pem"),
		filepath.Join(keyDir, fmt.Sprintf("%s.private.pem", cfg.JacsAgentName)),
		filepath.Join(keyDir, "private_key.pem"),
	}

	for _, path := range candidates {
		if _, err := os.Stat(path); err == nil {
			return path
		}
	}

	// Default to standard name (will error at load time if missing)
	return candidates[0]
}

// ResolvePublicKeyPath resolves a public key file path with the same search
// priority as ResolveKeyPath.
func ResolvePublicKeyPath(cfg *Config, configPath string) string {
	keyDir := cfg.JacsKeyDir
	if keyDir == "" {
		keyDir = filepath.Dir(configPath)
	}

	candidates := []string{
		filepath.Join(keyDir, "agent_public_key.pem"),
		filepath.Join(keyDir, fmt.Sprintf("%s.public.pem", cfg.JacsAgentName)),
		filepath.Join(keyDir, "public_key.pem"),
	}

	for _, path := range candidates {
		if _, err := os.Stat(path); err == nil {
			return path
		}
	}

	return candidates[0]
}
