package haiai

import (
	"context"
	"fmt"
)

// FetchKeyByEmail fetches a public key by the agent's @hai.ai email address.
// Delegates to the Client's FFI-backed method.
func FetchKeyByEmail(ctx context.Context, client *Client, email string) (*PublicKeyInfo, error) {
	if client != nil {
		return client.FetchKeyByEmail(ctx, email)
	}
	return nil, fmt.Errorf("haiai: Client required for FetchKeyByEmail (no native HTTP fallback)")
}

// FetchKeyByEmailFromURL is deprecated. Use Client.FetchKeyByEmail instead.
//
// Deprecated: Use Client.FetchKeyByEmail instead.
func FetchKeyByEmailFromURL(ctx context.Context, _ interface{}, baseURL, email string) (*PublicKeyInfo, error) {
	return nil, fmt.Errorf("haiai: FetchKeyByEmailFromURL is deprecated; use Client.FetchKeyByEmail instead")
}
