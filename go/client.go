// Package haisdk provides the Go SDK for the HAI.AI agent benchmarking platform.
//
// All authentication uses JACS agent identity (Ed25519 signatures).
// There is no API key authentication path.
//
// Quick start:
//
//	client, err := haisdk.NewClient()
//	if err != nil {
//	    log.Fatal(err)
//	}
//
//	result, err := client.Hello(ctx)
package haisdk

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"fmt"
	"io"
	"net/http"
	"os"
	"strings"
	"time"
)

const (
	// DefaultEndpoint is the default HAI API endpoint.
	DefaultEndpoint = "https://api.hai.ai"

	// DefaultKeysEndpoint is the default HAI key distribution service.
	DefaultKeysEndpoint = "https://keys.hai.ai"
)

// Client is the HAI SDK client. It authenticates using JACS agent identity.
type Client struct {
	endpoint   string
	jacsID     string
	privateKey ed25519.PrivateKey
	httpClient *http.Client
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
		cfg, err := DiscoverConfig()
		if err != nil {
			return nil, err
		}

		if cl.jacsID == "" {
			cl.jacsID = cfg.JacsID
		}

		if cl.privateKey == nil {
			// Determine config path to resolve relative key path
			configPath := os.Getenv("JACS_CONFIG_PATH")
			if configPath == "" {
				configPath = "jacs.config.json"
			}
			keyPath := ResolveKeyPath(cfg, configPath)
			key, err := LoadPrivateKey(keyPath)
			if err != nil {
				return nil, err
			}
			cl.privateKey = key
		}
	}

	if cl.jacsID == "" {
		return nil, newError(ErrConfigInvalid, "jacsId is empty in config")
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

// doRequest performs an authenticated HTTP request and decodes the JSON response.
func (c *Client) doRequest(ctx context.Context, method, path string, body interface{}, result interface{}) error {
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

	SetAuthHeaders(req, c.jacsID, c.privateKey)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return wrapError(ErrConnection, err, "request failed")
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		respBody, _ := io.ReadAll(resp.Body)
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
	var result HelloResult
	if err := c.doRequest(ctx, http.MethodGet, "/api/v1/hello", nil, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// TestConnection verifies connectivity to the HAI server (unauthenticated).
func (c *Client) TestConnection(ctx context.Context) (bool, error) {
	url := c.endpoint + "/health"

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return false, wrapError(ErrConnection, err, "failed to create request")
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return false, wrapError(ErrConnection, err, "connection failed")
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return false, newError(ErrConnection, "server returned status: %d", resp.StatusCode)
	}

	var health struct {
		Status string `json:"status"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&health); err != nil {
		return true, nil
	}

	return health.Status == "ok" || health.Status == "healthy", nil
}

// Register registers the agent with HAI.
func (c *Client) Register(ctx context.Context, agentJSON string) (*RegistrationResult, error) {
	reqBody := struct {
		AgentJSON string `json:"agent_json"`
	}{
		AgentJSON: agentJSON,
	}

	var result RegistrationResult
	if err := c.doRequest(ctx, http.MethodPost, "/api/v1/agents/register", reqBody, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// Status checks the registration/verification status of this agent with HAI.
// Calls GET /api/v1/agents/{jacs_id}/verify.
func (c *Client) Status(ctx context.Context) (*StatusResult, error) {
	path := fmt.Sprintf("/api/v1/agents/%s/verify", c.jacsID)

	url := c.endpoint + path
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, wrapError(ErrConnection, err, "failed to create request")
	}
	SetAuthHeaders(req, c.jacsID, c.privateKey)

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
		body, _ := io.ReadAll(resp.Body)
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
		Name string `json:"name"`
		Tier string `json:"tier"`
	}{
		Name: tier,
		Tier: tier,
	}

	var result BenchmarkResult
	if err := c.doRequest(ctx, http.MethodPost, "/api/benchmark/run", reqBody, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// FreeChaoticRun runs the free chaotic benchmark tier.
func (c *Client) FreeChaoticRun(ctx context.Context) (*BenchmarkResult, error) {
	return c.Benchmark(ctx, "free_chaotic")
}

// BaselineRun runs the baseline benchmark tier.
func (c *Client) BaselineRun(ctx context.Context) (*BenchmarkResult, error) {
	return c.Benchmark(ctx, "baseline")
}

// SubmitResponse submits a moderation response for a benchmark job.
func (c *Client) SubmitResponse(ctx context.Context, jobID string, response ModerationResponse) (*JobResponseResult, error) {
	reqBody := struct {
		Response ModerationResponse `json:"response"`
	}{
		Response: response,
	}

	path := fmt.Sprintf("/api/v1/agents/jobs/%s/response", jobID)
	var result JobResponseResult
	if err := c.doRequest(ctx, http.MethodPost, path, reqBody, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// GetAgentAttestation gets the agent's attestation from HAI.
func (c *Client) GetAgentAttestation(ctx context.Context) (*AttestationResult, error) {
	path := fmt.Sprintf("/api/v1/agents/%s/attestation", c.jacsID)
	var result AttestationResult
	if err := c.doRequest(ctx, http.MethodGet, path, nil, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// VerifyAgent verifies another agent's registration and identity with HAI.
func (c *Client) VerifyAgent(ctx context.Context, agentID string) (*VerifyResult, error) {
	path := fmt.Sprintf("/api/v1/agents/%s/verify", agentID)
	var result VerifyResult
	if err := c.doRequest(ctx, http.MethodGet, path, nil, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// CheckUsername checks if a username is available for @hai.ai email.
// Calls GET /api/v1/agents/username/check?username={name}.
func (c *Client) CheckUsername(ctx context.Context, username string) (*CheckUsernameResult, error) {
	path := fmt.Sprintf("/api/v1/agents/username/check?username=%s", username)
	var result CheckUsernameResult
	if err := c.doRequest(ctx, http.MethodGet, path, nil, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// ClaimUsername claims a username for an agent, getting {username}@hai.ai email.
// Calls POST /api/v1/agents/{agentID}/username.
func (c *Client) ClaimUsername(ctx context.Context, agentID string, username string) (*ClaimUsernameResult, error) {
	path := fmt.Sprintf("/api/v1/agents/%s/username", agentID)
	reqBody := struct {
		Username string `json:"username"`
	}{
		Username: username,
	}
	var result ClaimUsernameResult
	if err := c.doRequest(ctx, http.MethodPost, path, reqBody, &result); err != nil {
		return nil, err
	}
	return &result, nil
}

// RegisterNewAgent generates a new Ed25519 keypair, creates a JACS agent document,
// signs it, and registers with HAI.
func (c *Client) RegisterNewAgent(ctx context.Context, agentName string, opts *RegisterNewAgentOptions) (*RegisterResult, error) {
	pub, priv, err := GenerateKeyPair()
	if err != nil {
		return nil, err
	}

	pubB64 := base64.StdEncoding.EncodeToString(pub)
	now := time.Now().UTC().Format(time.RFC3339)

	agent := map[string]interface{}{
		"jacsAgentName":    agentName,
		"jacsAgentVersion": "1.0",
		"publicKey":        pubB64,
		"algorithm":        "Ed25519",
		"createdAt":        now,
	}

	if opts != nil && opts.Domain != "" {
		agent["domain"] = opts.Domain
	}

	agentJSON, err := json.Marshal(agent)
	if err != nil {
		return nil, wrapError(ErrSigningFailed, err, "failed to marshal agent document")
	}

	// Sign the agent document
	sig := Sign(priv, agentJSON)
	sigB64 := base64.StdEncoding.EncodeToString(sig)

	signedAgent := map[string]interface{}{
		"agent":     json.RawMessage(agentJSON),
		"signature": sigB64,
		"algorithm": "Ed25519",
	}

	signedJSON, err := json.Marshal(signedAgent)
	if err != nil {
		return nil, wrapError(ErrSigningFailed, err, "failed to marshal signed agent")
	}

	// Register with HAI using a temporary client with the new key
	tempClient := &Client{
		endpoint:   c.endpoint,
		jacsID:     agentName,
		privateKey: priv,
		httpClient: c.httpClient,
	}

	reg, err := tempClient.Register(ctx, string(signedJSON))
	if err != nil {
		return nil, err
	}

	// Encode keys as PEM
	privPEM := pem.EncodeToMemory(&pem.Block{
		Type:  "PRIVATE KEY",
		Bytes: priv.Seed(),
	})
	pubPEM := pem.EncodeToMemory(&pem.Block{
		Type:  "PUBLIC KEY",
		Bytes: pub,
	})

	return &RegisterResult{
		Registration: reg,
		PrivateKey:   privPEM,
		PublicKey:    pubPEM,
		AgentJSON:    string(signedJSON),
	}, nil
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

	// Sign the canonical data payload
	sig := Sign(c.privateKey, dataJSON)
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
	baseURL := os.Getenv("HAI_KEYS_BASE_URL")
	if baseURL == "" {
		baseURL = DefaultKeysEndpoint
	}
	return FetchRemoteKeyFromURL(ctx, c.httpClient, baseURL, agentID, version)
}

// FetchRemoteKeyFromURL fetches a public key from a specific key service URL.
func FetchRemoteKeyFromURL(ctx context.Context, httpClient *http.Client, baseURL, agentID, version string) (*PublicKeyInfo, error) {
	baseURL = strings.TrimRight(baseURL, "/")
	url := fmt.Sprintf("%s/jacs/v1/agents/%s/keys/%s", baseURL, agentID, version)

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
		body, _ := io.ReadAll(resp.Body)
		return nil, newError(ErrConnection, "status %d: %s", resp.StatusCode, string(body))
	}

	var keyResp struct {
		PublicKey     string `json:"public_key"`
		Algorithm     string `json:"algorithm"`
		PublicKeyHash string `json:"public_key_hash"`
		AgentID       string `json:"agent_id"`
		Version       string `json:"version"`
	}

	if err := json.NewDecoder(resp.Body).Decode(&keyResp); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode key response")
	}

	publicKey, err := base64.StdEncoding.DecodeString(keyResp.PublicKey)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "invalid public key encoding")
	}

	return &PublicKeyInfo{
		PublicKey:     publicKey,
		Algorithm:     keyResp.Algorithm,
		PublicKeyHash: keyResp.PublicKeyHash,
		AgentID:       keyResp.AgentID,
		Version:       keyResp.Version,
	}, nil
}
