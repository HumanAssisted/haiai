package haisdk

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
)

type emailVerificationFixture struct {
	Headers             map[string]string `json:"headers"`
	Subject             string            `json:"subject"`
	Body                string            `json:"body"`
	ExpectedContentHash string            `json:"expected_content_hash"`
	ExpectedSignInput   string            `json:"expected_sign_input"`
	TestPublicKeyPem    string            `json:"test_public_key_pem"`
	TestPublicKeyB64    string            `json:"test_public_key_b64"`
	TestPrivateKeyPem   string            `json:"test_private_key_pem"`
}

func loadVerificationFixture(t *testing.T) emailVerificationFixture {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(contractDir(), "email_verification_example.json"))
	if err != nil {
		t.Fatalf("read email_verification_example.json: %v", err)
	}
	var fixture emailVerificationFixture
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatalf("unmarshal fixture: %v", err)
	}
	return fixture
}

func mockRegistryServer(t *testing.T, fixture emailVerificationFixture) *httptest.Server {
	t.Helper()
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		resp := KeyRegistryResponse{
			Email:          fixture.Headers["From"],
			JacsID:         "test-agent-jacs-id",
			PublicKey:      fixture.TestPublicKeyPem,
			Algorithm:      "ed25519",
			ReputationTier: "established",
			RegisteredAt:   "2026-01-15T00:00:00Z",
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(resp)
	}))
}

func TestParseJacsSignatureHeader(t *testing.T) {
	fields := ParseJacsSignatureHeader("v=1; a=ed25519; id=test-agent; t=1740000000; s=base64sig")
	if fields["v"] != "1" {
		t.Fatalf("v = %q, want %q", fields["v"], "1")
	}
	if fields["a"] != "ed25519" {
		t.Fatalf("a = %q, want %q", fields["a"], "ed25519")
	}
	if fields["id"] != "test-agent" {
		t.Fatalf("id = %q, want %q", fields["id"], "test-agent")
	}
	if fields["t"] != "1740000000" {
		t.Fatalf("t = %q, want %q", fields["t"], "1740000000")
	}
	if fields["s"] != "base64sig" {
		t.Fatalf("s = %q, want %q", fields["s"], "base64sig")
	}
}

func TestVerifyEmailSignatureValid(t *testing.T) {
	fixture := loadVerificationFixture(t)
	srv := mockRegistryServer(t, fixture)
	defer srv.Close()

	// Override time to be close to fixture timestamp
	origNow := nowFunc
	nowFunc = func() int64 { return 1740393600 + 100 }
	defer func() { nowFunc = origNow }()

	result := VerifyEmailSignature(
		fixture.Headers,
		fixture.Subject,
		fixture.Body,
		srv.URL,
	)

	if !result.Valid {
		errMsg := ""
		if result.Error != nil {
			errMsg = *result.Error
		}
		t.Fatalf("expected valid=true, got false (error: %s)", errMsg)
	}
	if result.JacsID != "test-agent-jacs-id" {
		t.Fatalf("JacsID = %q, want %q", result.JacsID, "test-agent-jacs-id")
	}
	if result.ReputationTier != "established" {
		t.Fatalf("ReputationTier = %q, want %q", result.ReputationTier, "established")
	}
	if result.Error != nil {
		t.Fatalf("Error = %v, want nil", result.Error)
	}
}

func TestVerifyContentHashMatchesContract(t *testing.T) {
	fixture := loadVerificationFixture(t)
	h := sha256.Sum256([]byte(fixture.Subject + "\n" + fixture.Body))
	computed := "sha256:" + hex.EncodeToString(h[:])
	if computed != fixture.ExpectedContentHash {
		t.Fatalf("content hash mismatch:\n  got:  %q\n  want: %q", computed, fixture.ExpectedContentHash)
	}
}

func TestVerifyEmailSignatureContentHashMismatch(t *testing.T) {
	fixture := loadVerificationFixture(t)
	headers := make(map[string]string)
	for k, v := range fixture.Headers {
		headers[k] = v
	}
	headers["X-JACS-Content-Hash"] = "sha256:0000000000000000000000000000000000000000000000000000000000000000"

	result := VerifyEmailSignature(headers, fixture.Subject, fixture.Body, "https://hai.ai")

	if result.Valid {
		t.Fatal("expected valid=false for content hash mismatch")
	}
	if result.Error == nil || *result.Error != "Content hash mismatch" {
		t.Fatalf("Error = %v, want 'Content hash mismatch'", result.Error)
	}
}

func TestVerifyEmailSignatureMissingHeaders(t *testing.T) {
	// Missing X-JACS-Signature
	result := VerifyEmailSignature(
		map[string]string{"X-JACS-Content-Hash": "sha256:abc", "From": "test@hai.ai"},
		"Test", "Body", "",
	)
	if result.Valid {
		t.Fatal("expected valid=false for missing X-JACS-Signature")
	}
	if result.Error == nil || *result.Error != "Missing X-JACS-Signature header" {
		t.Fatalf("unexpected error: %v", result.Error)
	}

	// Missing X-JACS-Content-Hash
	result = VerifyEmailSignature(
		map[string]string{"X-JACS-Signature": "v=1; a=ed25519; id=x; t=1; s=abc", "From": "test@hai.ai"},
		"Test", "Body", "",
	)
	if result.Valid {
		t.Fatal("expected valid=false for missing X-JACS-Content-Hash")
	}
	if result.Error == nil || *result.Error != "Missing X-JACS-Content-Hash header" {
		t.Fatalf("unexpected error: %v", result.Error)
	}
}

func TestVerifyEmailSignatureRegistryFetchFailure(t *testing.T) {
	fixture := loadVerificationFixture(t)

	// Use a URL that will fail to connect
	result := VerifyEmailSignature(
		fixture.Headers,
		fixture.Subject,
		fixture.Body,
		"http://127.0.0.1:1",
	)

	if result.Valid {
		t.Fatal("expected valid=false for registry fetch failure")
	}
	if result.Error == nil {
		t.Fatal("expected error message for fetch failure")
	}
}

func TestVerifyEmailSignatureStaleTimestamp(t *testing.T) {
	fixture := loadVerificationFixture(t)
	srv := mockRegistryServer(t, fixture)
	defer srv.Close()

	// Override time to be >24h after fixture timestamp
	origNow := nowFunc
	nowFunc = func() int64 { return 1740393600 + 90000 }
	defer func() { nowFunc = origNow }()

	result := VerifyEmailSignature(fixture.Headers, fixture.Subject, fixture.Body, srv.URL)

	if result.Valid {
		t.Fatal("expected valid=false for stale timestamp")
	}
	if result.Error == nil || *result.Error != "Signature timestamp is too old (>24h)" {
		t.Fatalf("Error = %v, want 'Signature timestamp is too old (>24h)'", result.Error)
	}
}

func TestVerifyEmailSignatureTamperedSignature(t *testing.T) {
	fixture := loadVerificationFixture(t)
	srv := mockRegistryServer(t, fixture)
	defer srv.Close()

	headers := make(map[string]string)
	for k, v := range fixture.Headers {
		headers[k] = v
	}
	// Tamper with the signature
	original := headers["X-JACS-Signature"]
	headers["X-JACS-Signature"] = original[:len(original)-4] + "AAAA"

	result := VerifyEmailSignature(headers, fixture.Subject, fixture.Body, srv.URL)

	if result.Valid {
		t.Fatal("expected valid=false for tampered signature")
	}
	if result.Error == nil || *result.Error != "Signature verification failed" {
		t.Fatalf("unexpected error: %v", result.Error)
	}
}
