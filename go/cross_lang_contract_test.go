package haisdk

import (
	"encoding/json"
	"os"
	"strconv"
	"testing"
)

type crossLangFixture struct {
	AuthHeader struct {
		Scheme                string   `json:"scheme"`
		Parts                 []string `json:"parts"`
		SignedMessageTemplate string   `json:"signed_message_template"`
		Example               struct {
			JacsID             string `json:"jacs_id"`
			Timestamp          int64  `json:"timestamp"`
			StubSignatureB64   string `json:"stub_signature_base64"`
			ExpectedHeader     string `json:"expected_header"`
		} `json:"example"`
	} `json:"auth_header"`
	CanonicalJSONCases []struct {
		Name     string      `json:"name"`
		Input    interface{} `json:"input"`
		Expected string      `json:"expected"`
	} `json:"canonical_json_cases"`
}

func loadCrossLangFixture(t *testing.T) crossLangFixture {
	t.Helper()

	data, err := os.ReadFile("../fixtures/cross_lang_test.json")
	if err != nil {
		t.Fatalf("read cross_lang_test fixture: %v", err)
	}

	var fixture crossLangFixture
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatalf("decode cross_lang_test fixture: %v", err)
	}
	return fixture
}

func TestCrossLangCanonicalJSONCases(t *testing.T) {
	fixture := loadCrossLangFixture(t)

	for _, tc := range fixture.CanonicalJSONCases {
		t.Run(tc.Name, func(t *testing.T) {
			got, err := json.Marshal(tc.Input)
			if err != nil {
				t.Fatalf("json.Marshal: %v", err)
			}
			if string(got) != tc.Expected {
				t.Fatalf("canonical JSON = %q, want %q", string(got), tc.Expected)
			}
		})
	}
}

func TestCrossLangAuthHeaderContract(t *testing.T) {
	fixture := loadCrossLangFixture(t)

	if fixture.AuthHeader.Scheme != "JACS" {
		t.Fatalf("scheme = %q, want JACS", fixture.AuthHeader.Scheme)
	}
	if len(fixture.AuthHeader.Parts) != 3 {
		t.Fatalf("parts len = %d, want 3", len(fixture.AuthHeader.Parts))
	}

	ts := strconv.FormatInt(fixture.AuthHeader.Example.Timestamp, 10)
	message := authHeaderMessage(fixture.AuthHeader.Example.JacsID, ts)
	if message != "test-agent-001:1700000000" {
		t.Fatalf("authHeaderMessage = %q", message)
	}
	if fixture.AuthHeader.SignedMessageTemplate != "{jacs_id}:{timestamp}" {
		t.Fatalf("signed message template = %q", fixture.AuthHeader.SignedMessageTemplate)
	}

	header := authHeaderValue(
		fixture.AuthHeader.Example.JacsID,
		ts,
		fixture.AuthHeader.Example.StubSignatureB64,
	)
	if header != fixture.AuthHeader.Example.ExpectedHeader {
		t.Fatalf("auth header = %q, want %q", header, fixture.AuthHeader.Example.ExpectedHeader)
	}
}
