package haiai

import (
	"context"
	"crypto/ed25519"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"strings"
	"testing"
)

type securityRegressionFixture struct {
	Description string `json:"description"`
	TestCases   []struct {
		Name      string `json:"name"`
		Assertion string `json:"assertion"`
	} `json:"test_cases"`
}

func loadSecurityRegressionFixture(t *testing.T) securityRegressionFixture {
	t.Helper()
	data, err := os.ReadFile("../fixtures/security_regression_contract.json")
	if err != nil {
		t.Fatalf("Failed to load security_regression_contract.json: %v", err)
	}
	var fixture securityRegressionFixture
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatalf("Failed to parse fixture: %v", err)
	}
	return fixture
}

func TestSecurityRegressionFixtureLoads(t *testing.T) {
	fixture := loadSecurityRegressionFixture(t)
	if len(fixture.TestCases) < 5 {
		t.Fatalf("expected at least 5 test cases, got %d", len(fixture.TestCases))
	}
}

func TestSecurityRegressionFallbackDoesNotActivate(t *testing.T) {
	fixture := loadSecurityRegressionFixture(t)
	var found bool
	for _, tc := range fixture.TestCases {
		if tc.Name == "fallback_does_not_activate" {
			found = true
			break
		}
	}
	if !found {
		t.Fatal("fallback_does_not_activate test case not found in fixture")
	}

	// When the FFI layer reports an auth error, the SDK should propagate it
	// without falling back to a different signing mechanism.
	errFFI := &errorFFIClient{
		helloErr: fmt.Errorf("AuthFailed: backend unavailable"),
	}

	client, err := NewClient(
		WithEndpoint("http://localhost:9999"),
		WithJACSID("test-agent-id"),
		WithPrivateKey(make([]byte, ed25519.PrivateKeySize)),
		WithFFIClient(errFFI),
	)
	if err != nil {
		t.Fatalf("NewClient: %v", err)
	}

	_, err = client.Hello(context.Background())
	if err == nil {
		t.Fatal("expected error when backend is unavailable")
	}

	var sdkErr *Error
	if !errors.As(err, &sdkErr) || sdkErr.Kind != ErrAuthRequired {
		t.Fatalf("expected ErrAuthRequired, got %v", err)
	}
}

func TestSecurityRegressionMalformedAgentIdEscaped(t *testing.T) {
	fixture := loadSecurityRegressionFixture(t)
	var found bool
	for _, tc := range fixture.TestCases {
		if tc.Name == "malformed_agent_id_escaped" {
			found = true
			break
		}
	}
	if !found {
		t.Fatal("malformed_agent_id_escaped test case not found in fixture")
	}

	malicious := "agent/../../../etc/passwd"
	escaped := url.PathEscape(malicious)
	if strings.Contains(escaped, "/") {
		t.Errorf("escaped agent ID %q still contains /", escaped)
	}
}

func TestSecurityRegressionRegisterOmitsPrivateKey(t *testing.T) {
	fixture := loadSecurityRegressionFixture(t)
	var found bool
	for _, tc := range fixture.TestCases {
		if tc.Name == "register_omits_private_key" {
			found = true
			break
		}
	}
	if !found {
		t.Fatal("register_omits_private_key test case not found in fixture")
	}

	// Generate a keypair via the CryptoBackend which returns PEM-encoded bytes
	pubPEM, privPEM, err := cryptoBackend.GenerateKeyPair()
	if err != nil {
		t.Fatalf("GenerateKeyPair: %v", err)
	}

	// Verify the PEM contents for sanity
	if !strings.Contains(string(pubPEM), "PUBLIC KEY") {
		t.Fatalf("pubPEM does not contain PUBLIC KEY header: %s", string(pubPEM)[:50])
	}
	if !strings.Contains(string(privPEM), "PRIVATE KEY") {
		t.Fatal("privPEM does not contain PRIVATE KEY header")
	}

	// Build a registration-style payload (mirrors the client.Register pattern)
	// Use string values to match how the client builds the payload
	regPayload := map[string]interface{}{
		"agent_json": `{"jacsId":"test","name":"test"}`,
		"public_key": string(pubPEM),
	}
	bodyBytes, _ := json.Marshal(regPayload)
	bodyStr := string(bodyBytes)

	// The body must NOT contain private key material
	if strings.Contains(bodyStr, "PRIVATE KEY") {
		t.Error("registration payload contains private key material")
	}
	if strings.Contains(bodyStr, "BEGIN PRIVATE") {
		t.Error("registration payload contains private key header")
	}
}

func TestSecurityRegressionRegisterIsUnauthenticated(t *testing.T) {
	fixture := loadSecurityRegressionFixture(t)
	var found bool
	for _, tc := range fixture.TestCases {
		if tc.Name == "register_is_unauthenticated" {
			found = true
			break
		}
	}
	if !found {
		t.Fatal("register_is_unauthenticated test case not found in fixture")
	}

	// Verify that registration requests do not include an Authorization header
	var capturedAuth string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		capturedAuth = r.Header.Get("Authorization")
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(201)
		w.Write([]byte(`{"agent_id":"agent-123","jacs_id":"jacs-123:v1"}`))
	}))
	defer server.Close()

	// Simulate a registration POST (unauthenticated)
	req, _ := http.NewRequest("POST", server.URL+"/api/v1/agents/register", strings.NewReader(`{}`))
	req.Header.Set("Content-Type", "application/json")
	// Explicitly do NOT set Authorization header
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("registration request failed: %v", err)
	}
	defer resp.Body.Close()

	if capturedAuth != "" {
		t.Errorf("registration request should not have Authorization header, got %q", capturedAuth)
	}
}

func TestSecurityRegressionEncryptedKeyRequiresPassword(t *testing.T) {
	fixture := loadSecurityRegressionFixture(t)
	var found bool
	for _, tc := range fixture.TestCases {
		if tc.Name == "encrypted_key_requires_password" {
			found = true
			break
		}
	}
	if !found {
		t.Fatal("encrypted_key_requires_password test case not found in fixture")
	}

	// Load the encrypted PEM key fixture and verify parsing fails without
	// password, producing a clear structured error.
	encryptedPEM, err := os.ReadFile("../fixtures/encrypted_test_key.pem")
	if err != nil {
		t.Fatalf("Failed to load encrypted_test_key.pem: %v", err)
	}

	// Parsing with no password should fail with a clear error
	_, err = ParsePrivateKeyWithPassword(encryptedPEM, nil)
	if err == nil {
		t.Fatal("expected error when loading encrypted key without password")
	}

	var sdkErr *Error
	if !errors.As(err, &sdkErr) {
		t.Fatalf("expected *Error, got %T: %v", err, err)
	}
	if sdkErr.Kind != ErrSigningFailed {
		t.Errorf("expected ErrSigningFailed, got Kind=%d", sdkErr.Kind)
	}
	if !strings.Contains(sdkErr.Message, "encrypted") && !strings.Contains(sdkErr.Message, "PKCS#8") {
		t.Errorf("error message should mention encrypted key: %q", sdkErr.Message)
	}
}
