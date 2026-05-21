package haiai

import (
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"strconv"
	"time"
)

// authHeaderMessage constructs the message to be signed for JACS auth headers.
func authHeaderMessage(jacsID, timestamp, nonce string) string {
	return fmt.Sprintf("%s:%s:%s", jacsID, timestamp, nonce)
}

// authHeaderValue constructs the full JACS auth header value from parts.
func authHeaderValue(jacsID, timestamp, nonce, signatureB64 string) string {
	return fmt.Sprintf("JACS %s:%s:%s:%s", jacsID, timestamp, nonce, signatureB64)
}

func authHeaderNonce() string {
	var buf [16]byte
	if _, err := rand.Read(buf[:]); err != nil {
		return strconv.FormatInt(time.Now().UnixNano(), 36)
	}
	return hex.EncodeToString(buf[:])
}

// build4PartAuthHeaderWithFFI constructs a versioned JACS authentication header
// using the FFI client for signing. Used during key rotation where a specific
// version must be included.
func build4PartAuthHeaderWithFFI(jacsID, version string, ffiClient FFIClient) (string, error) {
	if ffiClient == nil {
		return "", newError(ErrSigningFailed, "FFI client is not initialized")
	}

	timestamp := strconv.FormatInt(time.Now().Unix(), 10)
	nonce := authHeaderNonce()
	message := fmt.Sprintf("%s:%s:%s:%s", jacsID, version, timestamp, nonce)

	sigB64, err := ffiClient.SignMessage(message)
	if err != nil {
		return "", wrapError(ErrSigningFailed, err, "failed to build rotation auth header")
	}
	return fmt.Sprintf("JACS %s:%s:%s:%s:%s", jacsID, version, timestamp, nonce, sigB64), nil
}
