package haiai

import (
	"encoding/json"
	"errors"
	"os"
	"sort"
	"testing"
)

type cryptoDelegationFixture struct {
	Description      string `json:"description"`
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

// testCanonicalizeJSON is a test-only local canonicalization for verifying
// fixture vectors. This is NOT used in runtime code.
func testCanonicalizeJSON(jsonStr string) (string, error) {
	var raw interface{}
	if err := json.Unmarshal([]byte(jsonStr), &raw); err != nil {
		return "", err
	}
	sorted := testSortKeys(raw)
	result, err := json.Marshal(sorted)
	if err != nil {
		return "", err
	}
	return string(result), nil
}

type testOrderedEntry struct {
	Key   string
	Value interface{}
}

type testOrderedMap []testOrderedEntry

func (om testOrderedMap) MarshalJSON() ([]byte, error) {
	buf := []byte{'{'}
	for i, entry := range om {
		if i > 0 {
			buf = append(buf, ',')
		}
		key, err := json.Marshal(entry.Key)
		if err != nil {
			return nil, err
		}
		val, err := json.Marshal(entry.Value)
		if err != nil {
			return nil, err
		}
		buf = append(buf, key...)
		buf = append(buf, ':')
		buf = append(buf, val...)
	}
	buf = append(buf, '}')
	return buf, nil
}

func testSortKeys(v interface{}) interface{} {
	switch val := v.(type) {
	case map[string]interface{}:
		keys := make([]string, 0, len(val))
		for k := range val {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		sorted := make(testOrderedMap, 0, len(val))
		for _, k := range keys {
			sorted = append(sorted, testOrderedEntry{k, testSortKeys(val[k])})
		}
		return sorted
	case []interface{}:
		result := make([]interface{}, len(val))
		for i, item := range val {
			result[i] = testSortKeys(item)
		}
		return result
	default:
		return v
	}
}

func TestCryptoDelegationCanonicalizationVectors(t *testing.T) {
	fixture := loadCryptoDelegationFixture(t)

	for i, vec := range fixture.Canonicalization.TestVectors {
		// Re-serialize input through JSON to get a string, then canonicalize
		inputJSON, err := json.Marshal(vec.Input)
		if err != nil {
			t.Fatalf("vector %d: json.Marshal: %v", i, err)
		}
		result, err := testCanonicalizeJSON(string(inputJSON))
		if err != nil {
			t.Fatalf("vector %d: testCanonicalizeJSON: %v", i, err)
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

func TestCryptoDelegationJacsNotLoadedErrors(t *testing.T) {
	// When a JACS agent cannot be loaded, all crypto operations should
	// return structured errors directing the developer to load JACS.
	nlb := &jacsNotLoadedBackend{loadErr: errors.New("test: no agent")}

	tests := []struct {
		name string
		fn   func() error
	}{
		{"SignString", func() error { _, err := nlb.SignString("msg"); return err }},
		{"SignBytes", func() error { _, err := nlb.SignBytes([]byte("msg")); return err }},
		{"SignRequest", func() error { _, err := nlb.SignRequest("{}"); return err }},
		{"VerifyResponse", func() error { _, err := nlb.VerifyResponse("{}"); return err }},
		{"CanonicalizeJSON", func() error { _, err := nlb.CanonicalizeJSON("{}"); return err }},
		{"SignResponse", func() error { _, err := nlb.SignResponse("{}"); return err }},
		{"EncodeVerifyPayload", func() error { _, err := nlb.EncodeVerifyPayload("doc"); return err }},
		{"UnwrapSignedEvent", func() error { _, err := nlb.UnwrapSignedEvent("{}", "{}"); return err }},
		{"BuildAuthHeader", func() error { _, err := nlb.BuildAuthHeader(); return err }},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			err := tc.fn()
			if err == nil {
				t.Fatal("expected error from jacsNotLoadedBackend")
			}

			var sdkErr *Error
			if !errors.As(err, &sdkErr) {
				t.Fatalf("expected *Error, got %T: %v", err, err)
			}

			if sdkErr.Kind != ErrJacsNotLoaded {
				t.Errorf("expected ErrJacsNotLoaded, got Kind=%d", sdkErr.Kind)
			}

			if sdkErr.Action == "" {
				t.Error("expected non-empty Action hint")
			}
		})
	}
}
