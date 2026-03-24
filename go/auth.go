package haiai

import (
	"fmt"
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

// build4PartAuthHeaderWithBackend constructs a 4-part JACS authentication header using
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
