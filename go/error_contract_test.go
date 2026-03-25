package haiai

import (
	"encoding/json"
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

func TestErrorContractJacsNotLoadedMatchesPattern(t *testing.T) {
	fixture := loadErrorContractFixture(t)
	spec, ok := fixture.ErrorCodes["JACS_NOT_LOADED"]
	if !ok {
		t.Fatal("JACS_NOT_LOADED not found in fixture")
	}

	// Produce a JACS-not-loaded error via the SDK error system
	sdkErr := &Error{
		Kind:    ErrJacsNotLoaded,
		Message: "JACS agent not loaded (test: no agent). Run 'haiai init' or set JACS_CONFIG_PATH",
		Action:  "Run 'haiai init' or set JACS_CONFIG_PATH",
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

	// Produce a verification error via the SDK error system
	sdkErr := &Error{
		Kind:    ErrSigningFailed,
		Message: "signature verification failed: invalid signature",
		Action:  "Verify the document was signed correctly",
	}

	msgRe := regexp.MustCompile("(?i)" + spec.MessagePattern)
	if !msgRe.MatchString(sdkErr.Message) {
		t.Errorf("error %q does not match pattern %q", sdkErr.Message, spec.MessagePattern)
	}
}
