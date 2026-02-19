package haisdk

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

const (
	envConfigPath          = "JACS_CONFIG_PATH"
	envPrivateKeyPassword  = "JACS_PRIVATE_KEY_PASSWORD"
	envPasswordFile        = "JACS_PASSWORD_FILE"
	envDisablePasswordEnv  = "JACS_DISABLE_PASSWORD_ENV"
	envDisablePasswordFile = "JACS_DISABLE_PASSWORD_FILE"
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
	cfg, _, err := discoverConfigWithPath()
	return cfg, err
}

// discoverConfigWithPath is DiscoverConfig plus the resolved config file path.
// The returned path is absolute when possible.
func discoverConfigWithPath() (*Config, string, error) {
	// 1. Environment variable
	if envPath := os.Getenv(envConfigPath); envPath != "" {
		cfg, err := LoadConfig(envPath)
		if err != nil {
			return nil, "", err
		}
		return cfg, absPathOrOriginal(envPath), nil
	}

	// 2. Current directory
	const localPath = "jacs.config.json"
	if cfg, err := LoadConfig(localPath); err == nil {
		return cfg, absPathOrOriginal(localPath), nil
	}

	// 3. Home directory
	home, err := os.UserHomeDir()
	if err == nil {
		homePath := filepath.Join(home, ".jacs", "jacs.config.json")
		if cfg, err := LoadConfig(homePath); err == nil {
			return cfg, absPathOrOriginal(homePath), nil
		}
	}

	return nil, "", newError(ErrConfigNotFound,
		"jacs.config.json not found. Set JACS_CONFIG_PATH or place it in . or ~/.jacs/")
}

func absPathOrOriginal(path string) string {
	abs, err := filepath.Abs(path)
	if err != nil {
		return path
	}
	return abs
}

func isSourceDisabled(flagName string) bool {
	value := strings.ToLower(strings.TrimSpace(os.Getenv(flagName)))
	switch value {
	case "1", "true", "yes", "on":
		return true
	default:
		return false
	}
}

func trimTrailingNewlines(value string) string {
	return strings.TrimRight(value, "\r\n")
}

// ResolvePrivateKeyPassword resolves the local private-key password from
// configured secret sources.
//
// Exactly one source must be configured after source filters are applied.
// Sources:
//   - JACS_PRIVATE_KEY_PASSWORD (developer default)
//   - JACS_PASSWORD_FILE
//
// Optional source disable flags:
//   - JACS_DISABLE_PASSWORD_ENV=1
//   - JACS_DISABLE_PASSWORD_FILE=1
func ResolvePrivateKeyPassword() ([]byte, error) {
	envEnabled := !isSourceDisabled(envDisablePasswordEnv)
	fileEnabled := !isSourceDisabled(envDisablePasswordFile)

	envPassword := os.Getenv(envPrivateKeyPassword)
	passwordFile := os.Getenv(envPasswordFile)

	configured := make([]string, 0, 2)
	if envEnabled && envPassword != "" {
		configured = append(configured, envPrivateKeyPassword)
	}
	if fileEnabled && passwordFile != "" {
		configured = append(configured, envPasswordFile)
	}

	if len(configured) > 1 {
		return nil, newError(
			ErrConfigInvalid,
			"multiple password sources configured: %s; configure exactly one",
			strings.Join(configured, ", "),
		)
	}

	if len(configured) == 0 {
		return nil, newError(
			ErrConfigInvalid,
			"private key password required: configure exactly one of %s or %s",
			envPrivateKeyPassword,
			envPasswordFile,
		)
	}

	if configured[0] == envPrivateKeyPassword {
		return []byte(envPassword), nil
	}

	data, err := os.ReadFile(passwordFile)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, newError(ErrConfigInvalid, "%s does not exist: %s", envPasswordFile, passwordFile)
		}
		return nil, wrapError(ErrConfigInvalid, err, "failed to read %s: %s", envPasswordFile, passwordFile)
	}

	password := trimTrailingNewlines(string(data))
	if password == "" {
		return nil, newError(ErrConfigInvalid, "%s is empty: %s", envPasswordFile, passwordFile)
	}

	return []byte(password), nil
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
	configDir := filepath.Dir(configPath)
	if keyDir == "" {
		keyDir = configDir
	} else if !filepath.IsAbs(keyDir) {
		keyDir = filepath.Join(configDir, keyDir)
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
	configDir := filepath.Dir(configPath)
	if keyDir == "" {
		keyDir = configDir
	} else if !filepath.IsAbs(keyDir) {
		keyDir = filepath.Join(configDir, keyDir)
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
