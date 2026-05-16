package haiai

import (
	"crypto/ed25519"
	"encoding/base64"
	"fmt"
	"net/http"
	"strconv"
	"time"
)

// BuildAuthHeader constructs the JACS authentication header value using a raw
// ed25519 private key. This is a test-only helper retained for backward
// compatibility with unit tests.
//
// Format: "JACS {jacsId}:{timestamp}:{nonce}:{signature_base64}"
func BuildAuthHeader(jacsID string, key ed25519.PrivateKey) string {
	timestamp := strconv.FormatInt(time.Now().Unix(), 10)
	nonce := authHeaderNonce()
	message := authHeaderMessage(jacsID, timestamp, nonce)
	sig := ed25519.Sign(key, []byte(message))
	sigB64 := base64.StdEncoding.EncodeToString(sig)

	return authHeaderValue(jacsID, timestamp, nonce, sigB64)
}

// Build4PartAuthHeader constructs a versioned JACS authentication header value
// using a raw ed25519 private key. Test-only helper.
//
// Format: "JACS {jacsId}:{version}:{timestamp}:{nonce}:{signature_base64}"
func Build4PartAuthHeader(jacsID, version string, key ed25519.PrivateKey) string {
	timestamp := strconv.FormatInt(time.Now().Unix(), 10)
	nonce := authHeaderNonce()
	message := fmt.Sprintf("%s:%s:%s:%s", jacsID, version, timestamp, nonce)
	sig := ed25519.Sign(key, []byte(message))
	sigB64 := base64.StdEncoding.EncodeToString(sig)

	return fmt.Sprintf("JACS %s:%s:%s:%s:%s", jacsID, version, timestamp, nonce, sigB64)
}

// SetAuthHeaders sets the JACS Authorization and Content-Type headers on an
// HTTP request using a raw ed25519 private key. Test-only helper.
func SetAuthHeaders(req *http.Request, jacsID string, key ed25519.PrivateKey) {
	req.Header.Set("Authorization", BuildAuthHeader(jacsID, key))
	req.Header.Set("Content-Type", "application/json")
}
