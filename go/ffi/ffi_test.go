//go:build cgo

package ffi

import (
	"encoding/json"
	"strings"
	"testing"
)

// TestFFISmokeNewClient verifies that the FFI binding can create a client.
// With StaticJacsProvider (no jacs_config_path), construction should succeed.
func TestFFISmokeNewClient(t *testing.T) {
	client, err := NewClient(`{"base_url":"https://beta.hai.ai","jacs_id":"test-agent:1"}`)
	if err != nil {
		t.Fatalf("NewClient failed: %v", err)
	}
	defer client.Close()
	t.Log("Client created successfully with StaticJacsProvider")
}

// TestClientDoubleClose verifies that Close() is safe to call multiple times.
func TestClientDoubleClose(t *testing.T) {
	client, err := NewClient(`{"base_url":"https://beta.hai.ai","jacs_id":"test-close:1"}`)
	if err != nil {
		t.Fatalf("NewClient failed: %v", err)
	}
	client.Close()
	client.Close() // should not panic
}

// TestClientMethodAfterClose verifies that methods return an error after Close().
func TestClientMethodAfterClose(t *testing.T) {
	client, err := NewClient(`{"base_url":"https://beta.hai.ai","jacs_id":"test-after-close:1"}`)
	if err != nil {
		t.Fatalf("NewClient failed: %v", err)
	}
	client.Close()

	_, err = client.Hello(false)
	if err == nil {
		t.Fatal("expected error after Close(), got nil")
	}
	if err.Error() != "client is closed" {
		t.Errorf("expected 'client is closed', got: %v", err)
	}
}

// TestParseEnvelopeOK verifies parsing a successful envelope.
func TestParseEnvelopeOK(t *testing.T) {
	raw, err := parseEnvelope(`{"ok":{"hello":"world"}}`)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	var m map[string]string
	if err := json.Unmarshal(raw, &m); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if m["hello"] != "world" {
		t.Errorf("expected hello=world, got %v", m["hello"])
	}
}

// TestParseEnvelopeError verifies parsing an error envelope.
func TestParseEnvelopeError(t *testing.T) {
	_, err := parseEnvelope(`{"error":{"kind":"AuthFailed","message":"token expired"}}`)
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	ffiErr, ok := err.(*FFIError)
	if !ok {
		t.Fatalf("expected *FFIError, got %T", err)
	}
	if ffiErr.Kind != "AuthFailed" {
		t.Errorf("expected kind=AuthFailed, got %s", ffiErr.Kind)
	}
	if ffiErr.Message != "token expired" {
		t.Errorf("expected message='token expired', got %s", ffiErr.Message)
	}
}

// TestMapFFIError verifies error mapping for all error kinds.
func TestMapFFIError(t *testing.T) {
	cases := []struct {
		kind     string
		wantAuth bool
		wantRate bool
		wantNF   bool
	}{
		{"AuthFailed", true, false, false},
		{"ProviderError", true, false, false},
		{"RateLimited", false, true, false},
		{"NotFound", false, false, true},
		{"NetworkFailed", false, false, false},
		{"ApiError", false, false, false},
		{"Generic", false, false, false},
	}
	for _, tc := range cases {
		err := MapFFIError(&FFIError{Kind: tc.kind, Message: "test"})
		if IsAuthError(err) != tc.wantAuth {
			t.Errorf("IsAuthError(%s) = %v, want %v", tc.kind, IsAuthError(err), tc.wantAuth)
		}
		if IsRateLimited(err) != tc.wantRate {
			t.Errorf("IsRateLimited(%s) = %v, want %v", tc.kind, IsRateLimited(err), tc.wantRate)
		}
		if IsNotFound(err) != tc.wantNF {
			t.Errorf("IsNotFound(%s) = %v, want %v", tc.kind, IsNotFound(err), tc.wantNF)
		}
	}
}

// TestMapFFIErrorNil verifies nil passthrough.
func TestMapFFIErrorNil(t *testing.T) {
	if MapFFIError(nil) != nil {
		t.Error("expected nil for nil input")
	}
}

// TestParseStringResponseAcceptsQuotedString verifies the happy path:
// a JSON-quoted string in the `ok` payload is unmarshaled and returned.
func TestParseStringResponseAcceptsQuotedString(t *testing.T) {
	out, err := parseStringResponse(`{"ok":"memory:v1"}`)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if out != "memory:v1" {
		t.Errorf("expected memory:v1, got %q", out)
	}
}

// TestParseStringResponseNullReturnsEmpty verifies that a null `ok`
// payload maps to an empty string with no error.
func TestParseStringResponseNullReturnsEmpty(t *testing.T) {
	out, err := parseStringResponse(`{"ok":null}`)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if out != "" {
		t.Errorf("expected empty string for null `ok`, got %q", out)
	}
}

// TestParseStringResponseRejectsObjectPayload verifies the loud-fail
// behavior introduced for Issue 014: a wire-contract violation
// (a JSON object where a quoted string is required) MUST surface as
// an error rather than silently coercing the raw bytes to a string.
func TestParseStringResponseRejectsObjectPayload(t *testing.T) {
	_, err := parseStringResponse(`{"ok":{"object":1}}`)
	if err == nil {
		t.Fatal("expected error for object `ok` payload, got nil")
	}
	if !strings.Contains(err.Error(), "FFI envelope `ok` was not a JSON string") {
		t.Errorf("expected wire-contract error, got: %v", err)
	}
}

// TestParseStringResponseRejectsArrayPayload — same shape as the
// object case, exercises the fail-loud guard with a different
// non-string JSON type.
func TestParseStringResponseRejectsArrayPayload(t *testing.T) {
	_, err := parseStringResponse(`{"ok":[1,2,3]}`)
	if err == nil {
		t.Fatal("expected error for array `ok` payload, got nil")
	}
	if !strings.Contains(err.Error(), "FFI envelope `ok` was not a JSON string") {
		t.Errorf("expected wire-contract error, got: %v", err)
	}
}

// TestParseOptionalStringResponseRejectsObjectPayload — Issue 014
// applies identically to the Optional<String> parser used by
// `GetMemory` / `GetSoul`.
func TestParseOptionalStringResponseRejectsObjectPayload(t *testing.T) {
	_, err := parseOptionalStringResponse(`{"ok":{"object":1}}`)
	if err == nil {
		t.Fatal("expected error for object `ok` payload, got nil")
	}
	if !strings.Contains(err.Error(), "FFI envelope `ok` was not a JSON string") {
		t.Errorf("expected wire-contract error, got: %v", err)
	}
}

// TestParseOptionalStringResponseAcceptsNull verifies the only legitimate
// non-string payload — `null` — maps to `("", nil)`.
func TestParseOptionalStringResponseAcceptsNull(t *testing.T) {
	out, err := parseOptionalStringResponse(`{"ok":null}`)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if out != "" {
		t.Errorf("expected empty string for null `ok`, got %q", out)
	}
}
