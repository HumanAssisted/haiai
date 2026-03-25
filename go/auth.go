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

// build4PartAuthHeaderWithFFI constructs a 4-part JACS authentication header
// using the FFI client for signing. Used during key rotation where a specific
// version must be included.
func build4PartAuthHeaderWithFFI(jacsID, version string, ffiClient FFIClient) (string, error) {
	if ffiClient == nil {
		return "", newError(ErrSigningFailed, "FFI client is not initialized")
	}

	timestamp := strconv.FormatInt(time.Now().Unix(), 10)
	message := fmt.Sprintf("%s:%s:%s", jacsID, version, timestamp)

	sigB64, err := ffiClient.SignMessage(message)
	if err != nil {
		return "", wrapError(ErrSigningFailed, err, "failed to build rotation auth header")
	}
	return fmt.Sprintf("JACS %s:%s:%s:%s", jacsID, version, timestamp, sigB64), nil
}
