package haiai

import (
	"encoding/base64"
	"encoding/json"
	"os"
	"testing"
)

type crossLangFixture struct {
	RequestAuth struct {
		Example struct {
			Method     string `json:"method"`
			URL        string `json:"url"`
			BodyBase64 string `json:"body_base64"`
			StubHeader string `json:"stub_header"`
		} `json:"example"`
	} `json:"request_auth"`
	AuthHeader struct {
		Scheme                string   `json:"scheme"`
		Parts                 []string `json:"parts"`
		SignedMessageTemplate string   `json:"signed_message_template"`
		Example               struct {
			JacsID           string `json:"jacs_id"`
			Timestamp        int64  `json:"timestamp"`
			Nonce            string `json:"nonce"`
			StubSignatureB64 string `json:"stub_signature_base64"`
			ExpectedHeader   string `json:"expected_header"`
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

func TestCrossLangRequestAuthDelegatesExactBytes(t *testing.T) {
	fixture := loadCrossLangFixture(t)
	example := fixture.RequestAuth.Example
	body, err := base64.StdEncoding.DecodeString(example.BodyBase64)
	if err != nil {
		t.Fatal(err)
	}
	mock := newMockFFIClient("https://hai.ai", "fixture-agent", "")
	mock.buildRequestAuthHeaderFn = func(requestJSON string) (string, error) {
		var input map[string]string
		if err := json.Unmarshal([]byte(requestJSON), &input); err != nil {
			t.Fatal(err)
		}
		if len(input) != 3 || input["method"] != example.Method || input["url"] != example.URL || input["body_base64"] != example.BodyBase64 {
			t.Fatalf("request context changed: %s", requestJSON)
		}
		return example.StubHeader, nil
	}
	client := &Client{ffi: mock}
	header, err := client.BuildRequestAuthHeader(example.Method, example.URL, body)
	if err != nil {
		t.Fatal(err)
	}
	if header != example.StubHeader {
		t.Fatalf("header = %q", header)
	}
}

func TestRequestAuthEmptyBodyAndMissingProvider(t *testing.T) {
	mock := newMockFFIClient("https://hai.ai", "fixture-agent", "")
	mock.buildRequestAuthHeaderFn = func(requestJSON string) (string, error) {
		var input map[string]string
		if err := json.Unmarshal([]byte(requestJSON), &input); err != nil {
			t.Fatal(err)
		}
		if len(input) != 3 || input["body_base64"] != "" {
			t.Fatalf("empty bytes changed: %s", requestJSON)
		}
		return "JACS v2.fixture", nil
	}
	client := &Client{ffi: mock}
	if _, err := client.BuildRequestAuthHeader("GET", "https://hai.ai/", nil); err != nil {
		t.Fatal(err)
	}
	if _, err := (&Client{}).BuildRequestAuthHeader("GET", "https://hai.ai/", nil); err == nil {
		t.Fatal("missing FFI should fail")
	}
}
