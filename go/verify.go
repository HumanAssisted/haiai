package haisdk

import (
	"encoding/base64"
	"fmt"
	"strings"
)

const (
	// MaxVerifyURLLen is the maximum allowed length for a verify URL.
	MaxVerifyURLLen = 2048
	// MaxVerifyDocumentBytes is the maximum document size in bytes that can
	// fit within MaxVerifyURLLen after base64 encoding and URL prefix.
	MaxVerifyDocumentBytes = 1515
)

// GenerateVerifyLink creates a verification URL for a signed JACS document.
// The document is base64url-encoded and appended as a query parameter.
// If baseUrl is empty, "https://hai.ai" is used.
func GenerateVerifyLink(document string, baseUrl string) (string, error) {
	if baseUrl == "" {
		baseUrl = "https://hai.ai"
	}
	base := strings.TrimRight(baseUrl, "/")
	encoded := base64.RawURLEncoding.EncodeToString([]byte(document))
	fullUrl := base + "/jacs/verify?s=" + encoded
	if len(fullUrl) > MaxVerifyURLLen {
		return "", fmt.Errorf(
			"verify URL would exceed max length (%d). Document must be at most %d UTF-8 bytes",
			MaxVerifyURLLen, MaxVerifyDocumentBytes,
		)
	}
	return fullUrl, nil
}
