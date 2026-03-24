package haiai

import (
	"context"
	"fmt"
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
// Delegates to the Client's FFI-backed method.
func FetchAllKeys(ctx context.Context, client *Client, jacsID string) (*AgentKeyHistory, error) {
	if client != nil {
		return client.FetchAllKeys(ctx, jacsID)
	}
	return nil, fmt.Errorf("haiai: Client required for FetchAllKeys (no native HTTP fallback)")
}

// FetchAllKeysFromURL is deprecated. Use Client.FetchAllKeys instead.
//
// Deprecated: Use Client.FetchAllKeys instead.
func FetchAllKeysFromURL(ctx context.Context, _ interface{}, baseURL, jacsID string) (*AgentKeyHistory, error) {
	return nil, fmt.Errorf("haiai: FetchAllKeysFromURL is deprecated; use Client.FetchAllKeys instead")
}
