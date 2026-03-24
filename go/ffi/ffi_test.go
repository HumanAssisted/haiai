package ffi

import (
	"encoding/json"
	"testing"
)

// TestFFISmokeNewClient verifies that the FFI binding can create a client
// (which will fail due to test config, but proves the cdylib loads and
// the C ABI works).
func TestFFISmokeNewClient(t *testing.T) {
	// Attempt to create a client with minimal config.
	// This should fail (no real JACS provider) but it exercises the FFI boundary.
	_, err := NewClient(`{"base_url":"https://beta.hai.ai","jacs_id":"test-agent:1"}`)
	if err == nil {
		t.Log("Client created successfully (test JACS provider)")
	} else {
		t.Logf("Client creation returned error (expected in CI): %v", err)
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
