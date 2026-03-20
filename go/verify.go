package haiai

import (
	"encoding/base64"
	"encoding/json"
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
// If baseUrl is empty, "https://beta.hai.ai" is used.
// Uses local base64url encoding. For JACS-delegated encoding, use
// GenerateVerifyLinkWithBackend.
func GenerateVerifyLink(document string, baseUrl string) (string, error) {
	return generateVerifyLinkImpl(document, baseUrl, nil)
}

// GenerateVerifyLinkWithBackend creates a verification URL, delegating the
// base64url encoding to the CryptoBackend when available.
func GenerateVerifyLinkWithBackend(document string, baseUrl string, backend CryptoBackend) (string, error) {
	return generateVerifyLinkImpl(document, baseUrl, backend)
}

func generateVerifyLinkImpl(document string, baseUrl string, backend CryptoBackend) (string, error) {
	if baseUrl == "" {
		baseUrl = "https://beta.hai.ai"
	}
	base := strings.TrimRight(baseUrl, "/")

	var encoded string
	if backend != nil {
		if enc, err := backend.EncodeVerifyPayload(document); err == nil {
			encoded = enc
		} else {
			// Fall back to local encoding
			encoded = base64.RawURLEncoding.EncodeToString([]byte(document))
		}
	} else {
		encoded = base64.RawURLEncoding.EncodeToString([]byte(document))
	}

	fullUrl := base + "/jacs/verify?s=" + encoded
	if len(fullUrl) > MaxVerifyURLLen {
		return "", fmt.Errorf(
			"verify URL would exceed max length (%d). Document must be at most %d UTF-8 bytes",
			MaxVerifyURLLen, MaxVerifyDocumentBytes,
		)
	}
	return fullUrl, nil
}

// GenerateVerifyLinkHosted creates a hosted verification URL for a signed JACS document.
// The document must contain one of: jacsDocumentId, document_id, or id.
// If baseUrl is empty, "https://beta.hai.ai" is used.
func GenerateVerifyLinkHosted(document string, baseUrl string) (string, error) {
	if baseUrl == "" {
		baseUrl = "https://beta.hai.ai"
	}
	base := strings.TrimRight(baseUrl, "/")

	var parsed map[string]any
	if err := json.Unmarshal([]byte(document), &parsed); err != nil {
		return "", fmt.Errorf("cannot generate hosted verify link: no document ID found in document")
	}

	docID := ""
	if value, ok := parsed["jacsDocumentId"].(string); ok && value != "" {
		docID = value
	} else if value, ok := parsed["document_id"].(string); ok && value != "" {
		docID = value
	} else if value, ok := parsed["id"].(string); ok && value != "" {
		docID = value
	}

	if docID == "" {
		return "", fmt.Errorf("cannot generate hosted verify link: no document ID found in document")
	}

	return base + "/verify/" + docID, nil
}
