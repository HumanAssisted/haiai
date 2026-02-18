package haisdk

import (
	"encoding/base64"
	"strings"
	"testing"
)

// ===========================================================================
// GenerateVerifyLink tests
// ===========================================================================

func TestGenerateVerifyLinkBasic(t *testing.T) {
	doc := `{"jacsId":"test-123","jacsSignature":{"signature":"abc"}}`
	link, err := GenerateVerifyLink(doc, "")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.HasPrefix(link, "https://hai.ai/jacs/verify?s=") {
		t.Errorf("expected default base URL, got '%s'", link)
	}
	// Decode the query parameter and verify it matches the input
	encoded := strings.TrimPrefix(link, "https://hai.ai/jacs/verify?s=")
	decoded, err := base64.RawURLEncoding.DecodeString(encoded)
	if err != nil {
		t.Fatalf("failed to decode base64: %v", err)
	}
	if string(decoded) != doc {
		t.Errorf("decoded document mismatch: got '%s'", string(decoded))
	}
}

func TestGenerateVerifyLinkCustomBaseUrl(t *testing.T) {
	doc := `{"jacsId":"test"}`
	link, err := GenerateVerifyLink(doc, "https://example.com")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.HasPrefix(link, "https://example.com/jacs/verify?s=") {
		t.Errorf("expected custom base URL, got '%s'", link)
	}
}

func TestGenerateVerifyLinkTrimsTrailingSlash(t *testing.T) {
	doc := `{"jacsId":"test"}`
	link, err := GenerateVerifyLink(doc, "https://example.com/")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.HasPrefix(link, "https://example.com/jacs/verify?s=") {
		t.Errorf("expected trailing slash trimmed, got '%s'", link)
	}
}

func TestGenerateVerifyLinkURLSafeBase64(t *testing.T) {
	// Document with characters that would produce + or / in standard base64
	doc := `{"jacsId":"test???","data":">>><<<+++"}`
	link, err := GenerateVerifyLink(doc, "")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	encoded := strings.TrimPrefix(link, "https://hai.ai/jacs/verify?s=")
	// URL-safe base64 should not contain + or /
	if strings.ContainsAny(encoded, "+/=") {
		t.Errorf("encoded string contains non-URL-safe characters: '%s'", encoded)
	}
}

func TestGenerateVerifyLinkDocumentTooLarge(t *testing.T) {
	// Create a document that exceeds MaxVerifyDocumentBytes
	doc := strings.Repeat("x", MaxVerifyDocumentBytes+1)
	_, err := GenerateVerifyLink(doc, "")
	if err == nil {
		t.Fatal("expected error for document too large")
	}
	if !strings.Contains(err.Error(), "max length") {
		t.Errorf("expected max length error, got: %v", err)
	}
}

func TestGenerateVerifyLinkEmptyDocument(t *testing.T) {
	link, err := GenerateVerifyLink("", "")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if !strings.HasPrefix(link, "https://hai.ai/jacs/verify?s=") {
		t.Errorf("expected valid link for empty doc, got '%s'", link)
	}
}

func TestGenerateVerifyLinkConstants(t *testing.T) {
	if MaxVerifyURLLen != 2048 {
		t.Errorf("expected MaxVerifyURLLen 2048, got %d", MaxVerifyURLLen)
	}
	if MaxVerifyDocumentBytes != 1515 {
		t.Errorf("expected MaxVerifyDocumentBytes 1515, got %d", MaxVerifyDocumentBytes)
	}
}
