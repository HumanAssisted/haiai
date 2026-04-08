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
// If baseUrl is empty, DefaultEndpoint is used.
//
// TODO: This link cannot be embedded in the email it verifies — the signed body would need to
// contain its own base64 encoding (chicken-and-egg), and hosting the content behind a token
// creates a public access path to private messages. Per-message verification is therefore
// recipient-initiated: paste the raw email at /verify.
func GenerateVerifyLink(document string, baseUrl string) (string, error) {
	return generateVerifyLinkImpl(document, baseUrl)
}

func generateVerifyLinkImpl(document string, baseUrl string) (string, error) {
	if baseUrl == "" {
		baseUrl = DefaultEndpoint
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

// GenerateVerifyLinkHosted creates a hosted verification URL for a signed JACS document.
// The document must contain one of: jacsDocumentId, document_id, or id.
// If baseUrl is empty, DefaultEndpoint is used.
//
// TODO: Same constraint as GenerateVerifyLink — hosting content behind a token creates a
// public access path to private messages. Per-message verification is recipient-initiated.
func GenerateVerifyLinkHosted(document string, baseUrl string) (string, error) {
	if baseUrl == "" {
		baseUrl = DefaultEndpoint
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
