package haiai

import (
	"context"
	"fmt"
)

// FetchKeyByDomain fetches the latest DNS-verified agent key for a domain.
// Delegates to the Client's FFI-backed method.
func FetchKeyByDomain(ctx context.Context, client *Client, domain string) (*PublicKeyInfo, error) {
	if client != nil {
		return client.FetchKeyByDomain(ctx, domain)
	}
	return nil, fmt.Errorf("haiai: Client required for FetchKeyByDomain (no native HTTP fallback)")
}

// FetchKeyByDomainFromURL is deprecated. Use Client.FetchKeyByDomain instead.
//
// Deprecated: Use Client.FetchKeyByDomain instead.
func FetchKeyByDomainFromURL(ctx context.Context, _ interface{}, baseURL, domain string) (*PublicKeyInfo, error) {
	return nil, fmt.Errorf("haiai: FetchKeyByDomainFromURL is deprecated; use Client.FetchKeyByDomain instead")
}
