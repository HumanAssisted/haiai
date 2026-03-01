package haisdk

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
)

// conformanceFixture mirrors the JSON structure of fixtures/email_conformance.json.
type conformanceFixture struct {
	ContentHashGolden struct {
		Vectors []struct {
			Name         string `json:"name"`
			Subject      string `json:"subject"`
			Body         string `json:"body"`
			Attachments  []struct {
				Filename    string `json:"filename"`
				ContentType string `json:"content_type"`
				DataUTF8    string `json:"data_utf8"`
			} `json:"attachments"`
			ExpectedHash string `json:"expected_hash"`
		} `json:"vectors"`
	} `json:"content_hash_golden"`

	VerificationResultV2Schema struct {
		RequiredFields   map[string]string `json:"required_fields"`
		FieldStatusValues []string          `json:"field_status_values"`
	} `json:"verification_result_v2_schema"`

	APIContracts struct {
		SignEmail struct {
			Method             string `json:"method"`
			Path               string `json:"path"`
			RequestContentType string `json:"request_content_type"`
		} `json:"sign_email"`
		VerifyEmail struct {
			Method             string `json:"method"`
			Path               string `json:"path"`
			RequestContentType string `json:"request_content_type"`
		} `json:"verify_email"`
		SendEmail struct {
			ExcludedFields []string `json:"excluded_fields"`
		} `json:"send_email"`
	} `json:"api_contracts"`

	MockVerifyResponse struct {
		JSON json.RawMessage `json:"json"`
	} `json:"mock_verify_response"`
}

func loadConformanceFixture(t *testing.T) conformanceFixture {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("..", "fixtures", "email_conformance.json"))
	if err != nil {
		t.Fatalf("read email_conformance.json: %v", err)
	}
	var f conformanceFixture
	if err := json.Unmarshal(data, &f); err != nil {
		t.Fatalf("unmarshal conformance fixture: %v", err)
	}
	return f
}

// ---------------------------------------------------------------------------
// EmailVerificationResultV2 structural conformance
// ---------------------------------------------------------------------------

func TestConformanceMockVerifyResponseDeserialization(t *testing.T) {
	f := loadConformanceFixture(t)

	var result EmailVerificationResultV2
	if err := json.Unmarshal(f.MockVerifyResponse.JSON, &result); err != nil {
		t.Fatalf("failed to deserialize mock verify response into EmailVerificationResultV2: %v", err)
	}

	// Verify all required fields were populated.
	if result.JacsID != "conformance-test-agent-001" {
		t.Fatalf("jacs_id mismatch: %s", result.JacsID)
	}
	if !result.Valid {
		t.Fatal("expected valid=true")
	}
	if result.Algorithm != "ed25519" {
		t.Fatalf("algorithm mismatch: %s", result.Algorithm)
	}
	if result.ReputationTier != "established" {
		t.Fatalf("reputation_tier mismatch: %s", result.ReputationTier)
	}
	if result.DNSVerified == nil || !*result.DNSVerified {
		t.Fatal("expected dns_verified=true")
	}
	if result.Error != nil {
		t.Fatalf("expected error=nil, got %q", *result.Error)
	}

	// Verify field_results.
	if len(result.FieldResults) != 4 {
		t.Fatalf("expected 4 field_results, got %d", len(result.FieldResults))
	}
	if result.FieldResults[0].Field != "subject" || result.FieldResults[0].Status != FieldStatusPass {
		t.Fatalf("unexpected field_results[0]: %+v", result.FieldResults[0])
	}
	if result.FieldResults[3].Field != "date" || result.FieldResults[3].Status != FieldStatusModified {
		t.Fatalf("unexpected field_results[3]: %+v", result.FieldResults[3])
	}

	// Verify chain.
	if len(result.Chain) != 1 {
		t.Fatalf("expected 1 chain entry, got %d", len(result.Chain))
	}
	if result.Chain[0].Signer != "agent@hai.ai" || result.Chain[0].JacsID != "conformance-test-agent-001" {
		t.Fatalf("unexpected chain[0]: %+v", result.Chain[0])
	}
	if !result.Chain[0].Valid || result.Chain[0].Forwarded {
		t.Fatalf("chain[0] should be valid=true, forwarded=false")
	}
}

// ---------------------------------------------------------------------------
// FieldStatus enum conformance
// ---------------------------------------------------------------------------

func TestConformanceFieldStatusValues(t *testing.T) {
	f := loadConformanceFixture(t)
	expectedValues := f.VerificationResultV2Schema.FieldStatusValues

	goValues := map[FieldStatus]bool{
		FieldStatusPass:         true,
		FieldStatusModified:     true,
		FieldStatusFail:         true,
		FieldStatusUnverifiable: true,
	}

	for _, expected := range expectedValues {
		fs := FieldStatus(expected)
		if !goValues[fs] {
			t.Fatalf("FieldStatus %q from conformance fixture is not defined in Go SDK", expected)
		}
	}

	if len(goValues) != len(expectedValues) {
		t.Fatalf("Go SDK has %d FieldStatus values but conformance fixture has %d",
			len(goValues), len(expectedValues))
	}
}

// ---------------------------------------------------------------------------
// API contract conformance: SignEmail
// ---------------------------------------------------------------------------

func TestConformanceSignEmailAPIContract(t *testing.T) {
	f := loadConformanceFixture(t)

	var gotMethod, gotPath, gotContentType string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotMethod = r.Method
		gotPath = r.URL.Path
		gotContentType = r.Header.Get("Content-Type")
		w.Header().Set("Content-Type", "message/rfc822")
		_, _ = w.Write([]byte("signed"))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.SignEmail(context.Background(), []byte("raw email"))
	if err != nil {
		t.Fatalf("SignEmail: %v", err)
	}

	if gotMethod != f.APIContracts.SignEmail.Method {
		t.Fatalf("expected method %s, got %s", f.APIContracts.SignEmail.Method, gotMethod)
	}
	if gotPath != f.APIContracts.SignEmail.Path {
		t.Fatalf("expected path %s, got %s", f.APIContracts.SignEmail.Path, gotPath)
	}
	if gotContentType != f.APIContracts.SignEmail.RequestContentType {
		t.Fatalf("expected content-type %s, got %s", f.APIContracts.SignEmail.RequestContentType, gotContentType)
	}
}

// ---------------------------------------------------------------------------
// API contract conformance: VerifyEmail
// ---------------------------------------------------------------------------

func TestConformanceVerifyEmailAPIContract(t *testing.T) {
	f := loadConformanceFixture(t)

	var gotMethod, gotPath, gotContentType string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotMethod = r.Method
		gotPath = r.URL.Path
		gotContentType = r.Header.Get("Content-Type")
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write(f.MockVerifyResponse.JSON)
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.VerifyEmail(context.Background(), []byte("raw email"))
	if err != nil {
		t.Fatalf("VerifyEmail: %v", err)
	}

	if gotMethod != f.APIContracts.VerifyEmail.Method {
		t.Fatalf("expected method %s, got %s", f.APIContracts.VerifyEmail.Method, gotMethod)
	}
	if gotPath != f.APIContracts.VerifyEmail.Path {
		t.Fatalf("expected path %s, got %s", f.APIContracts.VerifyEmail.Path, gotPath)
	}
	if gotContentType != f.APIContracts.VerifyEmail.RequestContentType {
		t.Fatalf("expected content-type %s, got %s", f.APIContracts.VerifyEmail.RequestContentType, gotContentType)
	}
}

// ---------------------------------------------------------------------------
// API contract conformance: SendEmail excluded fields
// ---------------------------------------------------------------------------

func TestConformanceSendEmailExcludesClientSigningFields(t *testing.T) {
	f := loadConformanceFixture(t)

	var gotBody map[string]interface{}
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		_ = json.Unmarshal(body, &gotBody)
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"message_id":"msg-conf","status":"sent"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.SendEmail(context.Background(), "bob@hai.ai", "Test", "Body")
	if err != nil {
		t.Fatalf("SendEmail: %v", err)
	}

	for _, excluded := range f.APIContracts.SendEmail.ExcludedFields {
		if _, ok := gotBody[excluded]; ok {
			t.Fatalf("SendEmail payload must not contain %q (server handles signing)", excluded)
		}
	}
}

// ---------------------------------------------------------------------------
// Error type conformance
// ---------------------------------------------------------------------------

func TestConformanceErrorTypeSentinels(t *testing.T) {
	// Verify all sentinel errors exist and have the expected string representation.
	sentinels := map[string]error{
		"EMAIL_NOT_ACTIVE":  ErrEmailNotActive,
		"RECIPIENT_NOT_FOUND": ErrRecipientNotFound,
		"RATE_LIMITED":       ErrEmailRateLimited,
	}

	for code, sentinel := range sentinels {
		if sentinel == nil {
			t.Fatalf("sentinel for %q is nil", code)
		}
		if sentinel.Error() == "" {
			t.Fatalf("sentinel for %q has empty error string", code)
		}
	}
}
