package haiai

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"strings"
	"time"
)

// FetchKeyByHash fetches a public key by its hash from the HAI key distribution service.
// If a Client is provided, delegates to its FFI-backed method.
// Otherwise falls back to direct HTTP.
func FetchKeyByHash(ctx context.Context, client *Client, publicKeyHash string) (*PublicKeyInfo, error) {
	if client != nil {
		return client.FetchKeyByHash(ctx, publicKeyHash)
	}
	baseURL := os.Getenv("HAI_KEYS_BASE_URL")
	if baseURL == "" {
		baseURL = DefaultEndpoint
	}
	return fetchKeyByHashHTTP(ctx, baseURL, publicKeyHash)
}

// FetchKeyByHashFromURL fetches a public key by its hash from a specific URL.
// Deprecated: Use Client.FetchKeyByHash instead.
func FetchKeyByHashFromURL(ctx context.Context, httpClient *http.Client, baseURL, publicKeyHash string) (*PublicKeyInfo, error) {
	return fetchKeyByHashHTTP(ctx, baseURL, publicKeyHash)
}

// fetchKeyByHashHTTP is the direct HTTP implementation (no FFI).
func fetchKeyByHashHTTP(ctx context.Context, baseURL, publicKeyHash string) (*PublicKeyInfo, error) {
	baseURL = strings.TrimRight(baseURL, "/")
	url := fmt.Sprintf("%s/api/agents/keys/hash/%s", baseURL, publicKeyHash)

	httpClient := &http.Client{Timeout: 30 * time.Second}

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, wrapError(ErrConnection, err, "failed to create key-by-hash request")
	}

	resp, err := httpClient.Do(req)
	if err != nil {
		return nil, wrapError(ErrConnection, err, "failed to fetch key by hash")
	}
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusNotFound {
		return nil, newError(ErrKeyNotFound, "public key not found for hash '%s'", publicKeyHash)
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		body, _ := limitedReadAll(resp.Body)
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
