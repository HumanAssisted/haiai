// Package haiai provides the Go SDK for the HAI.AI agent benchmarking platform.
//
// All authentication uses JACS agent identity (Ed25519 signatures).
// There is no API key authentication path.
//
// Quick start:
//
//	client, err := haiai.NewClient()
//	if err != nil {
//	    log.Fatal(err)
//	}
//
//	result, err := client.Hello(ctx)
package haiai

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"fmt"
	"io"
	"math"
	"net/http"
	neturl "net/url"
	"os"
	"strings"
	"sync"
	"time"
)

const (
	// maxResponseSize is the maximum allowed response body size (10 MB).
	maxResponseSize = 10 * 1024 * 1024

	// defaultMaxRetries is the default number of retries for retryable HTTP errors.
	defaultMaxRetries = 3
)

// retryableStatusCodes lists HTTP status codes that trigger automatic retry.
var retryableStatusCodes = map[int]bool{
	429: true,
	500: true,
	502: true,
	503: true,
	504: true,
}

// limitedReadAll reads from r up to maxResponseSize bytes.
// Returns an error if the response body exceeds the limit.
func limitedReadAll(r io.ReadCloser) ([]byte, error) {
	lr := io.LimitReader(r, int64(maxResponseSize)+1)
	data, err := io.ReadAll(lr)
	if err != nil {
		return nil, err
	}
	if len(data) > maxResponseSize {
		return nil, fmt.Errorf("response body exceeds maximum allowed size of %d bytes", maxResponseSize)
	}
	return data, nil
}

const (
	// DefaultEndpoint is the default HAI API endpoint.
	DefaultEndpoint = "https://beta.hai.ai"

	// DefaultKeysEndpoint is the default HAI key distribution service.
	DefaultKeysEndpoint = "https://keys.hai.ai"
)

// Client is the HAI SDK client. It authenticates using JACS agent identity.
type Client struct {
	endpoint   string
	jacsID     string
	mu         sync.RWMutex // protects haiAgentID and agentEmail
	haiAgentID string       // HAI-assigned agent UUID for email URL paths (set after registration)
	agentEmail string       // Agent's @hai.ai email address (set after ClaimUsername)
	privateKey ed25519.PrivateKey
	crypto     CryptoBackend // signing/verification backend (JACS CGo)
	httpClient *http.Client
	agentKeys  *keyCache // Agent key cache with 5-minute TTL
	maxRetries int       // maximum number of retries for retryable HTTP errors (default 3)
}

// Option configures a Client.
type Option func(*Client)

// WithEndpoint sets the HAI API base URL.
func WithEndpoint(endpoint string) Option {
	return func(c *Client) {
		c.endpoint = strings.TrimRight(endpoint, "/")
	}
}

// WithJACSID sets the JACS agent ID explicitly.
func WithJACSID(jacsID string) Option {
	return func(c *Client) {
		c.jacsID = jacsID
	}
}

// WithPrivateKey sets the Ed25519 private key explicitly.
func WithPrivateKey(key ed25519.PrivateKey) Option {
	return func(c *Client) {
		c.privateKey = key
	}
}

// WithHaiAgentID sets the HAI-assigned agent UUID (used for email URL paths).
func WithHaiAgentID(id string) Option {
	return func(c *Client) {
		c.haiAgentID = id
	}
}

// WithHTTPClient sets a custom HTTP client.
func WithHTTPClient(httpClient *http.Client) Option {
	return func(c *Client) {
		c.httpClient = httpClient
	}
}

// WithTimeout sets the HTTP client timeout.
func WithTimeout(timeout time.Duration) Option {
	return func(c *Client) {
		c.httpClient.Timeout = timeout
	}
}

// WithMaxRetries sets the maximum number of retries for retryable HTTP errors.
// Default is 3. Set to 0 to disable retries.
func WithMaxRetries(n int) Option {
	return func(c *Client) {
		c.maxRetries = n
	}
}

// NewClient creates a new HAI client.
//
// With no options, it auto-discovers jacs.config.json and loads the private key.
// Use options to override specific settings.
func NewClient(opts ...Option) (*Client, error) {
	cl := &Client{
		endpoint: DefaultEndpoint,
		httpClient: &http.Client{
			Timeout: 30 * time.Second,
		},
		agentKeys:  newKeyCache(),
		maxRetries: defaultMaxRetries,
	}

	// Apply options first -- user-provided values take priority
	for _, opt := range opts {
		opt(cl)
	}

	// Override endpoint from environment if not set by option
	if envURL := os.Getenv("HAI_URL"); envURL != "" && cl.endpoint == DefaultEndpoint {
		cl.endpoint = strings.TrimRight(envURL, "/")
	}

	// Auto-discover config if jacsID or privateKey are missing
	if cl.jacsID == "" || cl.privateKey == nil {
		cfg, configPath, err := discoverConfigWithPath()
		if err != nil {
			return nil, err
		}

		if cl.jacsID == "" {
			cl.jacsID = cfg.JacsID
		}

		if cl.privateKey == nil {
			keyPath := ResolveKeyPath(cfg, configPath)
			password, err := ResolvePrivateKeyPassword()
			if err != nil {
				return nil, err
			}

			key, err := LoadPrivateKey(keyPath, password)
			if err != nil {
				return nil, err
			}
			cl.privateKey = key
		}
	}

	if cl.jacsID == "" {
		return nil, newError(ErrConfigInvalid, "jacsId is empty in config")
	}

	// Initialize crypto backend (JACS CGo agent)
	cl.crypto = newClientCryptoBackend(cl.privateKey, cl.jacsID)

	return cl, nil
}

// Endpoint returns the base endpoint URL.
func (c *Client) Endpoint() string {
	return c.endpoint
}

// JacsID returns the agent's JACS ID.
func (c *Client) JacsID() string {
	return c.jacsID
}

// HaiAgentID returns the HAI-assigned agent UUID. Falls back to jacsID if not set.
func (c *Client) HaiAgentID() string {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if c.haiAgentID != "" {
		return c.haiAgentID
	}
	return c.jacsID
}

// SetHaiAgentID sets the HAI-assigned agent UUID (used for email URL paths).
func (c *Client) SetHaiAgentID(id string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.haiAgentID = id
}

// doRequest performs an authenticated HTTP request and decodes the JSON response.
// It retries on retryable status codes (429, 500, 502, 503, 504) with exponential backoff.
func (c *Client) doRequest(ctx context.Context, method, path string, body interface{}, result interface{}) error {
	url := c.endpoint + path

	var bodyData []byte
	if body != nil {
		var err error
		bodyData, err = json.Marshal(body)
		if err != nil {
			return wrapError(ErrInvalidResponse, err, "failed to marshal request body")
		}
	}

	maxRetries := c.maxRetries
	var lastErr error

	for attempt := 0; attempt <= maxRetries; attempt++ {
		if attempt > 0 {
			// Exponential backoff: 100ms, 200ms, 400ms, ...
			backoff := time.Duration(math.Pow(2, float64(attempt-1))) * 100 * time.Millisecond
			select {
			case <-ctx.Done():
				return ctx.Err()
			case <-time.After(backoff):
			}
		}

		var bodyReader io.Reader
		if bodyData != nil {
			bodyReader = bytes.NewReader(bodyData)
		}

		req, err := http.NewRequestWithContext(ctx, method, url, bodyReader)
		if err != nil {
			return wrapError(ErrConnection, err, "failed to create request")
		}

		if err := c.setAuthHeaders(req); err != nil {
			return err
		}

		resp, err := c.httpClient.Do(req)
		if err != nil {
			return wrapError(ErrConnection, err, "request failed")
		}

		if resp.StatusCode >= 200 && resp.StatusCode < 300 {
			defer resp.Body.Close()
			if result != nil {
				if err := json.NewDecoder(resp.Body).Decode(result); err != nil {
					return wrapError(ErrInvalidResponse, err, "failed to decode response")
				}
			}
			return nil
		}

		respBody, _ := limitedReadAll(resp.Body)
		resp.Body.Close()

		if retryableStatusCodes[resp.StatusCode] && attempt < maxRetries {
			lastErr = classifyHTTPError(resp.StatusCode, respBody)
			continue
		}

		return classifyHTTPError(resp.StatusCode, respBody)
	}

	return lastErr
}

// doPublicRequest performs an unauthenticated HTTP request and decodes the JSON response.
func (c *Client) doPublicRequest(ctx context.Context, method, path string, result interface{}) error {
	url := c.endpoint + path

	req, err := http.NewRequestWithContext(ctx, method, url, nil)
	if err != nil {
		return wrapError(ErrConnection, err, "failed to create request")
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return wrapError(ErrConnection, err, "request failed")
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		respBody, _ := limitedReadAll(resp.Body)
		return classifyHTTPError(resp.StatusCode, respBody)
	}

	if result != nil {
		if err := json.NewDecoder(resp.Body).Decode(result); err != nil {
			return wrapError(ErrInvalidResponse, err, "failed to decode response")
		}
	}

	return nil
}

// doPublicJSONRequest performs an unauthenticated HTTP request with optional JSON body.
func (c *Client) doPublicJSONRequest(ctx context.Context, method, path string, body interface{}, result interface{}) error {
	url := c.endpoint + path

	var bodyReader io.Reader
	if body != nil {
		data, err := json.Marshal(body)
		if err != nil {
			return wrapError(ErrInvalidResponse, err, "failed to marshal request body")
		}
		bodyReader = bytes.NewReader(data)
	}

	req, err := http.NewRequestWithContext(ctx, method, url, bodyReader)
	if err != nil {
		return wrapError(ErrConnection, err, "failed to create request")
	}
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return wrapError(ErrConnection, err, "request failed")
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		respBody, _ := limitedReadAll(resp.Body)
		return classifyHTTPError(resp.StatusCode, respBody)
	}

	if result != nil {
		if err := json.NewDecoder(resp.Body).Decode(result); err != nil {
			return wrapError(ErrInvalidResponse, err, "failed to decode response")
		}
	}

	return nil
}

// classifyHTTPError maps HTTP status codes to appropriate ErrorKind values.
func classifyHTTPError(statusCode int, body []byte) *Error {
	msg := fmt.Sprintf("status %d: %s", statusCode, string(body))
	switch statusCode {
	case http.StatusUnauthorized:
		return newError(ErrAuthRequired, msg)
	case http.StatusForbidden:
		return newError(ErrForbidden, msg)
	case http.StatusNotFound:
		return newError(ErrNotFound, msg)
	case http.StatusTooManyRequests:
		return newError(ErrRateLimited, msg)
	default:
		return newError(ErrInvalidResponse, msg)
	}
}

// =============================================================================
// API Methods
// =============================================================================

// Hello tests connectivity and authentication with HAI.
func (c *Client) Hello(ctx context.Context) (*HelloResult, error) {
	reqBody := map[string]string{
		"agent_id": c.jacsID,
	}
	var result HelloResult
	if err := c.doRequest(ctx, http.MethodPost, "/api/v1/agents/hello", reqBody, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// TestConnection verifies connectivity to the HAI server (unauthenticated).
func (c *Client) TestConnection(ctx context.Context) (bool, error) {
	endpoints := []string{"/api/v1/health", "/health", "/api/health", "/"}
	var lastErr error

	for _, endpoint := range endpoints {
		url := c.endpoint + endpoint
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
		if err != nil {
			return false, wrapError(ErrConnection, err, "failed to create request")
		}

		resp, err := c.httpClient.Do(req)
		if err != nil {
			lastErr = err
			continue
		}

		if resp.StatusCode >= 200 && resp.StatusCode < 300 {
			var health struct {
				Status string `json:"status"`
			}
			if err := json.NewDecoder(resp.Body).Decode(&health); err != nil {
				_ = resp.Body.Close()
				return true, nil
			}
			_ = resp.Body.Close()
			if health.Status == "" || health.Status == "ok" || health.Status == "healthy" {
				return true, nil
			}
		} else {
			_ = resp.Body.Close()
		}
	}

	if lastErr != nil {
		return false, wrapError(ErrConnection, lastErr, "connection failed")
	}
	return false, nil
}

// RegisterOptions configures the Register call.
type RegisterOptions struct {
	AgentJSON  string `json:"agent_json"`
	PublicKey  string `json:"public_key,omitempty"`
	OwnerEmail string `json:"owner_email,omitempty"`
}

// Register registers the agent with HAI.
// The public key PEM is base64-encoded on the wire to match Python/Node SDKs.
func (c *Client) Register(ctx context.Context, opts RegisterOptions) (*RegistrationResult, error) {
	wireOpts := opts
	if wireOpts.PublicKey != "" {
		wireOpts.PublicKey = base64.StdEncoding.EncodeToString([]byte(opts.PublicKey))
	}

	var result RegistrationResult
	if err := c.doRequest(ctx, http.MethodPost, "/api/v1/agents/register", wireOpts, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// RotateKeys rotates the agent's cryptographic keys.
//
// It archives old keys, generates a new Ed25519 keypair, builds a new
// self-signed agent document, updates config, and optionally re-registers
// with HAI.
//
// The config file (jacs.config.json) is updated with the new version.
// If opts is nil, defaults are used (registerWithHai=true).
func (c *Client) RotateKeys(ctx context.Context, opts *RotateKeysOptions) (*RotationResult, error) {
	if c.jacsID == "" {
		return nil, newError(ErrConfigInvalid, "cannot rotate keys: no jacsId")
	}

	registerWithHai := true
	configPath := ""
	if opts != nil {
		if opts.RegisterWithHai != nil {
			registerWithHai = *opts.RegisterWithHai
		}
		configPath = opts.ConfigPath
	}

	// Discover config to find key paths
	var cfg *Config
	if configPath != "" {
		var err error
		cfg, err = LoadConfig(configPath)
		if err != nil {
			return nil, err
		}
	} else {
		var cfgPath string
		var err error
		cfg, cfgPath, err = discoverConfigWithPath()
		if err != nil {
			return nil, err
		}
		configPath = cfgPath
	}

	oldVersion := cfg.JacsAgentVersion

	// Save old private key for chain-of-trust auth during re-registration
	oldPrivateKey := c.privateKey

	// Resolve key paths
	privKeyPath := ResolveKeyPath(cfg, configPath)
	pubKeyPath := ResolvePublicKeyPath(cfg, configPath)

	// 1. Archive old keys
	archivePriv := strings.TrimSuffix(privKeyPath, ".pem") + "." + oldVersion + ".pem"
	archivePub := strings.TrimSuffix(pubKeyPath, ".pem") + "." + oldVersion + ".pem"

	if err := os.Rename(privKeyPath, archivePriv); err != nil {
		return nil, wrapError(ErrSigningFailed, err, "failed to archive private key")
	}
	// Only ignore file-not-found for public key; warn on other errors
	if err := os.Rename(pubKeyPath, archivePub); err != nil && !os.IsNotExist(err) {
		// Non-fatal but log a warning
		fmt.Fprintf(os.Stderr, "warning: failed to archive public key: %v\n", err)
	}

	// 2. Generate new keypair via CryptoBackend
	pubPEM, privPEM, err := c.crypto.GenerateKeyPair()
	if err != nil {
		// Rollback: restore archived keys
		_ = os.Rename(archivePriv, privKeyPath)
		_ = os.Rename(archivePub, pubKeyPath)
		return nil, wrapError(ErrSigningFailed, err, "key generation failed")
	}

	// Parse the generated keys for local use
	newPriv, err := ParsePrivateKey(privPEM)
	if err != nil {
		_ = os.Rename(archivePriv, privKeyPath)
		_ = os.Rename(archivePub, pubKeyPath)
		return nil, wrapError(ErrSigningFailed, err, "failed to parse generated private key")
	}
	newPub := newPriv.Public().(ed25519.PublicKey)

	// Re-encode to canonical PEM formats for disk storage
	pubDER, err := x509.MarshalPKIXPublicKey(newPub)
	if err != nil {
		_ = os.Rename(archivePriv, privKeyPath)
		_ = os.Rename(archivePub, pubKeyPath)
		return nil, wrapError(ErrSigningFailed, err, "failed to marshal new public key")
	}
	pubPEM = pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: pubDER})

	privDER, err := x509.MarshalPKCS8PrivateKey(newPriv)
	if err != nil {
		_ = os.Rename(archivePriv, privKeyPath)
		_ = os.Rename(archivePub, pubKeyPath)
		return nil, wrapError(ErrSigningFailed, err, "failed to marshal new private key")
	}
	privPEM = pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: privDER})

	if err := os.WriteFile(privKeyPath, privPEM, 0o600); err != nil {
		_ = os.Rename(archivePriv, privKeyPath)
		_ = os.Rename(archivePub, pubKeyPath)
		return nil, wrapError(ErrSigningFailed, err, "failed to write new private key")
	}
	if err := os.WriteFile(pubKeyPath, pubPEM, 0o644); err != nil {
		// Best-effort cleanup
		_ = os.Rename(archivePriv, privKeyPath)
		_ = os.Rename(archivePub, pubKeyPath)
		return nil, wrapError(ErrSigningFailed, err, "failed to write new public key")
	}

	// 4. Build new signed agent document
	newVersion := generateUUID()
	pubPEMStr := string(pubPEM)
	now := time.Now().UTC().Format(time.RFC3339)

	// Preserve description from config if available, otherwise use default
	description := "Agent registered via Go SDK"
	if cfg.Description != "" {
		description = cfg.Description
	}
	doc := map[string]interface{}{
		"jacsId":              c.jacsID,
		"jacsVersion":         newVersion,
		"jacsPreviousVersion": oldVersion,
		"jacsPublicKey":       pubPEMStr,
		"name":                cfg.JacsAgentName,
		"description":         description,
		"jacsSignature": map[string]interface{}{
			"agentID": c.jacsID,
			"date":    now,
		},
	}

	// Canonical JSON (Go encoding/json sorts map keys by default)
	canonical, err := json.Marshal(doc)
	if err != nil {
		return nil, wrapError(ErrSigningFailed, err, "failed to marshal agent document")
	}

	// Sign with the NEW key via a temporary CryptoBackend
	newKeyBackend := newClientCryptoBackend(newPriv, c.jacsID)
	sig, signErr := newKeyBackend.SignBytes(canonical)
	if signErr != nil {
		return nil, wrapError(ErrSigningFailed, signErr, "failed to sign agent document with new key")
	}
	sigB64 := base64.StdEncoding.EncodeToString(sig)
	doc["jacsSignature"].(map[string]interface{})["signature"] = sigB64

	agentJSON, err := json.Marshal(doc)
	if err != nil {
		return nil, wrapError(ErrSigningFailed, err, "failed to marshal signed agent document")
	}
	signedAgentJSON := string(agentJSON)

	// 5. Compute new public key hash
	newPublicKeyHash := fmt.Sprintf("%x", sha256.Sum256(pubDER))

	// 6. Update in-memory state
	c.privateKey = newPriv
	c.crypto = newClientCryptoBackend(newPriv, c.jacsID)

	// 7. Update config file
	cfgData, err := os.ReadFile(configPath)
	if err == nil {
		var rawCfg map[string]interface{}
		if json.Unmarshal(cfgData, &rawCfg) == nil {
			rawCfg["jacsAgentVersion"] = newVersion
			if updated, err := json.MarshalIndent(rawCfg, "", "  "); err == nil {
				_ = os.WriteFile(configPath, append(updated, '\n'), 0o600)
			}
		}
	}

	// 8. Optionally re-register with HAI using the OLD key for auth
	// (chain of trust: old key vouches for new key)
	registeredWithHai := false
	if registerWithHai {
		regBody := RegisterOptions{
			AgentJSON: signedAgentJSON,
			PublicKey: pubPEMStr,
		}
		wireOpts := regBody
		if wireOpts.PublicKey != "" {
			wireOpts.PublicKey = base64.StdEncoding.EncodeToString([]byte(regBody.PublicKey))
		}
		bodyData, err := json.Marshal(wireOpts)
		if err == nil {
			regURL := c.endpoint + "/api/v1/agents/register"
			req, reqErr := http.NewRequestWithContext(ctx, http.MethodPost, regURL, bytes.NewReader(bodyData))
			if reqErr == nil {
				// Build 4-part auth header signed by OLD key via CryptoBackend
				oldKeyBackend := newClientCryptoBackend(oldPrivateKey, c.jacsID)
				authHeader, authErr := build4PartAuthHeaderWithBackend(c.jacsID, oldVersion, oldKeyBackend)
				if authErr == nil {
					req.Header.Set("Authorization", authHeader)
					req.Header.Set("Content-Type", "application/json")
					resp, doErr := c.httpClient.Do(req)
					if doErr == nil {
						defer resp.Body.Close()
						if resp.StatusCode >= 200 && resp.StatusCode < 300 {
							registeredWithHai = true
						}
					}
				}
			}
		}
		// HAI registration failure is non-fatal
	}

	return &RotationResult{
		JacsID:            c.jacsID,
		OldVersion:        oldVersion,
		NewVersion:        newVersion,
		NewPublicKeyHash:  newPublicKeyHash,
		RegisteredWithHai: registeredWithHai,
		SignedAgentJSON:   signedAgentJSON,
	}, nil
}

// registerWithoutAuth registers an agent document without JACS request auth.
// New-agent registration is self-authenticated by the signed agent document.
func (c *Client) registerWithoutAuth(ctx context.Context, opts RegisterOptions) (*RegistrationResult, error) {
	wireOpts := opts
	if wireOpts.PublicKey != "" {
		wireOpts.PublicKey = base64.StdEncoding.EncodeToString([]byte(opts.PublicKey))
	}

	body, err := json.Marshal(wireOpts)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to marshal registration request")
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.endpoint+"/api/v1/agents/register", bytes.NewReader(body))
	if err != nil {
		return nil, wrapError(ErrConnection, err, "failed to create registration request")
	}
	req.Header.Set("Content-Type", "application/json")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, wrapError(ErrConnection, err, "registration request failed")
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		respBody, _ := limitedReadAll(resp.Body)
		return nil, classifyHTTPError(resp.StatusCode, respBody)
	}

	var result RegistrationResult
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode registration response")
	}

	return &result, nil
}

// Status checks the registration/verification status of this agent with HAI.
// Calls GET /api/v1/agents/{jacs_id}/verify.
func (c *Client) Status(ctx context.Context) (*StatusResult, error) {
	path := fmt.Sprintf("/api/v1/agents/%s/verify", neturl.PathEscape(c.jacsID))

	url := c.endpoint + path
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, wrapError(ErrConnection, err, "failed to create request")
	}
	if err := c.setAuthHeaders(req); err != nil {
		return nil, err
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, wrapError(ErrConnection, err, "request failed")
	}
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusNotFound {
		return &StatusResult{
			Registered: false,
			JacsID:     c.jacsID,
		}, nil
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		body, _ := limitedReadAll(resp.Body)
		return nil, classifyHTTPError(resp.StatusCode, body)
	}

	var result StatusResult
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode verify response")
	}

	if result.JacsID == "" {
		result.JacsID = c.jacsID
	}

	return &result, nil
}

// Benchmark runs a benchmark suite at the given tier.
// Calls POST /api/benchmark/run with {name, tier}.
func (c *Client) Benchmark(ctx context.Context, tier string) (*BenchmarkResult, error) {
	reqBody := struct {
		Name      string `json:"name"`
		Tier      string `json:"tier"`
		Transport string `json:"transport"`
	}{
		Name:      generateBenchmarkName(tier, c.jacsID),
		Tier:      tier,
		Transport: "sse",
	}

	var result BenchmarkResult
	if err := c.doRequest(ctx, http.MethodPost, "/api/benchmark/run", reqBody, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// generateBenchmarkName creates a descriptive benchmark run name.
func generateBenchmarkName(tier, jacsID string) string {
	displayNames := map[string]string{
		"free":       "Free",
		"pro":        "Pro",
		"enterprise": "Enterprise",
		// Legacy names (backward compat during transition)
		"dns_certified":   "Pro",
		"fully_certified": "Enterprise",
		"free_chaotic":    "Free",
		"baseline":        "Pro",
		"certified":       "Enterprise",
	}

	display, ok := displayNames[tier]
	if !ok {
		display = tier
	}

	suffix := jacsID
	if len(suffix) > 8 {
		suffix = suffix[:8]
	}
	if suffix == "" {
		suffix = time.Now().Format("20060102-150405")
	}

	return fmt.Sprintf("%s Run - %s", display, suffix)
}

// FreeRun runs the free benchmark tier.
func (c *Client) FreeRun(ctx context.Context) (*BenchmarkResult, error) {
	return c.Benchmark(ctx, "free")
}

// ProRunOptions configures the pro benchmark run.
type ProRunOptions struct {
	// OnCheckoutURL is called with the Stripe checkout URL when payment is
	// required. The caller is responsible for presenting this URL to the user
	// (e.g., opening a browser, printing to stdout, sending via chat).
	// If nil, ProRun returns ErrAuthRequired with the checkout URL in the
	// error message so the caller can still act on it.
	OnCheckoutURL func(checkoutURL string)

	// PollInterval is the time between payment status checks. Default 5s.
	PollInterval time.Duration

	// PollTimeout is the maximum time to wait for payment. Default 5 min.
	PollTimeout time.Duration
}

// ProRun runs the pro benchmark tier with Stripe checkout.
// It creates a subscription session, notifies the caller of the checkout URL
// via opts.OnCheckoutURL, polls for payment confirmation, then runs the benchmark.
//
// If opts is nil, defaults are used and the checkout URL is returned in the
// error message when payment is required.
func (c *Client) ProRun(ctx context.Context, opts *ProRunOptions) (*BenchmarkResult, error) {
	pollInterval := 5 * time.Second
	pollTimeout := 5 * time.Minute
	if opts != nil {
		if opts.PollInterval > 0 {
			pollInterval = opts.PollInterval
		}
		if opts.PollTimeout > 0 {
			pollTimeout = opts.PollTimeout
		}
	}

	// 1. Create subscription session.
	var sub struct {
		CheckoutURL string `json:"checkout_url"`
		SessionID   string `json:"session_id"`
		AlreadyPaid bool   `json:"already_paid"`
	}
	err := c.doRequest(ctx, http.MethodPost, "/api/benchmark/purchase", map[string]string{
		"tier": "pro",
	}, &sub)
	if err != nil {
		return nil, err
	}

	// Skip checkout if already subscribed.
	if !sub.AlreadyPaid && sub.CheckoutURL != "" {
		// 2. Notify caller of checkout URL.
		if opts != nil && opts.OnCheckoutURL != nil {
			opts.OnCheckoutURL(sub.CheckoutURL)
		} else {
			return nil, newErrorWithAction(
				ErrAuthRequired,
				fmt.Sprintf("Open this URL to complete payment: %s", sub.CheckoutURL),
				"payment required for pro tier benchmark",
			)
		}

		// 3. Poll for payment confirmation.
		ticker := time.NewTicker(pollInterval)
		defer ticker.Stop()
		timeout := time.After(pollTimeout)

		for {
			select {
			case <-ticker.C:
				var status struct {
					Paid bool `json:"paid"`
				}
				statusPath := fmt.Sprintf("/api/benchmark/payments/%s/status", neturl.PathEscape(sub.SessionID))
				if err := c.doRequest(ctx, http.MethodGet, statusPath, nil, &status); err == nil && status.Paid {
					goto runBenchmark
				}
			case <-timeout:
				return nil, newError(ErrTimeout, "payment confirmation timed out after %v", pollTimeout)
			case <-ctx.Done():
				return nil, ctx.Err()
			}
		}
	}

runBenchmark:
	return c.Benchmark(ctx, "pro")
}

// DnsCertifiedRun is a deprecated alias for ProRun.
// Deprecated: Use ProRun instead. The tier was renamed from dns_certified to pro.
func (c *Client) DnsCertifiedRun(ctx context.Context) (*BenchmarkResult, error) {
	return c.ProRun(ctx, nil)
}

// EnterpriseRun runs an enterprise tier benchmark.
//
// The enterprise tier is coming soon.
// Contact support@hai.ai for early access.
func (c *Client) EnterpriseRun(ctx context.Context) (*BenchmarkResult, error) {
	return nil, fmt.Errorf(
		"the enterprise tier is coming soon; " +
			"contact support@hai.ai for early access",
	)
}

// CertifiedRun is a deprecated alias for EnterpriseRun.
// Deprecated: Use EnterpriseRun instead. The tier was renamed from fully_certified to enterprise.
func (c *Client) CertifiedRun(ctx context.Context) (*BenchmarkResult, error) {
	return c.EnterpriseRun(ctx)
}

// SubmitResponse submits a moderation response for a benchmark job, wrapped
// in a signed JACS document envelope.
func (c *Client) SubmitResponse(ctx context.Context, jobID string, response ModerationResponse) (*JobResponseResult, error) {
	signedDoc, err := c.signResponse(response)
	if err != nil {
		return nil, err
	}

	path := fmt.Sprintf("/api/v1/agents/jobs/%s/response", neturl.PathEscape(jobID))
	var result JobResponseResult
	if err := c.doRequest(ctx, http.MethodPost, path, signedDoc, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// signResponse wraps a response payload in a JACS document envelope and signs it.
//
// Delegates to CryptoBackend.SignResponse and fails closed if the backend
// cannot produce a signed envelope.
func (c *Client) signResponse(response interface{}) (map[string]interface{}, error) {
	if c.crypto == nil {
		return nil, newError(ErrSigningFailed, "crypto backend is not initialized")
	}

	payloadBytes, err := json.Marshal(response)
	if err != nil {
		return nil, wrapError(ErrSigningFailed, err, "failed to marshal response for signing")
	}

	signedJSON, signErr := c.crypto.SignResponse(string(payloadBytes))
	if signErr != nil {
		return nil, wrapError(ErrSigningFailed, signErr, "failed to sign response")
	}

	var result map[string]interface{}
	if parseErr := json.Unmarshal([]byte(signedJSON), &result); parseErr != nil {
		return nil, wrapError(ErrSigningFailed, parseErr, "failed to parse signed response")
	}
	return result, nil
}

// GetAgentAttestation gets the agent's attestation from HAI.
func (c *Client) GetAgentAttestation(ctx context.Context) (*AttestationResult, error) {
	path := fmt.Sprintf("/api/v1/agents/%s/attestations", neturl.PathEscape(c.jacsID))
	var result AttestationResult
	if err := c.doRequest(ctx, http.MethodGet, path, nil, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// VerifyAgent verifies another agent's registration and identity with HAI.
func (c *Client) VerifyAgent(ctx context.Context, agentID string) (*VerifyResult, error) {
	path := fmt.Sprintf("/api/v1/agents/%s/verify", neturl.PathEscape(agentID))
	var result VerifyResult
	if err := c.doRequest(ctx, http.MethodGet, path, nil, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// VerifyDocument verifies a signed JACS document using HAI's public endpoint.
// Calls POST /api/jacs/verify with {"document": "<json>"}.
func (c *Client) VerifyDocument(ctx context.Context, document string) (*DocumentVerificationResult, error) {
	url := c.endpoint + "/api/jacs/verify"
	reqBody := struct {
		Document string `json:"document"`
	}{
		Document: document,
	}

	bodyBytes, err := json.Marshal(reqBody)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to encode verify payload")
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(bodyBytes))
	if err != nil {
		return nil, wrapError(ErrConnection, err, "failed to create verify request")
	}
	req.Header.Set("Content-Type", "application/json")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, wrapError(ErrConnection, err, "verify request failed")
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		respBody, _ := limitedReadAll(resp.Body)
		return nil, classifyHTTPError(resp.StatusCode, respBody)
	}

	var result DocumentVerificationResult
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode verify response")
	}
	return &result, nil
}

// GetVerification gets advanced 3-level verification status for an agent.
// Calls GET /api/v1/agents/{agent_id}/verification (public endpoint).
func (c *Client) GetVerification(ctx context.Context, agentID string) (*AgentVerificationResult, error) {
	path := fmt.Sprintf("/api/v1/agents/%s/verification", neturl.PathEscape(agentID))
	var result AgentVerificationResult
	if err := c.doPublicRequest(ctx, http.MethodGet, path, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// VerifyAgentDocument verifies an agent document with HAI's advanced verifier.
// Calls POST /api/v1/agents/verify (public endpoint).
func (c *Client) VerifyAgentDocument(ctx context.Context, request VerifyAgentDocumentRequest) (*AgentVerificationResult, error) {
	var result AgentVerificationResult
	if err := c.doPublicJSONRequest(ctx, http.MethodPost, "/api/v1/agents/verify", request, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// CheckUsername checks if a username is available for @hai.ai email.
// Calls GET /api/v1/agents/username/check?username={name}.
func (c *Client) CheckUsername(ctx context.Context, username string) (*CheckUsernameResult, error) {
	query := neturl.Values{}
	query.Set("username", username)
	path := "/api/v1/agents/username/check?" + query.Encode()
	var result CheckUsernameResult
	if err := c.doPublicRequest(ctx, http.MethodGet, path, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// ClaimUsername claims a username for an agent, getting {username}@hai.ai email.
// Calls POST /api/v1/agents/{agentID}/username.
func (c *Client) ClaimUsername(ctx context.Context, agentID string, username string) (*ClaimUsernameResult, error) {
	path := fmt.Sprintf("/api/v1/agents/%s/username", neturl.PathEscape(agentID))
	reqBody := struct {
		Username string `json:"username"`
	}{
		Username: username,
	}
	var result ClaimUsernameResult
	if err := c.doRequest(ctx, http.MethodPost, path, reqBody, &result); err != nil {
		return nil, err
	}
	if result.Email != "" {
		c.mu.Lock()
		c.agentEmail = result.Email
		c.mu.Unlock()
	}
	return &result, nil
}

// AgentEmail returns the agent's @hai.ai email address (set after ClaimUsername).
func (c *Client) AgentEmail() string {
	c.mu.RLock()
	defer c.mu.RUnlock()
	return c.agentEmail
}

// SetAgentEmail sets the agent's @hai.ai email address manually.
func (c *Client) SetAgentEmail(email string) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.agentEmail = email
}

// UpdateUsername renames an existing username for an agent.
// Calls PUT /api/v1/agents/{agentID}/username.
func (c *Client) UpdateUsername(ctx context.Context, agentID string, username string) (*UpdateUsernameResult, error) {
	path := fmt.Sprintf("/api/v1/agents/%s/username", neturl.PathEscape(agentID))
	reqBody := struct {
		Username string `json:"username"`
	}{
		Username: username,
	}
	var result UpdateUsernameResult
	if err := c.doRequest(ctx, http.MethodPut, path, reqBody, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// DeleteUsername releases an agent's claimed username.
// Calls DELETE /api/v1/agents/{agentID}/username.
func (c *Client) DeleteUsername(ctx context.Context, agentID string) (*DeleteUsernameResult, error) {
	path := fmt.Sprintf("/api/v1/agents/%s/username", neturl.PathEscape(agentID))
	var result DeleteUsernameResult
	if err := c.doRequest(ctx, http.MethodDelete, path, nil, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// SendEmail sends an email from this agent.
func (c *Client) SendEmail(ctx context.Context, to, subject, body string) (*SendEmailResult, error) {
	return c.SendEmailWithOptions(ctx, SendEmailOptions{To: to, Subject: subject, Body: body})
}

// SendEmailWithOptions sends an email with full options (e.g. threading).
// The server handles JACS attachment signing; the client only sends content fields.
//
// Returns typed sentinel errors that callers can check with errors.Is:
//   - ErrEmailNotActive: agent email is not provisioned or active
//   - ErrRecipientNotFound: the recipient address does not exist
//   - ErrEmailRateLimited: sending rate limit exceeded
func (c *Client) SendEmailWithOptions(ctx context.Context, opts SendEmailOptions) (*SendEmailResult, error) {
	c.mu.RLock()
	email := c.agentEmail
	c.mu.RUnlock()
	if email == "" {
		return nil, fmt.Errorf("%w: agent email not set — call ClaimUsername first", ErrEmailNotActive)
	}

	// Encode attachment data to base64 for JSON serialization
	for i := range opts.Attachments {
		if opts.Attachments[i].DataBase64 == "" && len(opts.Attachments[i].Data) > 0 {
			opts.Attachments[i].DataBase64 = base64.StdEncoding.EncodeToString(opts.Attachments[i].Data)
		}
	}

	url := c.endpoint + fmt.Sprintf("/api/agents/%s/email/send", neturl.PathEscape(c.HaiAgentID()))

	data, err := json.Marshal(opts)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to marshal send email request")
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(data))
	if err != nil {
		return nil, wrapError(ErrConnection, err, "failed to create request")
	}
	if err := c.setAuthHeaders(req); err != nil {
		return nil, err
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, wrapError(ErrConnection, err, "request failed")
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		respBody, _ := limitedReadAll(resp.Body)
		return nil, classifyEmailError(resp.StatusCode, respBody)
	}

	var result SendEmailResult
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode response")
	}
	return &result, nil
}

// classifyEmailError attempts to parse a structured API error response with
// an error_code field and maps known codes to sentinel errors. Falls back to
// the generic classifyHTTPError for unstructured responses.
func classifyEmailError(statusCode int, body []byte) error {
	var apiErr HaiAPIError
	if err := json.Unmarshal(body, &apiErr); err == nil && apiErr.ErrorCode != "" {
		apiErr.Status = statusCode
		switch apiErr.ErrorCode {
		case "EMAIL_NOT_ACTIVE":
			return fmt.Errorf("%w: %s", ErrEmailNotActive, apiErr.Message)
		case "RECIPIENT_NOT_FOUND":
			return fmt.Errorf("%w: %s", ErrRecipientNotFound, apiErr.Message)
		case "RATE_LIMITED":
			return fmt.Errorf("%w: %s", ErrEmailRateLimited, apiErr.Message)
		default:
			return &apiErr
		}
	}
	return classifyHTTPError(statusCode, body)
}

// SignEmail sends a raw RFC 5322 email to the HAI server for JACS attachment signing.
// The server signs the email and returns the signed email bytes (with JACS signature
// attachment added).
func (c *Client) SignEmail(ctx context.Context, rawEmail []byte) ([]byte, error) {
	url := c.endpoint + "/api/v1/email/sign"

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(rawEmail))
	if err != nil {
		return nil, wrapError(ErrConnection, err, "failed to create sign email request")
	}
	if err := c.setAuthHeaders(req); err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "message/rfc822")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, wrapError(ErrConnection, err, "sign email request failed")
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		respBody, _ := limitedReadAll(resp.Body)
		return nil, classifyHTTPError(resp.StatusCode, respBody)
	}

	return limitedReadAll(resp.Body)
}

// SendSignedEmail builds an RFC 5322 MIME email and sends it.
//
// Deprecated: SendSignedEmail currently delegates to SendEmailWithOptions.
// The previous implementation called /api/v1/email/sign (HAI authority key)
// then POSTed to send-signed, which rejects because the signer ID does not
// match the authenticated agent. True agent-key local signing will be
// available when the Rust SDK core (DevEx TASK_017) ships.
// Use SendEmail or SendEmailWithOptions directly.
func (c *Client) SendSignedEmail(ctx context.Context, opts SendEmailOptions) (*SendEmailResult, error) {
	// Deprecated: delegates to SendEmailWithOptions until local agent-key
	// signing is available (DevEx TASK_017).
	return c.SendEmailWithOptions(ctx, opts)
}

// VerifyEmail sends a raw RFC 5322 email to the HAI server for JACS signature verification.
// The server verifies the JACS attachment signature and returns a detailed result including
// per-field verification status and the chain of custody.
func (c *Client) VerifyEmail(ctx context.Context, rawEmail []byte) (*EmailVerificationResultV2, error) {
	url := c.endpoint + "/api/v1/email/verify"

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(rawEmail))
	if err != nil {
		return nil, wrapError(ErrConnection, err, "failed to create verify email request")
	}
	if err := c.setAuthHeaders(req); err != nil {
		return nil, err
	}
	req.Header.Set("Content-Type", "message/rfc822")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, wrapError(ErrConnection, err, "verify email request failed")
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		respBody, _ := limitedReadAll(resp.Body)
		return nil, classifyHTTPError(resp.StatusCode, respBody)
	}

	var result EmailVerificationResultV2
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode verify email response")
	}
	return &result, nil
}

// ListMessages retrieves messages from the agent's mailbox.
func (c *Client) ListMessages(ctx context.Context, opts ListMessagesOptions) ([]EmailMessage, error) {
	query := neturl.Values{}
	query.Set("limit", fmt.Sprintf("%d", opts.Limit))
	query.Set("offset", fmt.Sprintf("%d", opts.Offset))
	if opts.Direction != "" {
		query.Set("direction", opts.Direction)
	}
	if opts.IsRead != nil {
		query.Set("is_read", fmt.Sprintf("%t", *opts.IsRead))
	}
	if opts.Folder != "" {
		query.Set("folder", opts.Folder)
	}
	if opts.Label != "" {
		query.Set("label", opts.Label)
	}
	if opts.HasAttachments != nil {
		query.Set("has_attachments", fmt.Sprintf("%t", *opts.HasAttachments))
	}
	if opts.Since != "" {
		query.Set("since", opts.Since)
	}
	if opts.Until != "" {
		query.Set("until", opts.Until)
	}
	path := fmt.Sprintf("/api/agents/%s/email/messages?%s", neturl.PathEscape(c.HaiAgentID()), query.Encode())
	var wrapper ListMessagesResponse
	if err := c.doRequest(ctx, http.MethodGet, path, nil, &wrapper); err != nil {
		return nil, err
	}
	return wrapper.Messages, nil
}

// MarkRead marks a message as read.
func (c *Client) MarkRead(ctx context.Context, messageID string) error {
	path := fmt.Sprintf(
		"/api/agents/%s/email/messages/%s/read",
		neturl.PathEscape(c.HaiAgentID()),
		neturl.PathEscape(messageID),
	)
	return c.doRequest(ctx, http.MethodPost, path, nil, nil)
}

// GetEmailStatus retrieves the agent's email usage and limits.
func (c *Client) GetEmailStatus(ctx context.Context) (*EmailStatus, error) {
	path := fmt.Sprintf("/api/agents/%s/email/status", neturl.PathEscape(c.HaiAgentID()))
	var status EmailStatus
	if err := c.doRequest(ctx, http.MethodGet, path, nil, &status); err != nil {
		return nil, err
	}
	return &status, nil
}

// GetMessage retrieves a single email message by ID.
func (c *Client) GetMessage(ctx context.Context, messageID string) (*EmailMessage, error) {
	path := fmt.Sprintf(
		"/api/agents/%s/email/messages/%s",
		neturl.PathEscape(c.HaiAgentID()),
		neturl.PathEscape(messageID),
	)
	var msg EmailMessage
	if err := c.doRequest(ctx, http.MethodGet, path, nil, &msg); err != nil {
		return nil, err
	}
	return &msg, nil
}

// DeleteMessage deletes an email message by ID.
func (c *Client) DeleteMessage(ctx context.Context, messageID string) error {
	path := fmt.Sprintf(
		"/api/agents/%s/email/messages/%s",
		neturl.PathEscape(c.HaiAgentID()),
		neturl.PathEscape(messageID),
	)
	return c.doRequest(ctx, http.MethodDelete, path, nil, nil)
}

// MarkUnread marks a message as unread.
func (c *Client) MarkUnread(ctx context.Context, messageID string) error {
	path := fmt.Sprintf(
		"/api/agents/%s/email/messages/%s/unread",
		neturl.PathEscape(c.HaiAgentID()),
		neturl.PathEscape(messageID),
	)
	return c.doRequest(ctx, http.MethodPost, path, nil, nil)
}

// SearchMessages searches the agent's mailbox.
func (c *Client) SearchMessages(ctx context.Context, opts SearchOptions) ([]EmailMessage, error) {
	query := neturl.Values{}
	if opts.Q != "" {
		query.Set("q", opts.Q)
	}
	if opts.Direction != "" {
		query.Set("direction", opts.Direction)
	}
	if opts.FromAddress != "" {
		query.Set("from_address", opts.FromAddress)
	}
	if opts.ToAddress != "" {
		query.Set("to_address", opts.ToAddress)
	}
	if opts.Limit > 0 {
		query.Set("limit", fmt.Sprintf("%d", opts.Limit))
	}
	if opts.Offset > 0 {
		query.Set("offset", fmt.Sprintf("%d", opts.Offset))
	}
	if opts.IsRead != nil {
		query.Set("is_read", fmt.Sprintf("%t", *opts.IsRead))
	}
	if opts.JacsVerified != nil {
		query.Set("jacs_verified", fmt.Sprintf("%t", *opts.JacsVerified))
	}
	if opts.Folder != "" {
		query.Set("folder", opts.Folder)
	}
	if opts.Label != "" {
		query.Set("label", opts.Label)
	}
	if opts.HasAttachments != nil {
		query.Set("has_attachments", fmt.Sprintf("%t", *opts.HasAttachments))
	}
	if opts.Since != "" {
		query.Set("since", opts.Since)
	}
	if opts.Until != "" {
		query.Set("until", opts.Until)
	}
	path := fmt.Sprintf("/api/agents/%s/email/search?%s", neturl.PathEscape(c.HaiAgentID()), query.Encode())
	var wrapper ListMessagesResponse
	if err := c.doRequest(ctx, http.MethodGet, path, nil, &wrapper); err != nil {
		return nil, err
	}
	return wrapper.Messages, nil
}

// GetUnreadCount returns the number of unread messages in the agent's inbox.
func (c *Client) GetUnreadCount(ctx context.Context) (int, error) {
	path := fmt.Sprintf("/api/agents/%s/email/unread-count", neturl.PathEscape(c.HaiAgentID()))
	var result UnreadCountResult
	if err := c.doRequest(ctx, http.MethodGet, path, nil, &result); err != nil {
		return 0, err
	}
	return result.Count, nil
}

// Reply sends a reply to an existing message. If subjectOverride is empty,
// the original message's subject is fetched and prefixed with "Re: ".
func (c *Client) Reply(ctx context.Context, messageID, body, subjectOverride string) (*SendEmailResult, error) {
	original, err := c.GetMessage(ctx, messageID)
	if err != nil {
		return nil, err
	}

	subject := subjectOverride
	if subject == "" {
		subject = original.Subject
		if !strings.HasPrefix(strings.ToLower(subject), "re: ") {
			subject = "Re: " + subject
		}
	}

	// Use the RFC 5322 Message-ID from the original message for threading,
	// falling back to the database UUID if the original has no Message-ID.
	inReplyTo := messageID
	if original.MessageID != "" {
		inReplyTo = original.MessageID
	}

	return c.SendEmailWithOptions(ctx, SendEmailOptions{
		To:        original.FromAddress,
		Subject:   subject,
		Body:      body,
		InReplyTo: inReplyTo,
	})
}

// Forward forwards a message to another recipient.
func (c *Client) Forward(ctx context.Context, opts ForwardOptions) (*SendEmailResult, error) {
	path := fmt.Sprintf("/api/agents/%s/email/forward", neturl.PathEscape(c.HaiAgentID()))
	var result SendEmailResult
	if err := c.doRequest(ctx, http.MethodPost, path, opts, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// Archive moves a message to the archive folder.
func (c *Client) Archive(ctx context.Context, messageID string) error {
	path := fmt.Sprintf(
		"/api/agents/%s/email/messages/%s/archive",
		neturl.PathEscape(c.HaiAgentID()),
		neturl.PathEscape(messageID),
	)
	return c.doRequest(ctx, http.MethodPost, path, nil, nil)
}

// Unarchive restores a message from the archive back to the inbox.
func (c *Client) Unarchive(ctx context.Context, messageID string) error {
	path := fmt.Sprintf(
		"/api/agents/%s/email/messages/%s/unarchive",
		neturl.PathEscape(c.HaiAgentID()),
		neturl.PathEscape(messageID),
	)
	return c.doRequest(ctx, http.MethodPost, path, nil, nil)
}

// GetContacts retrieves the agent's contacts derived from email history.
// Handles both wrapped {"contacts": [...]} and bare array [...] responses.
func (c *Client) GetContacts(ctx context.Context) ([]Contact, error) {
	path := fmt.Sprintf("/api/agents/%s/email/contacts", neturl.PathEscape(c.HaiAgentID()))

	url := c.endpoint + path

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, wrapError(ErrConnection, err, "failed to create request")
	}
	if err := c.setAuthHeaders(req); err != nil {
		return nil, err
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, wrapError(ErrConnection, err, "request failed")
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		respBody, _ := limitedReadAll(resp.Body)
		return nil, classifyHTTPError(resp.StatusCode, respBody)
	}

	body, err := limitedReadAll(resp.Body)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to read response body")
	}

	// Try wrapped format first: {"contacts": [...]}
	var wrapper ContactsResponse
	if err := json.Unmarshal(body, &wrapper); err == nil && wrapper.Contacts != nil {
		return wrapper.Contacts, nil
	}

	// Fall back to bare array: [...]
	var contacts []Contact
	if err := json.Unmarshal(body, &contacts); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode contacts response")
	}
	return contacts, nil
}

// RegisterNewAgent generates a new Ed25519 keypair, creates a flat JACS agent
// document, signs it, and registers with HAI.
//
// The document structure matches the Rust API's expected format:
//
//	{
//	    "jacsId": "<uuid>",
//	    "jacsVersion": "1.0.0",
//	    "jacsAgentVersion": "1.0.0",
//	    "jacsAgentName": "<name>",
//	    "jacsPublicKey": "<PEM>",
//	    "jacsSignature": {"agentID": "<uuid>", "date": "<ISO8601>", "signature": "<base64>"},
//	    ...
//	}
func (c *Client) RegisterNewAgent(ctx context.Context, agentName string, opts *RegisterNewAgentOptions) (*RegisterResult, error) {
	// Generate keypair via CryptoBackend
	backend := c.crypto
	if backend == nil {
		backend = cryptoBackend
	}
	pubPEMBytes, privPEMBytes, err := backend.GenerateKeyPair()
	if err != nil {
		return nil, wrapError(ErrSigningFailed, err, "key generation failed")
	}

	// Parse the generated private key so we can create a signing backend for it
	priv, err := ParsePrivateKey(privPEMBytes)
	if err != nil {
		return nil, wrapError(ErrSigningFailed, err, "failed to parse generated private key")
	}
	pub := priv.Public().(ed25519.PublicKey)

	// Re-encode public key to canonical SPKI PEM (matches Rust verifier expectation)
	pubDER, err := x509.MarshalPKIXPublicKey(pub)
	if err != nil {
		return nil, wrapError(ErrSigningFailed, err, "failed to marshal public key")
	}
	pubPEMBytes = pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: pubDER})
	pubPEMStr := string(pubPEMBytes)

	jacsID := generateUUID()
	now := time.Now().UTC().Format(time.RFC3339)

	// Build flat JACS document with jacsSignature (minus .signature).
	description := "Agent registered via Go SDK"
	if opts != nil && opts.Description != "" {
		description = opts.Description
	}

	doc := map[string]interface{}{
		"jacsId":           jacsID,
		"jacsVersion":      "1.0.0",
		"jacsAgentVersion": "1.0.0",
		"jacsAgentName":    agentName,
		"jacsPublicKey":    pubPEMStr,
		"description":      description,
		"jacsSignature": map[string]interface{}{
			"agentID": jacsID,
			"date":    now,
		},
	}

	if opts != nil && opts.Domain != "" {
		doc["domain"] = opts.Domain
	}

	// Canonical JSON (Go encoding/json sorts map keys by default).
	canonical, err := json.Marshal(doc)
	if err != nil {
		return nil, wrapError(ErrSigningFailed, err, "failed to marshal agent document")
	}

	// Sign the canonical JSON via CryptoBackend
	newKeyBackend := newClientCryptoBackend(priv, jacsID)
	sig, signErr := newKeyBackend.SignBytes(canonical)
	if signErr != nil {
		return nil, wrapError(ErrSigningFailed, signErr, "failed to sign agent document")
	}
	sigB64 := base64.StdEncoding.EncodeToString(sig)

	// Insert signature into jacsSignature and re-serialize.
	doc["jacsSignature"].(map[string]interface{})["signature"] = sigB64

	agentJSON, err := json.Marshal(doc)
	if err != nil {
		return nil, wrapError(ErrSigningFailed, err, "failed to marshal signed agent document")
	}

	// Register with HAI (self-authenticated agent document).
	regOpts := RegisterOptions{
		AgentJSON: string(agentJSON),
		PublicKey: pubPEMStr,
	}
	if opts != nil {
		regOpts.OwnerEmail = opts.OwnerEmail
	}

	reg, err := c.registerWithoutAuth(ctx, regOpts)
	if err != nil {
		return nil, err
	}

	// Encode keys as PEM for local storage (PKCS#8 DER for private key).
	pkcs8Bytes, err := x509.MarshalPKCS8PrivateKey(priv)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal private key: %w", err)
	}
	privPEM := pem.EncodeToMemory(&pem.Block{
		Type:  "PRIVATE KEY",
		Bytes: pkcs8Bytes,
	})
	pubPEM := pem.EncodeToMemory(&pem.Block{
		Type:  "PUBLIC KEY",
		Bytes: pubDER,
	})

	// Print next-step messaging
	if opts == nil || !opts.Quiet {
		ownerEmail := ""
		if opts != nil {
			ownerEmail = opts.OwnerEmail
		}
		fmt.Println()
		fmt.Println("Agent created and submitted for registration!")
		fmt.Printf("  -> Check your email (%s) for a verification link\n", ownerEmail)
		fmt.Println("  -> Click the link and log into hai.ai to complete registration")
		fmt.Println("  -> After verification, claim a @hai.ai username with:")
		fmt.Println("     client.ClaimUsername(ctx, agentID, \"my-agent\")")
		fmt.Println("  -> Save your config and private key to a secure, access-controlled location")

		if opts != nil && opts.Domain != "" {
			hash := sha256.Sum256([]byte(pubPEMStr))
			fmt.Println()
			fmt.Println("--- DNS Setup Instructions ---")
			fmt.Printf("Add this TXT record to your domain '%s':\n", opts.Domain)
			fmt.Printf("  Name:  _jacs.%s\n", opts.Domain)
			fmt.Println("  Type:  TXT")
			fmt.Printf("  Value: sha256:%x\n", hash)
			fmt.Println("DNS verification enables the pro tier.")
		}
		fmt.Println()
	}

	return &RegisterResult{
		Registration: reg,
		PrivateKey:   privPEM,
		PublicKey:    pubPEM,
		AgentJSON:    string(agentJSON),
	}, nil
}

// RegisterNewAgentWithEndpoint bootstraps registration on a clean machine
// without requiring a local config or existing private key.
func RegisterNewAgentWithEndpoint(ctx context.Context, endpoint, agentName string, opts *RegisterNewAgentOptions) (*RegisterResult, error) {
	cl := &Client{
		endpoint: strings.TrimRight(endpoint, "/"),
		httpClient: &http.Client{
			Timeout: 30 * time.Second,
		},
	}
	return cl.RegisterNewAgent(ctx, agentName, opts)
}

// generateUUID produces a UUIDv4 string.
func generateUUID() string {
	var uuid [16]byte
	_, _ = rand.Read(uuid[:])
	uuid[6] = (uuid[6] & 0x0f) | 0x40 // version 4
	uuid[8] = (uuid[8] & 0x3f) | 0x80 // variant 2
	return fmt.Sprintf("%08x-%04x-%04x-%04x-%012x",
		uuid[0:4], uuid[4:6], uuid[6:8], uuid[8:10], uuid[10:16])
}

// SignBenchmarkResult signs a benchmark result as a JACS document for
// independent verification. The format matches the Python SDK's sign_response.
func (c *Client) SignBenchmarkResult(result map[string]interface{}) (*SignedDocument, error) {
	now := time.Now().UTC().Format(time.RFC3339)

	// Generate UUIDv4
	var uuid [16]byte
	if _, err := rand.Read(uuid[:]); err != nil {
		return nil, wrapError(ErrSigningFailed, err, "failed to generate UUID")
	}
	uuid[6] = (uuid[6] & 0x0f) | 0x40 // version 4
	uuid[8] = (uuid[8] & 0x3f) | 0x80 // variant 2
	documentID := fmt.Sprintf("%08x-%04x-%04x-%04x-%012x",
		uuid[0:4], uuid[4:6], uuid[6:8], uuid[8:10], uuid[10:16])

	// Canonical JSON of the data for signing and hashing.
	// Go's encoding/json sorts map keys by default.
	dataJSON, err := json.Marshal(result)
	if err != nil {
		return nil, wrapError(ErrSigningFailed, err, "failed to marshal benchmark result")
	}

	// SHA-256 hash of canonical data
	hashBytes := sha256.Sum256(dataJSON)
	hash := fmt.Sprintf("%x", hashBytes)

	// Sign the canonical data payload via CryptoBackend
	sig, signErr := c.crypto.SignBytes(dataJSON)
	if signErr != nil {
		return nil, wrapError(ErrSigningFailed, signErr, "failed to sign benchmark result")
	}
	sigB64 := base64.StdEncoding.EncodeToString(sig)

	doc := &SignedDocument{
		Version:      "1.0.0",
		DocumentType: "benchmark_result",
		Data:         result,
		Metadata: SignedDocumentMetadata{
			Issuer:     c.jacsID,
			DocumentID: documentID,
			CreatedAt:  now,
			Hash:       hash,
		},
		JacsSignature: JacsSignatureBlock{
			AgentID:   c.jacsID,
			Date:      now,
			Signature: sigB64,
		},
	}

	return doc, nil
}

// FetchRemoteKey fetches a public key from HAI's key distribution service.
func (c *Client) FetchRemoteKey(ctx context.Context, agentID, version string) (*PublicKeyInfo, error) {
	cacheKey := "remote:" + agentID + ":" + version
	if cached := c.agentKeys.get(cacheKey); cached != nil {
		return cached, nil
	}
	baseURL := os.Getenv("HAI_KEYS_BASE_URL")
	if baseURL == "" {
		baseURL = c.endpoint
	}
	result, err := FetchRemoteKeyFromURL(ctx, c.httpClient, baseURL, agentID, version)
	if err != nil {
		return nil, err
	}
	c.agentKeys.set(cacheKey, result)
	return result, nil
}

// FetchKeyByHash fetches a public key by its SHA-256 hash.
func (c *Client) FetchKeyByHash(ctx context.Context, publicKeyHash string) (*PublicKeyInfo, error) {
	cacheKey := "hash:" + publicKeyHash
	if cached := c.agentKeys.get(cacheKey); cached != nil {
		return cached, nil
	}
	baseURL := os.Getenv("HAI_KEYS_BASE_URL")
	if baseURL == "" {
		baseURL = c.endpoint
	}
	result, err := FetchKeyByHashFromURL(ctx, c.httpClient, baseURL, publicKeyHash)
	if err != nil {
		return nil, err
	}
	c.agentKeys.set(cacheKey, result)
	return result, nil
}

// ClearAgentKeyCache clears the agent key cache, forcing subsequent fetches to hit the API.
func (c *Client) ClearAgentKeyCache() {
	c.agentKeys.clear()
}

// FetchRemoteKeyFromURL fetches a public key from a specific key service URL.
func FetchRemoteKeyFromURL(ctx context.Context, httpClient *http.Client, baseURL, agentID, version string) (*PublicKeyInfo, error) {
	baseURL = strings.TrimRight(baseURL, "/")
	url := fmt.Sprintf(
		"%s/api/agents/keys/%s/%s",
		baseURL,
		neturl.PathEscape(agentID),
		neturl.PathEscape(version),
	)

	if httpClient == nil {
		httpClient = &http.Client{Timeout: 30 * time.Second}
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, wrapError(ErrConnection, err, "failed to create key request")
	}

	resp, err := httpClient.Do(req)
	if err != nil {
		return nil, wrapError(ErrConnection, err, "failed to fetch key")
	}
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusNotFound {
		return nil, newError(ErrKeyNotFound, "public key not found for agent '%s' version '%s'", agentID, version)
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		body, _ := limitedReadAll(resp.Body)
		return nil, newError(ErrConnection, "status %d: %s", resp.StatusCode, string(body))
	}

	var keyResp struct {
		JacsID          string `json:"jacs_id"`
		AgentID         string `json:"agent_id"`
		Version         string `json:"version"`
		PublicKey       string `json:"public_key"`
		PublicKeyB64    string `json:"public_key_b64"`
		PublicKeyRawB64 string `json:"public_key_raw_b64"`
		Algorithm       string `json:"algorithm"`
		PublicKeyHash   string `json:"public_key_hash"`
	}

	if err := json.NewDecoder(resp.Body).Decode(&keyResp); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode key response")
	}

	var publicKey []byte
	switch {
	case keyResp.PublicKeyRawB64 != "":
		publicKey, err = base64.StdEncoding.DecodeString(keyResp.PublicKeyRawB64)
		if err != nil {
			return nil, wrapError(ErrInvalidResponse, err, "invalid public_key_raw_b64 encoding")
		}
	case keyResp.PublicKeyB64 != "":
		publicKey, err = base64.StdEncoding.DecodeString(keyResp.PublicKeyB64)
		if err != nil {
			return nil, wrapError(ErrInvalidResponse, err, "invalid public_key_b64 encoding")
		}
	case strings.Contains(keyResp.PublicKey, "BEGIN PUBLIC KEY"):
		publicKey = []byte(keyResp.PublicKey)
	default:
		publicKey, err = base64.StdEncoding.DecodeString(keyResp.PublicKey)
		if err != nil {
			return nil, wrapError(ErrInvalidResponse, err, "invalid public key encoding")
		}
	}

	agentIDResp := keyResp.AgentID
	if agentIDResp == "" {
		agentIDResp = keyResp.JacsID
	}

	return &PublicKeyInfo{
		PublicKey:     publicKey,
		Algorithm:     keyResp.Algorithm,
		PublicKeyHash: keyResp.PublicKeyHash,
		AgentID:       agentIDResp,
		Version:       keyResp.Version,
	}, nil
}

// FetchKeyByEmail fetches an agent's public key by their @hai.ai email address.
func (c *Client) FetchKeyByEmail(ctx context.Context, email string) (*PublicKeyInfo, error) {
	cacheKey := "email:" + email
	if cached := c.agentKeys.get(cacheKey); cached != nil {
		return cached, nil
	}
	baseURL := os.Getenv("HAI_KEYS_BASE_URL")
	if baseURL == "" {
		baseURL = c.endpoint
	}
	result, err := FetchKeyByEmailFromURL(ctx, c.httpClient, baseURL, email)
	if err != nil {
		return nil, err
	}
	c.agentKeys.set(cacheKey, result)
	return result, nil
}

// FetchKeyByDomain fetches the latest DNS-verified agent key for a domain.
func (c *Client) FetchKeyByDomain(ctx context.Context, domain string) (*PublicKeyInfo, error) {
	cacheKey := "domain:" + domain
	if cached := c.agentKeys.get(cacheKey); cached != nil {
		return cached, nil
	}
	baseURL := os.Getenv("HAI_KEYS_BASE_URL")
	if baseURL == "" {
		baseURL = c.endpoint
	}
	result, err := FetchKeyByDomainFromURL(ctx, c.httpClient, baseURL, domain)
	if err != nil {
		return nil, err
	}
	c.agentKeys.set(cacheKey, result)
	return result, nil
}

// FetchAllKeys fetches all key versions for an agent.
func (c *Client) FetchAllKeys(ctx context.Context, jacsID string) (*AgentKeyHistory, error) {
	baseURL := os.Getenv("HAI_KEYS_BASE_URL")
	if baseURL == "" {
		baseURL = c.endpoint
	}
	return FetchAllKeysFromURL(ctx, c.httpClient, baseURL, jacsID)
}
