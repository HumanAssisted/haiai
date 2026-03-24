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
)

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
	crypto     CryptoBackend // signing/verification backend (JACS CGo) — used by native methods only
	httpClient *http.Client  // used by SSE/WS streaming and bootstrap methods only
	agentKeys  *keyCache     // Agent key cache with 5-minute TTL
	ffi        FFIClient     // Rust FFI client for all API calls
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
// Deprecated: Retry logic is now handled by the Rust FFI layer. This option is
// retained for API compatibility but has no effect on FFI-delegated calls.
func WithMaxRetries(n int) Option {
	return func(c *Client) {
		// No-op: retries are handled by the Rust FFI layer.
		// Kept for backward compatibility.
	}
}

// WithFFIClient injects a custom FFI client (used for testing).
func WithFFIClient(ffiClient FFIClient) Option {
	return func(c *Client) {
		c.ffi = ffiClient
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
		agentKeys: newKeyCache(),
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
	var configPath string
	if cl.jacsID == "" || cl.privateKey == nil {
		cfg, cfgPath, err := discoverConfigWithPath()
		if err != nil {
			return nil, err
		}
		configPath = cfgPath

		if cl.jacsID == "" {
			cl.jacsID = cfg.JacsID
		}

		if cl.privateKey == nil {
			keyPath := ResolveKeyPath(cfg, cfgPath)
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

	// Initialize crypto backend (JACS CGo agent) — used by native methods only
	cl.crypto = newClientCryptoBackend(cl.privateKey, cl.jacsID)

	// Initialize FFI client if not injected via WithFFIClient
	if cl.ffi == nil {
		ffiConfig := map[string]interface{}{
			"base_url": cl.endpoint,
			"jacs_id":  cl.jacsID,
		}
		if configPath != "" {
			ffiConfig["jacs_config_path"] = configPath
		}
		configJSON, err := json.Marshal(ffiConfig)
		if err != nil {
			return nil, wrapError(ErrConfigInvalid, err, "failed to build FFI config")
		}
		_ = configJSON
		// TODO: Enable FFI client creation once libhaiigo is available at link time.
		// For now, the FFI client is injected via WithFFIClient in tests.
		// ffiClient, err := ffi.NewClient(string(configJSON))
		// if err != nil {
		//     return nil, wrapError(ErrConfigInvalid, err, "failed to create FFI client")
		// }
		// cl.ffi = ffiClient
	}

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

// mapFFIErr converts an FFI error to the appropriate haiai Error type.
func mapFFIErr(err error) error {
	if err == nil {
		return nil
	}
	// Use the ffi package MapFFIError if possible; otherwise wrap generically.
	// Since we can't import ffi (circular dep with CGo), we pattern-match on
	// the error message for now.
	msg := err.Error()
	switch {
	case strings.Contains(msg, "auth") || strings.Contains(msg, "Auth"):
		return newError(ErrAuthRequired, msg)
	case strings.Contains(msg, "rate_limited") || strings.Contains(msg, "RateLimited"):
		return newError(ErrRateLimited, msg)
	case strings.Contains(msg, "not_found") || strings.Contains(msg, "NotFound"):
		return newError(ErrNotFound, msg)
	case strings.Contains(msg, "connection") || strings.Contains(msg, "NetworkFailed"):
		return newError(ErrConnection, msg)
	default:
		return newError(ErrInvalidResponse, msg)
	}
}

// buildAuthHeader constructs the JACS authentication header.
// Delegates to the FFI client when available, otherwise falls back to the CryptoBackend.
func (c *Client) buildAuthHeader() (string, error) {
	if c.ffi != nil {
		header, err := c.ffi.BuildAuthHeader()
		if err != nil {
			return "", wrapError(ErrSigningFailed, err, "failed to build JACS auth header via FFI")
		}
		return header, nil
	}
	if c.crypto == nil {
		return "", newError(ErrSigningFailed, "crypto backend is not initialized")
	}
	header, err := c.crypto.BuildAuthHeader()
	if err != nil {
		return "", wrapError(ErrSigningFailed, err, "failed to build JACS auth header")
	}
	return header, nil
}

// setAuthHeaders sets the JACS Authorization and Content-Type headers.
func (c *Client) setAuthHeaders(req *http.Request) error {
	header, err := c.buildAuthHeader()
	if err != nil {
		return err
	}
	req.Header.Set("Authorization", header)
	req.Header.Set("Content-Type", "application/json")
	return nil
}

// classifyHTTPError maps HTTP status codes to appropriate ErrorKind values.
// Retained for native methods (SSE/WS, bootstrap) that still use HTTP directly.
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

// limitedReadAll reads from r up to maxResponseSize bytes.
// Returns an error if the response body exceeds the limit.
// Retained for native methods that still use HTTP directly.
func limitedReadAll(r io.Reader) ([]byte, error) {
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

// =============================================================================
// API Methods — delegated to FFI
// =============================================================================

// Hello tests connectivity and authentication with HAI.
func (c *Client) Hello(ctx context.Context) (*HelloResult, error) {
	raw, err := c.ffi.Hello(false)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var result HelloResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode hello response")
	}
	return &result, nil
}

// TestConnection verifies connectivity to the HAI server using the FFI-backed
// hello() as a single authenticated health check.
func (c *Client) TestConnection(ctx context.Context) (bool, error) {
	_, err := c.ffi.Hello(false)
	if err != nil {
		return false, nil
	}
	return true, nil
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
	optsJSON, err := json.Marshal(wireOpts)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to marshal registration options")
	}
	raw, err := c.ffi.Register(string(optsJSON))
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var result RegistrationResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode registration response")
	}
	return &result, nil
}

// RotateKeys rotates the agent's cryptographic keys.
//
// This method is kept native (not delegated to FFI) because it involves
// local file operations, key generation, config file updates, and a multi-step
// registration flow that requires Go-specific logic.
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
// Retained for RegisterNewAgent bootstrap flow.
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
	raw, err := c.ffi.VerifyStatus(c.jacsID)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var result StatusResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode verify response")
	}
	if result.JacsID == "" {
		result.JacsID = c.jacsID
	}
	return &result, nil
}

// Benchmark runs a benchmark suite at the given tier.
func (c *Client) Benchmark(ctx context.Context, tier string) (*BenchmarkResult, error) {
	name := generateBenchmarkName(tier, c.jacsID)
	raw, err := c.ffi.Benchmark(name, tier)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var result BenchmarkResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode benchmark response")
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
	// Delegate to FFI for the full pro run flow.
	proOptsJSON := "{}"
	if opts != nil {
		proOpts := map[string]interface{}{}
		if opts.PollInterval > 0 {
			proOpts["poll_interval_secs"] = int(opts.PollInterval.Seconds())
		}
		if opts.PollTimeout > 0 {
			proOpts["poll_timeout_secs"] = int(opts.PollTimeout.Seconds())
		}
		data, err := json.Marshal(proOpts)
		if err == nil {
			proOptsJSON = string(data)
		}
	}
	raw, err := c.ffi.ProRun(proOptsJSON)
	if err != nil {
		// Check if this is a checkout URL error that we need to surface
		mapped := mapFFIErr(err)
		if opts != nil && opts.OnCheckoutURL != nil {
			// Try to extract checkout URL from error message
			errMsg := err.Error()
			if strings.Contains(errMsg, "checkout_url") {
				// Parse the error to get the checkout URL
				var errData struct {
					CheckoutURL string `json:"checkout_url"`
				}
				if json.Unmarshal([]byte(errMsg), &errData) == nil && errData.CheckoutURL != "" {
					opts.OnCheckoutURL(errData.CheckoutURL)
				}
			}
		}
		return nil, mapped
	}
	var result BenchmarkResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode pro run response")
	}
	return &result, nil
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
	params := map[string]interface{}{
		"job_id":   jobID,
		"response": response,
	}
	paramsJSON, err := json.Marshal(params)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to marshal submit response params")
	}
	raw, err := c.ffi.SubmitResponse(string(paramsJSON))
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var result JobResponseResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode submit response result")
	}
	return &result, nil
}

// signResponse wraps a response payload in a JACS document envelope and signs it.
//
// Delegates to CryptoBackend.SignResponse and fails closed if the backend
// cannot produce a signed envelope.
// Retained for local signing operations (e.g., SignBenchmarkResult).
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
	// No direct FFI equivalent; use VerifyStatus which encompasses attestation data.
	raw, err := c.ffi.VerifyStatus(c.jacsID)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var result AttestationResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode attestation response")
	}
	return &result, nil
}

// VerifyAgent verifies another agent's registration and identity with HAI.
func (c *Client) VerifyAgent(ctx context.Context, agentID string) (*VerifyResult, error) {
	raw, err := c.ffi.VerifyStatus(agentID)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var result VerifyResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode verify response")
	}
	return &result, nil
}

// VerifyDocument verifies a signed JACS document using HAI's public endpoint.
func (c *Client) VerifyDocument(ctx context.Context, document string) (*DocumentVerificationResult, error) {
	raw, err := c.ffi.VerifyDocument(document)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var result DocumentVerificationResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode verify response")
	}
	return &result, nil
}

// GetVerification gets advanced 3-level verification status for an agent.
func (c *Client) GetVerification(ctx context.Context, agentID string) (*AgentVerificationResult, error) {
	raw, err := c.ffi.GetVerification(agentID)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var result AgentVerificationResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode verification response")
	}
	return &result, nil
}

// VerifyAgentDocument verifies an agent document with HAI's advanced verifier.
func (c *Client) VerifyAgentDocument(ctx context.Context, request VerifyAgentDocumentRequest) (*AgentVerificationResult, error) {
	reqJSON, err := json.Marshal(request)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to marshal verify request")
	}
	raw, err := c.ffi.VerifyAgentDocument(string(reqJSON))
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var result AgentVerificationResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode verification response")
	}
	return &result, nil
}

// CheckUsername checks if a username is available for @hai.ai email.
func (c *Client) CheckUsername(ctx context.Context, username string) (*CheckUsernameResult, error) {
	raw, err := c.ffi.CheckUsername(username)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var result CheckUsernameResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode check username response")
	}
	return &result, nil
}

// ClaimUsername claims a username for an agent, getting {username}@hai.ai email.
func (c *Client) ClaimUsername(ctx context.Context, agentID string, username string) (*ClaimUsernameResult, error) {
	raw, err := c.ffi.ClaimUsername(agentID, username)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var result ClaimUsernameResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode claim username response")
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
func (c *Client) UpdateUsername(ctx context.Context, agentID string, username string) (*UpdateUsernameResult, error) {
	raw, err := c.ffi.UpdateUsername(agentID, username)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var result UpdateUsernameResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode update username response")
	}
	return &result, nil
}

// DeleteUsername releases an agent's claimed username.
func (c *Client) DeleteUsername(ctx context.Context, agentID string) (*DeleteUsernameResult, error) {
	raw, err := c.ffi.DeleteUsername(agentID)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var result DeleteUsernameResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode delete username response")
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

	optsJSON, err := json.Marshal(opts)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to marshal send email options")
	}
	raw, err := c.ffi.SendEmail(string(optsJSON))
	if err != nil {
		// Email errors may already be sentinel-typed by the FFI layer,
		// so pass them through directly rather than using mapFFIErr.
		return nil, err
	}
	var result SendEmailResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode send email response")
	}
	return &result, nil
}

// classifyEmailError attempts to parse a structured API error response with
// an error_code field and maps known codes to sentinel errors. Falls back to
// the generic classifyHTTPError for unstructured responses.
// Retained for native methods that still use HTTP directly.
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
//
// This method still uses HTTP directly because it sends raw bytes, not JSON.
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
// Use SendEmail or SendEmailWithOptions directly.
func (c *Client) SendSignedEmail(ctx context.Context, opts SendEmailOptions) (*SendEmailResult, error) {
	return c.SendEmailWithOptions(ctx, opts)
}

// VerifyEmail sends a raw RFC 5322 email to the HAI server for JACS signature verification.
// The server verifies the JACS attachment signature and returns a detailed result.
//
// This method still uses HTTP directly because it sends raw bytes, not JSON.
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
	optsJSON, err := json.Marshal(opts)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to marshal list messages options")
	}
	raw, err := c.ffi.ListMessages(string(optsJSON))
	if err != nil {
		return nil, mapFFIErr(err)
	}
	// Try the wrapped format first: {"messages": [...]}
	var wrapper ListMessagesResponse
	if err := json.Unmarshal(raw, &wrapper); err == nil && wrapper.Messages != nil {
		return wrapper.Messages, nil
	}
	// Fall back to bare array
	var messages []EmailMessage
	if err := json.Unmarshal(raw, &messages); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode list messages response")
	}
	return messages, nil
}

// MarkRead marks a message as read.
func (c *Client) MarkRead(ctx context.Context, messageID string) error {
	err := c.ffi.MarkRead(messageID)
	if err != nil {
		return mapFFIErr(err)
	}
	return nil
}

// GetEmailStatus retrieves the agent's email usage and limits.
func (c *Client) GetEmailStatus(ctx context.Context) (*EmailStatus, error) {
	raw, err := c.ffi.GetEmailStatus()
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var status EmailStatus
	if err := json.Unmarshal(raw, &status); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode email status response")
	}
	return &status, nil
}

// GetMessage retrieves a single email message by ID.
func (c *Client) GetMessage(ctx context.Context, messageID string) (*EmailMessage, error) {
	raw, err := c.ffi.GetMessage(messageID)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var msg EmailMessage
	if err := json.Unmarshal(raw, &msg); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode message response")
	}
	return &msg, nil
}

// DeleteMessage deletes an email message by ID.
func (c *Client) DeleteMessage(ctx context.Context, messageID string) error {
	err := c.ffi.DeleteMessage(messageID)
	if err != nil {
		return mapFFIErr(err)
	}
	return nil
}

// MarkUnread marks a message as unread.
func (c *Client) MarkUnread(ctx context.Context, messageID string) error {
	err := c.ffi.MarkUnread(messageID)
	if err != nil {
		return mapFFIErr(err)
	}
	return nil
}

// SearchMessages searches the agent's mailbox.
func (c *Client) SearchMessages(ctx context.Context, opts SearchOptions) ([]EmailMessage, error) {
	optsJSON, err := json.Marshal(opts)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to marshal search options")
	}
	raw, err := c.ffi.SearchMessages(string(optsJSON))
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var wrapper ListMessagesResponse
	if err := json.Unmarshal(raw, &wrapper); err == nil && wrapper.Messages != nil {
		return wrapper.Messages, nil
	}
	var messages []EmailMessage
	if err := json.Unmarshal(raw, &messages); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode search messages response")
	}
	return messages, nil
}

// GetUnreadCount returns the number of unread messages in the agent's inbox.
func (c *Client) GetUnreadCount(ctx context.Context) (int, error) {
	raw, err := c.ffi.GetUnreadCount()
	if err != nil {
		return 0, mapFFIErr(err)
	}
	var result UnreadCountResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return 0, wrapError(ErrInvalidResponse, err, "failed to decode unread count response")
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
	optsJSON, err := json.Marshal(opts)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to marshal forward options")
	}
	raw, err := c.ffi.Forward(string(optsJSON))
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var result SendEmailResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode forward response")
	}
	return &result, nil
}

// Archive moves a message to the archive folder.
func (c *Client) Archive(ctx context.Context, messageID string) error {
	err := c.ffi.Archive(messageID)
	if err != nil {
		return mapFFIErr(err)
	}
	return nil
}

// Unarchive restores a message from the archive back to the inbox.
func (c *Client) Unarchive(ctx context.Context, messageID string) error {
	err := c.ffi.Unarchive(messageID)
	if err != nil {
		return mapFFIErr(err)
	}
	return nil
}

// GetContacts retrieves the agent's contacts derived from email history.
func (c *Client) GetContacts(ctx context.Context) ([]Contact, error) {
	raw, err := c.ffi.Contacts()
	if err != nil {
		return nil, mapFFIErr(err)
	}

	// Try wrapped format first: {"contacts": [...]}
	var wrapper ContactsResponse
	if err := json.Unmarshal(raw, &wrapper); err == nil && wrapper.Contacts != nil {
		return wrapper.Contacts, nil
	}

	// Fall back to bare array: [...]
	var contacts []Contact
	if err := json.Unmarshal(raw, &contacts); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode contacts response")
	}
	return contacts, nil
}

// RegisterNewAgent generates a new Ed25519 keypair, creates a flat JACS agent
// document, signs it, and registers with HAI.
//
// This method is kept native because it involves keygen, local signing, and
// bootstrap registration without pre-existing auth.
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
	raw, err := c.ffi.FetchRemoteKey(agentID, version)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	result, err := parseKeyResponseJSON(raw)
	if err != nil {
		return nil, err
	}
	c.agentKeys.set(cacheKey, result)
	return result, nil
}

// parseKeyResponseJSON parses a key API response into PublicKeyInfo.
// Handles the various formats (PEM, raw_b64, b64, etc.) from the HAI API.
func parseKeyResponseJSON(raw json.RawMessage) (*PublicKeyInfo, error) {
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
	if err := json.Unmarshal(raw, &keyResp); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode key response")
	}

	var publicKey []byte
	var err error
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

// FetchKeyByHash fetches a public key by its SHA-256 hash.
func (c *Client) FetchKeyByHash(ctx context.Context, publicKeyHash string) (*PublicKeyInfo, error) {
	cacheKey := "hash:" + publicKeyHash
	if cached := c.agentKeys.get(cacheKey); cached != nil {
		return cached, nil
	}
	raw, err := c.ffi.FetchKeyByHash(publicKeyHash)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	result, err := parseKeyResponseJSON(raw)
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
// Retained as a standalone function for backward compatibility.
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
	raw, err := c.ffi.FetchKeyByEmail(email)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	result, err := parseKeyResponseJSON(raw)
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
	raw, err := c.ffi.FetchKeyByDomain(domain)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	result, err := parseKeyResponseJSON(raw)
	if err != nil {
		return nil, err
	}
	c.agentKeys.set(cacheKey, result)
	return result, nil
}

// FetchAllKeys fetches all key versions for an agent.
func (c *Client) FetchAllKeys(ctx context.Context, jacsID string) (*AgentKeyHistory, error) {
	raw, err := c.ffi.FetchAllKeys(jacsID)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	return parseKeyHistoryJSON(raw)
}

// parseKeyHistoryJSON parses a key history response from the FFI layer.
func parseKeyHistoryJSON(raw json.RawMessage) (*AgentKeyHistory, error) {
	var rawHist struct {
		JacsID string        `json:"jacs_id"`
		Keys   []rawKeyEntry `json:"keys"`
		Total  int           `json:"total"`
	}
	if err := json.Unmarshal(raw, &rawHist); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode key history response")
	}

	keys := make([]PublicKeyInfo, 0, len(rawHist.Keys))
	for _, k := range rawHist.Keys {
		publicKey, err := decodePublicKey(k.PublicKeyRawB64, k.PublicKey)
		if err != nil {
			return nil, wrapError(ErrInvalidResponse, err, "invalid public key encoding in key history")
		}
		agentID := k.AgentID
		if agentID == "" {
			agentID = k.JacsID
		}
		keys = append(keys, PublicKeyInfo{
			PublicKey:     publicKey,
			Algorithm:     k.Algorithm,
			PublicKeyHash: k.PublicKeyHash,
			AgentID:       agentID,
			Version:       k.Version,
		})
	}

	return &AgentKeyHistory{
		JacsID: rawHist.JacsID,
		Keys:   keys,
		Total:  rawHist.Total,
	}, nil
}
