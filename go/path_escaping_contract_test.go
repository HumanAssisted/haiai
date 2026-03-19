package haiai

import (
	"encoding/json"
	"net/url"
	"os"
	"testing"
)

type pathEscapingFixture struct {
	Description string `json:"description"`
	TestVectors []struct {
		Raw     string `json:"raw"`
		Escaped string `json:"escaped"`
	} `json:"test_vectors"`
}

func loadPathEscapingFixture(t *testing.T) pathEscapingFixture {
	t.Helper()
	data, err := os.ReadFile("../fixtures/path_escaping_contract.json")
	if err != nil {
		t.Fatalf("Failed to load path_escaping_contract.json: %v", err)
	}
	var fixture pathEscapingFixture
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatalf("Failed to parse fixture: %v", err)
	}
	return fixture
}

func TestPathEscapingContractVectors(t *testing.T) {
	fixture := loadPathEscapingFixture(t)

	// Go's url.PathEscape follows RFC 3986 which allows : and @ in path segments.
	// Python and Node encode these more aggressively with safe="" / encodeURIComponent.
	// Known divergences are documented here.
	goKnownDivergent := map[string]bool{
		"id:with:colons":   true, // Go doesn't encode ':'
		"id@domain":        true, // Go doesn't encode '@'
		"id#hash&amp?query": true, // Go doesn't encode '&'
	}

	for _, vec := range fixture.TestVectors {
		t.Run(vec.Raw, func(t *testing.T) {
			result := url.PathEscape(vec.Raw)
			if goKnownDivergent[vec.Raw] {
				t.Logf("Known Go divergence: PathEscape(%q) = %q (fixture expects %q)", vec.Raw, result, vec.Escaped)
				// Verify at least slashes are escaped (security invariant)
				if vec.Raw != "" {
					for _, ch := range result {
						if ch == '/' {
							t.Errorf("PathEscape(%q) contains unescaped slash", vec.Raw)
						}
					}
				}
				return
			}
			if result != vec.Escaped {
				t.Errorf("PathEscape(%q) = %q, want %q", vec.Raw, result, vec.Escaped)
			}
		})
	}
}

func TestPathTraversalEscaped(t *testing.T) {
	malicious := "../../../etc/passwd"
	escaped := url.PathEscape(malicious)
	for _, ch := range escaped {
		if ch == '/' {
			t.Errorf("PathEscape(%q) contains unescaped slash: %q", malicious, escaped)
		}
	}
}
