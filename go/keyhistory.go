package haiai

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"strings"
	"time"
)

// rawKeyEntry is an intermediate struct for JSON deserialization of key entries
// within the key history response. PublicKeyInfo.PublicKey is []byte, so Go's
// JSON decoder would try base64-decode on a PEM string and fail. This struct
// keeps everything as strings and we convert manually.
type rawKeyEntry struct {
	JacsID          string `json:"jacs_id"`
	AgentID         string `json:"agent_id"`
	Version         string `json:"version"`
	PublicKey       string `json:"public_key"`
	PublicKeyRawB64 string `json:"public_key_raw_b64"`
	Algorithm       string `json:"algorithm"`
	PublicKeyHash   string `json:"public_key_hash"`
}

// AgentKeyHistory contains all key versions for an agent.
type AgentKeyHistory struct {
	JacsID string          `json:"jacs_id"`
	Keys   []PublicKeyInfo `json:"keys"`
	Total  int             `json:"total"`
}

// FetchAllKeys fetches all key versions for an agent.
// The base URL is read from HAI_KEYS_BASE_URL env, defaulting to DefaultKeysEndpoint.
func FetchAllKeys(ctx context.Context, httpClient *http.Client, jacsID string) (*AgentKeyHistory, error) {
	baseURL := os.Getenv("HAI_KEYS_BASE_URL")
	if baseURL == "" {
		baseURL = DefaultKeysEndpoint
	}
	return FetchAllKeysFromURL(ctx, httpClient, baseURL, jacsID)
}

// FetchAllKeysFromURL fetches all key versions for an agent from a specific URL.
func FetchAllKeysFromURL(ctx context.Context, httpClient *http.Client, baseURL, jacsID string) (*AgentKeyHistory, error) {
	baseURL = strings.TrimRight(baseURL, "/")
	apiURL := fmt.Sprintf("%s/jacs/v1/agents/%s/keys", baseURL, url.PathEscape(jacsID))

	if httpClient == nil {
		httpClient = &http.Client{Timeout: 30 * time.Second}
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, apiURL, nil)
	if err != nil {
		return nil, wrapError(ErrConnection, err, "failed to create fetch-all-keys request")
	}

	resp, err := httpClient.Do(req)
	if err != nil {
		return nil, wrapError(ErrConnection, err, "failed to fetch all keys")
	}
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusNotFound {
		return nil, newError(ErrKeyNotFound, "agent not found: '%s'", jacsID)
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		body, _ := io.ReadAll(resp.Body)
		return nil, newError(ErrConnection, "status %d: %s", resp.StatusCode, string(body))
	}

	var raw struct {
		JacsID string        `json:"jacs_id"`
		Keys   []rawKeyEntry `json:"keys"`
		Total  int           `json:"total"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&raw); err != nil {
		return nil, wrapError(ErrInvalidResponse, err, "failed to decode key history response")
	}

	keys := make([]PublicKeyInfo, 0, len(raw.Keys))
	for _, k := range raw.Keys {
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
		JacsID: raw.JacsID,
		Keys:   keys,
		Total:  raw.Total,
	}, nil
}
