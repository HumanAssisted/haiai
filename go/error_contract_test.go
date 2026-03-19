package haiai

import (
	"encoding/json"
	"errors"
	"os"
	"regexp"
	"testing"
)

type errorContractFixture struct {
	Description string `json:"description"`
	ErrorCodes  map[string]struct {
		MessagePattern    string `json:"message_pattern"`
		ActionHintPattern string `json:"action_hint_pattern"`
	} `json:"error_codes"`
	HTTPErrorMapping map[string]string `json:"http_error_mapping"`
}

func loadErrorContractFixture(t *testing.T) errorContractFixture {
	t.Helper()
	data, err := os.ReadFile("../fixtures/error_contract.json")
	if err != nil {
		t.Fatalf("Failed to load error_contract.json: %v", err)
	}
	var fixture errorContractFixture
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatalf("Failed to parse fixture: %v", err)
	}
	return fixture
}

func TestErrorContractFixtureLoads(t *testing.T) {
	fixture := loadErrorContractFixture(t)
	if len(fixture.ErrorCodes) == 0 {
		t.Fatal("expected at least one error code in fixture")
	}
}

func TestErrorContractAllCodesHavePatterns(t *testing.T) {
	fixture := loadErrorContractFixture(t)
	for code, spec := range fixture.ErrorCodes {
		if spec.MessagePattern == "" {
			t.Errorf("error code %q missing message_pattern", code)
		}
		if spec.ActionHintPattern == "" {
			t.Errorf("error code %q missing action_hint_pattern", code)
		}
	}
}

func TestErrorContractJacsBuildRequiredMatchesPattern(t *testing.T) {
	fixture := loadErrorContractFixture(t)
	spec, ok := fixture.ErrorCodes["JACS_BUILD_REQUIRED"]
	if !ok {
		t.Fatal("JACS_BUILD_REQUIRED not found in fixture")
	}

	fb := &ed25519Fallback{}
	_, err := fb.CanonicalizeJSON("{}")
	if err == nil {
		t.Fatal("expected error from fallback CanonicalizeJSON")
	}

	var sdkErr *Error
	if !errors.As(err, &sdkErr) {
		t.Fatalf("expected *Error, got %T", err)
	}

	msgRe := regexp.MustCompile("(?i)" + spec.MessagePattern)
	if !msgRe.MatchString(sdkErr.Message) {
		t.Errorf("message %q does not match pattern %q", sdkErr.Message, spec.MessagePattern)
	}

	actionRe := regexp.MustCompile("(?i)" + spec.ActionHintPattern)
	if !actionRe.MatchString(sdkErr.Action) {
		t.Errorf("action %q does not match pattern %q", sdkErr.Action, spec.ActionHintPattern)
	}
}

func TestErrorContractVerificationFailedMatchesPattern(t *testing.T) {
	fixture := loadErrorContractFixture(t)
	spec, ok := fixture.ErrorCodes["VERIFICATION_FAILED"]
	if !ok {
		t.Fatal("VERIFICATION_FAILED not found in fixture")
	}

	fb := &ed25519Fallback{}
	err := fb.VerifyBytes([]byte("data"), []byte("badsig"), "not-a-pem")
	if err == nil {
		t.Fatal("expected error from VerifyBytes with bad PEM")
	}

	var sdkErr *Error
	if !errors.As(err, &sdkErr) {
		t.Fatalf("expected *Error, got %T", err)
	}

	if sdkErr.Kind != ErrVerificationFailed {
		t.Errorf("expected ErrVerificationFailed, got Kind=%d", sdkErr.Kind)
	}

	// The error message should match the verification failed pattern
	msgRe := regexp.MustCompile("(?i)" + spec.MessagePattern)
	if !msgRe.MatchString(sdkErr.Message) {
		t.Errorf("message %q does not match pattern %q", sdkErr.Message, spec.MessagePattern)
	}
}
