package haiai

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"testing"
)

// errorFFIClient is a mock FFI client that returns errors for specific methods.
type errorFFIClient struct {
	mockFFIClient
	helloErr          error
	submitResponseErr error
}

func (e *errorFFIClient) Hello(includeTest bool) (json.RawMessage, error) {
	if e.helloErr != nil {
		return nil, e.helloErr
	}
	return e.mockFFIClient.Hello(includeTest)
}

func (e *errorFFIClient) SubmitResponse(paramsJSON string) (json.RawMessage, error) {
	if e.submitResponseErr != nil {
		return nil, e.submitResponseErr
	}
	return e.mockFFIClient.SubmitResponse(paramsJSON)
}

func TestHelloFailsClosedWhenFFIAuthFails(t *testing.T) {
	// Verify that an FFI auth error is properly mapped to ErrAuthRequired.
	errFFI := &errorFFIClient{
		helloErr: fmt.Errorf("AuthFailed: backend unavailable"),
	}

	cl, err := NewClient(
		WithEndpoint("http://localhost:9999"),
		WithJACSID("test-agent-id"),
		WithFFIClient(errFFI),
	)
	if err != nil {
		t.Fatalf("NewClient: %v", err)
	}

	_, err = cl.Hello(context.Background())
	if err == nil {
		t.Fatal("expected Hello to fail when FFI auth fails")
	}

	var sdkErr *Error
	if !errors.As(err, &sdkErr) || sdkErr.Kind != ErrAuthRequired {
		t.Fatalf("expected ErrAuthRequired, got %v", err)
	}
}

func TestSubmitResponseFailsClosedWhenFFISigningFails(t *testing.T) {
	// In the FFI architecture, signing failures are reported by the FFI layer.
	errFFI := &errorFFIClient{
		submitResponseErr: fmt.Errorf("AuthFailed: sign response unavailable"),
	}

	cl, err := NewClient(
		WithEndpoint("http://localhost:9999"),
		WithJACSID("test-agent-id"),
		WithFFIClient(errFFI),
	)
	if err != nil {
		t.Fatalf("NewClient: %v", err)
	}

	_, err = cl.SubmitResponse(context.Background(), "job-123", ModerationResponse{
		Message: "safe",
	})
	if err == nil {
		t.Fatal("expected SubmitResponse to fail when FFI signing fails")
	}
}

func TestBuild4PartAuthHeaderFailsClosedWithoutFFI(t *testing.T) {
	_, err := build4PartAuthHeaderWithFFI("agent-123", "v1", nil)
	if err == nil {
		t.Fatal("expected build4PartAuthHeaderWithFFI to fail with nil FFI client")
	}
}

func TestBuild4PartAuthHeaderFailsClosedWhenSignFails(t *testing.T) {
	mockFFI := newMockFFIClient("http://localhost:9999", "agent-123", "")
	// mockFFI.SignMessage returns "not implemented" by default
	_, err := build4PartAuthHeaderWithFFI("agent-123", "v1", mockFFI)
	if err == nil {
		t.Fatal("expected build4PartAuthHeaderWithFFI to fail when SignMessage fails")
	}
}
