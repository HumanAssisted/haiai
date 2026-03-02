package haisdk

import (
	"crypto/ed25519"
	"encoding/base64"
	"fmt"
	"net/http"
	"strconv"
	"time"
)

// BuildAuthHeader constructs the JACS authentication header value.
//
// Format: "JACS {jacsId}:{timestamp}:{signature_base64}"
//
// The message signed is "{jacsId}:{timestamp}" where timestamp is Unix seconds.
func BuildAuthHeader(jacsID string, key ed25519.PrivateKey) string {
	timestamp := strconv.FormatInt(time.Now().Unix(), 10)
	message := fmt.Sprintf("%s:%s", jacsID, timestamp)
	sig := ed25519.Sign(key, []byte(message))
	sigB64 := base64.StdEncoding.EncodeToString(sig)

	return fmt.Sprintf("JACS %s:%s:%s", jacsID, timestamp, sigB64)
}

// Build4PartAuthHeader constructs a 4-part JACS authentication header value.
//
// Format: "JACS {jacsId}:{version}:{timestamp}:{signature_base64}"
//
// The signed message is "{jacsId}:{version}:{timestamp}".
// Used during key rotation to authenticate re-registration with the OLD key
// (chain of trust: old key vouches for new key).
func Build4PartAuthHeader(jacsID, version string, key ed25519.PrivateKey) string {
	timestamp := strconv.FormatInt(time.Now().Unix(), 10)
	message := fmt.Sprintf("%s:%s:%s", jacsID, version, timestamp)
	sig := ed25519.Sign(key, []byte(message))
	sigB64 := base64.StdEncoding.EncodeToString(sig)

	return fmt.Sprintf("JACS %s:%s:%s:%s", jacsID, version, timestamp, sigB64)
}

// SetAuthHeaders sets the JACS Authorization and Content-Type headers on an HTTP request.
func SetAuthHeaders(req *http.Request, jacsID string, key ed25519.PrivateKey) {
	req.Header.Set("Authorization", BuildAuthHeader(jacsID, key))
	req.Header.Set("Content-Type", "application/json")
}
