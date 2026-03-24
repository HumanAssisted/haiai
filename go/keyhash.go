package haiai

import (
	"context"
	"fmt"
)

// FetchKeyByHash fetches a public key by its hash from the HAI key distribution service.
// Delegates to the Client's FFI-backed method.
func FetchKeyByHash(ctx context.Context, client *Client, publicKeyHash string) (*PublicKeyInfo, error) {
	if client != nil {
		return client.FetchKeyByHash(ctx, publicKeyHash)
	}
	return nil, fmt.Errorf("haiai: Client required for FetchKeyByHash (no native HTTP fallback)")
}

// FetchKeyByHashFromURL is deprecated. Use Client.FetchKeyByHash instead.
//
// Deprecated: Use Client.FetchKeyByHash instead.
func FetchKeyByHashFromURL(ctx context.Context, _ interface{}, baseURL, publicKeyHash string) (*PublicKeyInfo, error) {
	return nil, fmt.Errorf("haiai: FetchKeyByHashFromURL is deprecated; use Client.FetchKeyByHash instead")
}
