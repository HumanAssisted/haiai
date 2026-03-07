package haiai

import (
	"crypto/ed25519"
	"encoding/base64"
	"fmt"
	"log"
	"net/http"
	"strconv"
	"time"
)

func authHeaderMessage(jacsID, timestamp string) string {
	return fmt.Sprintf("%s:%s", jacsID, timestamp)
}

func authHeaderValue(jacsID, timestamp, signatureB64 string) string {
	return fmt.Sprintf("JACS %s:%s:%s", jacsID, timestamp, signatureB64)
}

// BuildAuthHeader constructs the JACS authentication header value.
//
// Format: "JACS {jacsId}:{timestamp}:{signature_base64}"
//
// The message signed is "{jacsId}:{timestamp}" where timestamp is Unix seconds.
//
// This function accepts an ed25519.PrivateKey for backward compatibility.
//
// Deprecated: Use Client.buildAuthHeader instead, which delegates to the CryptoBackend.
func BuildAuthHeader(jacsID string, key ed25519.PrivateKey) string {
	timestamp := strconv.FormatInt(time.Now().Unix(), 10)
	message := authHeaderMessage(jacsID, timestamp)
	sig := ed25519.Sign(key, []byte(message))
	sigB64 := base64.StdEncoding.EncodeToString(sig)

	return authHeaderValue(jacsID, timestamp, sigB64)
}

// Build4PartAuthHeader constructs a 4-part JACS authentication header value.
//
// Format: "JACS {jacsId}:{version}:{timestamp}:{signature_base64}"
//
// The signed message is "{jacsId}:{version}:{timestamp}".
// Used during key rotation to authenticate re-registration with the OLD key
// (chain of trust: old key vouches for new key).
//
// This function accepts an ed25519.PrivateKey for backward compatibility.
//
// Deprecated: Use Client.build4PartAuthHeader (via build4PartAuthHeaderWithBackend) instead,
// which delegates to the CryptoBackend.
func Build4PartAuthHeader(jacsID, version string, key ed25519.PrivateKey) string {
	timestamp := strconv.FormatInt(time.Now().Unix(), 10)
	message := fmt.Sprintf("%s:%s:%s", jacsID, version, timestamp)
	sig := ed25519.Sign(key, []byte(message))
	sigB64 := base64.StdEncoding.EncodeToString(sig)

	return fmt.Sprintf("JACS %s:%s:%s:%s", jacsID, version, timestamp, sigB64)
}

// SetAuthHeaders sets the JACS Authorization and Content-Type headers on an HTTP request.
//
// This function accepts an ed25519.PrivateKey for backward compatibility.
//
// Deprecated: Use Client.setAuthHeaders instead, which delegates to the CryptoBackend.
func SetAuthHeaders(req *http.Request, jacsID string, key ed25519.PrivateKey) {
	req.Header.Set("Authorization", BuildAuthHeader(jacsID, key))
	req.Header.Set("Content-Type", "application/json")
}

// buildAuthHeader constructs the JACS authentication header using the Client's
// CryptoBackend. Falls back to direct ed25519 signing if the backend cannot
// sign (e.g., standalone fallback without a loaded key).
func (c *Client) buildAuthHeader() string {
	timestamp := strconv.FormatInt(time.Now().Unix(), 10)
	message := authHeaderMessage(c.jacsID, timestamp)

	if c.crypto != nil {
		sigB64, err := c.crypto.SignString(message)
		if err == nil {
			return authHeaderValue(c.jacsID, timestamp, sigB64)
		}
		log.Printf("WARNING: CryptoBackend.SignString failed, falling back to direct Ed25519: %v", err)
	}

	// Fallback to direct signing
	return BuildAuthHeader(c.jacsID, c.privateKey)
}

// build4PartAuthHeader constructs a 4-part JACS authentication header using
// the provided CryptoBackend. Used during key rotation where a specific
// (old) key's backend is needed.
func build4PartAuthHeaderWithBackend(jacsID, version string, backend CryptoBackend, fallbackKey ed25519.PrivateKey) string {
	timestamp := strconv.FormatInt(time.Now().Unix(), 10)
	message := fmt.Sprintf("%s:%s:%s", jacsID, version, timestamp)

	if backend != nil {
		sigB64, err := backend.SignString(message)
		if err == nil {
			return fmt.Sprintf("JACS %s:%s:%s:%s", jacsID, version, timestamp, sigB64)
		}
		log.Printf("WARNING: CryptoBackend.SignString failed, falling back to direct Ed25519: %v", err)
	}

	// Fallback to direct signing
	return Build4PartAuthHeader(jacsID, version, fallbackKey)
}

// setAuthHeaders sets the JACS Authorization and Content-Type headers using
// the Client's CryptoBackend.
func (c *Client) setAuthHeaders(req *http.Request) {
	req.Header.Set("Authorization", c.buildAuthHeader())
	req.Header.Set("Content-Type", "application/json")
}
