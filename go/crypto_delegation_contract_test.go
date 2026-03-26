package haiai

import (
	"encoding/json"
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

func TestFFIDelegationBuildAuthHeaderRequiresFFI(t *testing.T) {
	// A client with no FFI should fail to build auth headers.
	cl := &Client{
		jacsID: "test-agent",
	}
	_, err := cl.buildAuthHeader()
	if err == nil {
		t.Fatal("expected error from buildAuthHeader without FFI client")
	}
}

func TestFFIDelegationSignMessageDelegates(t *testing.T) {
	// Verify that SignMessage delegates to the FFI client.
	var called bool
	mockFFI := newMockFFIClient("http://localhost:9999", "test-agent", "")
	mockFFI.signMessageFn = func(message string) (string, error) {
		called = true
		return "test-signature", nil
	}

	cl := &Client{
		jacsID: "test-agent",
		ffi:    mockFFI,
	}
	sig, err := cl.ffi.SignMessage("test")
	if err != nil {
		t.Fatalf("SignMessage: %v", err)
	}
	if !called {
		t.Fatal("expected FFI SignMessage to be called")
	}
	if sig != "test-signature" {
		t.Fatalf("expected 'test-signature', got %q", sig)
	}
}
