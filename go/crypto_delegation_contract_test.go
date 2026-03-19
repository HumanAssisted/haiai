package haiai

import (
	"encoding/json"
	"errors"
	"os"
	"testing"
)

type cryptoDelegationFixture struct {
	Description     string `json:"description"`
	Canonicalization struct {
		TestVectors []struct {
			Input    interface{} `json:"input"`
			Expected string      `json:"expected"`
		} `json:"test_vectors"`
		JacsRequired    bool   `json:"jacs_required"`
		ErrorWhenNoJacs string `json:"error_when_no_jacs"`
	} `json:"canonicalization"`
	Signing struct {
		Operations      []string `json:"operations"`
		JacsRequired    bool     `json:"jacs_required"`
		ErrorWhenNoJacs string   `json:"error_when_no_jacs"`
	} `json:"signing"`
	Verification struct {
		Operations      []string `json:"operations"`
		JacsRequired    bool     `json:"jacs_required"`
		ErrorWhenNoJacs string   `json:"error_when_no_jacs"`
	} `json:"verification"`
}

func loadCryptoDelegationFixture(t *testing.T) cryptoDelegationFixture {
	t.Helper()
	data, err := os.ReadFile("../fixtures/crypto_delegation_contract.json")
	if err != nil {
		t.Fatalf("Failed to load crypto_delegation_contract.json: %v", err)
	}
	var fixture cryptoDelegationFixture
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatalf("Failed to parse fixture: %v", err)
	}
	return fixture
}

func TestCryptoDelegationCanonicalizationVectors(t *testing.T) {
	fixture := loadCryptoDelegationFixture(t)

	for i, vec := range fixture.Canonicalization.TestVectors {
		// Re-serialize input through JSON to get a string, then canonicalize
		inputJSON, err := json.Marshal(vec.Input)
		if err != nil {
			t.Fatalf("vector %d: json.Marshal: %v", i, err)
		}
		result, err := canonicalizeJSONLocal(string(inputJSON))
		if err != nil {
			t.Fatalf("vector %d: canonicalizeJSONLocal: %v", i, err)
		}
		if result != vec.Expected {
			t.Errorf("vector %d: got %q, want %q", i, result, vec.Expected)
		}
	}
}

func TestCryptoDelegationFixtureAssertions(t *testing.T) {
	fixture := loadCryptoDelegationFixture(t)

	if !fixture.Canonicalization.JacsRequired {
		t.Error("fixture asserts canonicalization.jacs_required should be true")
	}
	if !fixture.Signing.JacsRequired {
		t.Error("fixture asserts signing.jacs_required should be true")
	}
	if !fixture.Verification.JacsRequired {
		t.Error("fixture asserts verification.jacs_required should be true")
	}
}

func TestCryptoDelegationModuleLevelFallbackErrors(t *testing.T) {
	// The module-level ed25519Fallback should return structured errors for
	// operations that require JACS.
	fb := &ed25519Fallback{}

	tests := []struct {
		name string
		fn   func() error
	}{
		{"SignString", func() error { _, err := fb.SignString("msg"); return err }},
		{"SignBytes", func() error { _, err := fb.SignBytes([]byte("msg")); return err }},
		{"SignRequest", func() error { _, err := fb.SignRequest("{}"); return err }},
		{"VerifyResponse", func() error { _, err := fb.VerifyResponse("{}"); return err }},
		{"CanonicalizeJSON", func() error { _, err := fb.CanonicalizeJSON("{}"); return err }},
		{"SignResponse", func() error { _, err := fb.SignResponse("{}"); return err }},
		{"EncodeVerifyPayload", func() error { _, err := fb.EncodeVerifyPayload("doc"); return err }},
		{"UnwrapSignedEvent", func() error { _, err := fb.UnwrapSignedEvent("{}", "{}"); return err }},
		{"BuildAuthHeader", func() error { _, err := fb.BuildAuthHeader(); return err }},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			err := tc.fn()
			if err == nil {
				t.Fatal("expected error from fallback")
			}

			var sdkErr *Error
			if !errors.As(err, &sdkErr) {
				t.Fatalf("expected *Error, got %T: %v", err, err)
			}

			// All should be ErrJacsBuildRequired or ErrPrivateKeyMissing
			if sdkErr.Kind != ErrJacsBuildRequired && sdkErr.Kind != ErrPrivateKeyMissing {
				t.Errorf("expected ErrJacsBuildRequired or ErrPrivateKeyMissing, got Kind=%d", sdkErr.Kind)
			}

			// All should have an Action hint
			if sdkErr.Action == "" {
				t.Error("expected non-empty Action hint")
			}
		})
	}
}
