package haiai

import (
	"fmt"
	"net/http"
	"strconv"
	"time"
)

// authHeaderMessage constructs the message to be signed for JACS auth headers.
func authHeaderMessage(jacsID, timestamp string) string {
	return fmt.Sprintf("%s:%s", jacsID, timestamp)
}

// authHeaderValue constructs the full JACS auth header value from parts.
func authHeaderValue(jacsID, timestamp, signatureB64 string) string {
	return fmt.Sprintf("JACS %s:%s:%s", jacsID, timestamp, signatureB64)
}

// buildAuthHeader constructs the JACS authentication header using the Client's
// CryptoBackend and fails closed if the backend cannot produce it.
func (c *Client) buildAuthHeader() (string, error) {
	if c.crypto == nil {
		return "", newError(ErrSigningFailed, "crypto backend is not initialized")
	}

	header, err := c.crypto.BuildAuthHeader()
	if err != nil {
		return "", wrapError(ErrSigningFailed, err, "failed to build JACS auth header")
	}
	return header, nil
}

// build4PartAuthHeader constructs a 4-part JACS authentication header using
// the provided CryptoBackend. Used during key rotation where a specific
// (old) key's backend is needed.
func build4PartAuthHeaderWithBackend(jacsID, version string, backend CryptoBackend) (string, error) {
	if backend == nil {
		return "", newError(ErrSigningFailed, "crypto backend is not initialized")
	}

	timestamp := strconv.FormatInt(time.Now().Unix(), 10)
	message := fmt.Sprintf("%s:%s:%s", jacsID, version, timestamp)

	sigB64, err := backend.SignString(message)
	if err != nil {
		return "", wrapError(ErrSigningFailed, err, "failed to build rotation auth header")
	}
	return fmt.Sprintf("JACS %s:%s:%s:%s", jacsID, version, timestamp, sigB64), nil
}

// setAuthHeaders sets the JACS Authorization and Content-Type headers using
// the Client's CryptoBackend.
func (c *Client) setAuthHeaders(req *http.Request) error {
	header, err := c.buildAuthHeader()
	if err != nil {
		return err
	}
	req.Header.Set("Authorization", header)
	req.Header.Set("Content-Type", "application/json")
	return nil
}
