package haiai

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"os"
	"strings"
	"time"
)

// FetchKeyByDomain fetches the latest DNS-verified agent key for a domain.
// The base URL is read from HAI_KEYS_BASE_URL env, defaulting to DefaultKeysEndpoint.
func FetchKeyByDomain(ctx context.Context, httpClient *http.Client, domain string) (*PublicKeyInfo, error) {
	baseURL := os.Getenv("HAI_KEYS_BASE_URL")
	if baseURL == "" {
		baseURL = DefaultEndpoint
	}
	return FetchKeyByDomainFromURL(ctx, httpClient, baseURL, domain)
}

// FetchKeyByDomainFromURL fetches the latest DNS-verified agent key for a domain from a specific URL.
func FetchKeyByDomainFromURL(ctx context.Context, httpClient *http.Client, baseURL, domain string) (*PublicKeyInfo, error) {
	baseURL = strings.TrimRight(baseURL, "/")
	apiURL := fmt.Sprintf("%s/api/agents/keys/domain/%s", baseURL, url.PathEscape(domain))

	if httpClient == nil {
		httpClient = &http.Client{Timeout: 30 * time.Second}
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, apiURL, nil)
	if err != nil {
		return nil, wrapError(ErrConnection, err, "failed to create key-by-domain request")
	}

	resp, err := httpClient.Do(req)
	if err != nil {
		return nil, wrapError(ErrConnection, err, "failed to fetch key by domain")
	}
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusNotFound {
		return nil, newError(ErrKeyNotFound, "no verified agent for domain '%s'", domain)
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
		PublicKeyRawB64 string `json:"public_key_raw_b64"`
		Algorithm       string `json:"algorithm"`
		PublicKeyHash   string `json:"public_key_hash"`
	}

	if err := json.NewDecoder(resp.Body).Decode(&keyResp); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode key response")
	}

	publicKey, err := decodePublicKey(keyResp.PublicKeyRawB64, keyResp.PublicKey)
	if err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "invalid public key encoding")
	}

	agentID := keyResp.AgentID
	if agentID == "" {
		agentID = keyResp.JacsID
	}

	return &PublicKeyInfo{
		PublicKey:     publicKey,
		Algorithm:     keyResp.Algorithm,
		PublicKeyHash: keyResp.PublicKeyHash,
		AgentID:       agentID,
		Version:       keyResp.Version,
	}, nil
}
