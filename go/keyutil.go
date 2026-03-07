package haiai

import (
	"encoding/base64"
	"strings"
)

// decodePublicKey converts a raw-b64 or PEM/base64 public key string into bytes.
// It mirrors the decoding logic from FetchRemoteKeyFromURL: prefer public_key_raw_b64,
// fall back to PEM passthrough, then try base64 decoding.
func decodePublicKey(rawB64, publicKey string) ([]byte, error) {
	switch {
	case rawB64 != "":
		return base64.StdEncoding.DecodeString(rawB64)
	case strings.Contains(publicKey, "BEGIN PUBLIC KEY"):
		return []byte(publicKey), nil
	case publicKey != "":
		return base64.StdEncoding.DecodeString(publicKey)
	default:
		return nil, nil
	}
}
