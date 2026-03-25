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
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
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
	crypto     CryptoBackend // signing/verification backend (JACS CGo) — used by local signing only
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
// Deprecated: All HTTP calls are now handled by the Rust FFI layer.
// This option is retained for API compatibility but has no effect.
func WithHTTPClient(httpClient interface{}) Option {
	return func(c *Client) {
		// No-op: HTTP is handled by Rust FFI layer.
	}
}

// WithTimeout sets the HTTP client timeout.
// Deprecated: Timeout is now configured in the Rust FFI layer.
// This option is retained for API compatibility but has no effect.
func WithTimeout(timeout time.Duration) Option {
	return func(c *Client) {
		// No-op: timeout is handled by Rust FFI layer.
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
		endpoint:  DefaultEndpoint,
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

// classifyHTTPError maps HTTP status codes to appropriate ErrorKind values.
// Retained for native methods (SSE/WS, bootstrap) that still use HTTP directly.
func classifyHTTPError(statusCode int, body []byte) *Error {
	msg := fmt.Sprintf("status %d: %s", statusCode, string(body))
	switch statusCode {
	case 401: // Unauthorized
		return newError(ErrAuthRequired, msg)
	case 403: // Forbidden
		return newError(ErrForbidden, msg)
	case 404: // Not Found
		return newError(ErrNotFound, msg)
	case 429: // Too Many Requests
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

// RotateKeys rotates the agent's cryptographic keys via FFI.
//
// Delegates key generation, document signing, file operations, and HAI
// re-registration to the Rust FFI layer. After successful rotation, reloads
// the new private key from disk to keep Go-side crypto state in sync.
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

	// Build FFI options JSON
	ffiOpts := map[string]interface{}{
		"register_with_hai": registerWithHai,
	}
	optionsJSON, err := json.Marshal(ffiOpts)
	if err != nil {
		return nil, wrapError(ErrSigningFailed, err, "failed to build rotate_keys options")
	}

	// Delegate to Rust FFI -- handles keygen, signing, file ops, and HAI registration
	raw, err := c.ffi.RotateKeys(string(optionsJSON))
	if err != nil {
		return nil, mapFFIErr(err)
	}

	var result RotationResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode rotation result")
	}

	// Reload new private key from disk to keep Go-side crypto state in sync.
	// The Rust side wrote the new key; we need it for local signing operations
	// (e.g., SignBenchmarkResult).
	if configPath == "" {
		_, cfgPath, discoverErr := discoverConfigWithPath()
		if discoverErr == nil {
			configPath = cfgPath
		}
	}
	if configPath != "" {
		cfg, loadErr := LoadConfig(configPath)
		if loadErr == nil {
			keyPath := ResolveKeyPath(cfg, configPath)
			password, pwErr := ResolvePrivateKeyPassword()
			if pwErr == nil {
				newKey, keyErr := LoadPrivateKey(keyPath, password)
				if keyErr == nil {
					c.privateKey = newKey
					c.crypto = newClientCryptoBackend(newKey, c.jacsID)
				}
			}
		}
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
func (c *Client) SignEmail(ctx context.Context, rawEmail []byte) ([]byte, error) {
	b64 := base64.StdEncoding.EncodeToString(rawEmail)
	raw, err := c.ffi.SignEmailRaw(b64)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	// raw is a JSON string containing the base64 result
	var b64Result string
	if err := json.Unmarshal(raw, &b64Result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode sign email response")
	}
	return base64.StdEncoding.DecodeString(b64Result)
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
func (c *Client) VerifyEmail(ctx context.Context, rawEmail []byte) (*EmailVerificationResultV2, error) {
	b64 := base64.StdEncoding.EncodeToString(rawEmail)
	raw, err := c.ffi.VerifyEmailRaw(b64)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var result EmailVerificationResultV2
	if err := json.Unmarshal(raw, &result); err != nil {
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

// RegisterNewAgent creates a new JACS agent and registers it with HAI.
// Keys, agent document, and registration are all handled by the Rust FFI layer.
func (c *Client) RegisterNewAgent(ctx context.Context, agentName string, opts *RegisterNewAgentOptions) (*RegisterResult, error) {
	options := map[string]interface{}{
		"agent_name": agentName,
	}
	if c.endpoint != "" {
		options["base_url"] = c.endpoint
	}
	if opts != nil {
		if opts.OwnerEmail != "" {
			options["owner_email"] = opts.OwnerEmail
		}
		if opts.Password != "" {
			options["password"] = opts.Password
		} else {
			if pw := os.Getenv("JACS_PRIVATE_KEY_PASSWORD"); pw != "" {
				options["password"] = pw
			} else if pwFile := os.Getenv("JACS_PASSWORD_FILE"); pwFile != "" {
				if data, err := os.ReadFile(pwFile); err == nil {
					options["password"] = strings.TrimSpace(string(data))
				}
			}
		}
		if opts.Domain != "" {
			options["domain"] = opts.Domain
		}
		if opts.Description != "" {
			options["description"] = opts.Description
		}
		if opts.KeyDirectory != "" {
			options["key_directory"] = opts.KeyDirectory
		}
		if opts.DataDirectory != "" {
			options["data_directory"] = opts.DataDirectory
		}
		if opts.ConfigPath != "" {
			options["config_path"] = opts.ConfigPath
		}
		if opts.Algorithm != "" {
			options["algorithm"] = opts.Algorithm
		}
	}

	optsJSON, err := json.Marshal(options)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to marshal register options")
	}

	raw, err := c.ffi.RegisterNewAgent(string(optsJSON))
	if err != nil {
		return nil, mapFFIErr(err)
	}

	var result RegisterResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode register result")
	}
	return &result, nil
}

// RegisterNewAgentWithEndpoint bootstraps registration on a clean machine
// without requiring a local config or existing private key.
// The endpoint is passed through to the Rust FFI layer as base_url.
func RegisterNewAgentWithEndpoint(ctx context.Context, endpoint, agentName string, opts *RegisterNewAgentOptions) (*RegisterResult, error) {
	cl := &Client{
		endpoint: strings.TrimRight(endpoint, "/"),
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
// Deprecated: Use Client.FetchRemoteKey instead. This function always returns an error.
func FetchRemoteKeyFromURL(ctx context.Context, httpClient interface{}, baseURL, agentID, version string) (*PublicKeyInfo, error) {
	return nil, newError(ErrConnection, "FetchRemoteKeyFromURL is deprecated: use Client.FetchRemoteKey instead")
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

// CreateAttestation creates a signed attestation document for a registered agent.
func (c *Client) CreateAttestation(ctx context.Context, agentID string, subject, claims interface{}, evidence interface{}) (json.RawMessage, error) {
	params := map[string]interface{}{
		"agent_id": agentID,
		"subject":  subject,
		"claims":   claims,
		"evidence": evidence,
	}
	if evidence == nil {
		params["evidence"] = []interface{}{}
	}
	paramsJSON, err := json.Marshal(params)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to marshal create attestation params")
	}
	raw, err := c.ffi.CreateAttestation(string(paramsJSON))
	if err != nil {
		return nil, mapFFIErr(err)
	}
	return raw, nil
}

// ListAttestations lists attestations for a registered agent.
func (c *Client) ListAttestations(ctx context.Context, agentID string, limit, offset int) (json.RawMessage, error) {
	params := map[string]interface{}{
		"agent_id": agentID,
		"limit":    limit,
		"offset":   offset,
	}
	paramsJSON, err := json.Marshal(params)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to marshal list attestations params")
	}
	raw, err := c.ffi.ListAttestations(string(paramsJSON))
	if err != nil {
		return nil, mapFFIErr(err)
	}
	return raw, nil
}

// GetAttestation retrieves a specific attestation document.
func (c *Client) GetAttestation(ctx context.Context, agentID, docID string) (json.RawMessage, error) {
	raw, err := c.ffi.GetAttestation(agentID, docID)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	return raw, nil
}

// VerifyAttestation verifies an attestation document via HAI.
func (c *Client) VerifyAttestation(ctx context.Context, document string) (json.RawMessage, error) {
	raw, err := c.ffi.VerifyAttestation(document)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	return raw, nil
}

// CreateEmailTemplate creates a new email template.
func (c *Client) CreateEmailTemplate(ctx context.Context, opts CreateEmailTemplateOptions) (*EmailTemplate, error) {
	optsJSON, err := json.Marshal(opts)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to marshal create email template options")
	}
	raw, err := c.ffi.CreateEmailTemplate(string(optsJSON))
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var tmpl EmailTemplate
	if err := json.Unmarshal(raw, &tmpl); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode email template response")
	}
	return &tmpl, nil
}

// ListEmailTemplates lists or searches email templates.
func (c *Client) ListEmailTemplates(ctx context.Context, opts ListEmailTemplatesOptions) (*ListEmailTemplatesResult, error) {
	optsJSON, err := json.Marshal(opts)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to marshal list email templates options")
	}
	raw, err := c.ffi.ListEmailTemplates(string(optsJSON))
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var result ListEmailTemplatesResult
	if err := json.Unmarshal(raw, &result); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode list email templates response")
	}
	return &result, nil
}

// GetEmailTemplate retrieves a single email template by ID.
func (c *Client) GetEmailTemplate(ctx context.Context, templateID string) (*EmailTemplate, error) {
	raw, err := c.ffi.GetEmailTemplate(templateID)
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var tmpl EmailTemplate
	if err := json.Unmarshal(raw, &tmpl); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode email template response")
	}
	return &tmpl, nil
}

// UpdateEmailTemplate updates an email template.
func (c *Client) UpdateEmailTemplate(ctx context.Context, templateID string, opts UpdateEmailTemplateOptions) (*EmailTemplate, error) {
	optsJSON, err := json.Marshal(opts)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to marshal update email template options")
	}
	raw, err := c.ffi.UpdateEmailTemplate(templateID, string(optsJSON))
	if err != nil {
		return nil, mapFFIErr(err)
	}
	var tmpl EmailTemplate
	if err := json.Unmarshal(raw, &tmpl); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode email template response")
	}
	return &tmpl, nil
}

// DeleteEmailTemplate deletes an email template (soft delete).
func (c *Client) DeleteEmailTemplate(ctx context.Context, templateID string) error {
	_, err := c.ffi.DeleteEmailTemplate(templateID)
	if err != nil {
		return mapFFIErr(err)
	}
	return nil
}
