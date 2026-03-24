package haiai

import (
	"context"
	"crypto/ed25519"
	"encoding/json"
	"errors"
	"fmt"
	"testing"
)

type stubCryptoBackend struct {
	buildAuthHeader func() (string, error)
	signString      func(string) (string, error)
	signBytes       func([]byte) ([]byte, error)
	signResponse    func(string) (string, error)
}

func (s *stubCryptoBackend) SignString(message string) (string, error) {
	if s.signString != nil {
		return s.signString(message)
	}
	return "", errors.New("unexpected SignString call")
}

func (s *stubCryptoBackend) SignBytes(data []byte) ([]byte, error) {
	if s.signBytes != nil {
		return s.signBytes(data)
	}
	return nil, errors.New("unexpected SignBytes call")
}

func (s *stubCryptoBackend) VerifyBytes(data, signature []byte, publicKeyPEM string) error {
	return nil
}

func (s *stubCryptoBackend) SignRequest(payloadJSON string) (string, error) {
	return "", errors.New("not implemented")
}

func (s *stubCryptoBackend) VerifyResponse(documentJSON string) (string, error) {
	return "", errors.New("not implemented")
}

func (s *stubCryptoBackend) GenerateKeyPair() ([]byte, []byte, error) {
	return nil, nil, errors.New("not implemented")
}

func (s *stubCryptoBackend) Algorithm() string {
	return "stub"
}

func (s *stubCryptoBackend) CanonicalizeJSON(jsonStr string) (string, error) {
	return jsonStr, nil
}

func (s *stubCryptoBackend) SignResponse(payloadJSON string) (string, error) {
	if s.signResponse != nil {
		return s.signResponse(payloadJSON)
	}
	return "", errors.New("unexpected SignResponse call")
}

func (s *stubCryptoBackend) EncodeVerifyPayload(document string) (string, error) {
	return document, nil
}

func (s *stubCryptoBackend) UnwrapSignedEvent(eventJSON, serverKeysJSON string) (string, error) {
	return "", errors.New("not implemented")
}

func (s *stubCryptoBackend) BuildAuthHeader() (string, error) {
	if s.buildAuthHeader != nil {
		return s.buildAuthHeader()
	}
	return "", errors.New("unexpected BuildAuthHeader call")
}

func (s *stubCryptoBackend) SignA2AArtifact(artifactJSON string, artifactType string) (string, error) {
	return "", errors.New("not implemented")
}

func (s *stubCryptoBackend) VerifyA2AArtifact(wrappedJSON string) (string, error) {
	return "", errors.New("not implemented")
}

func (s *stubCryptoBackend) VerifyA2AArtifactWithPolicy(wrappedJSON, agentCardJSON, policyJSON string) (string, error) {
	return "", errors.New("not implemented")
}

func (s *stubCryptoBackend) AssessA2AAgent(agentCardJSON, policyJSON string) (string, error) {
	return "", errors.New("not implemented")
}

func (s *stubCryptoBackend) ExportAgentCard(agentDataJSON string) (string, error) {
	return "", errors.New("not implemented")
}

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

func TestHelloFailsClosedWhenCryptoBackendCannotBuildAuthHeader(t *testing.T) {
	// In the FFI architecture, auth failures are reported by the FFI layer.
	// Verify that an FFI auth error is properly mapped to ErrAuthRequired.
	errFFI := &errorFFIClient{
		helloErr: fmt.Errorf("AuthFailed: backend unavailable"),
	}

	cl, err := NewClient(
		WithEndpoint("http://localhost:9999"),
		WithJACSID("test-agent-id"),
		WithPrivateKey(make([]byte, ed25519.PrivateKeySize)),
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

func TestSubmitResponseFailsClosedWhenBackendCannotSignResponse(t *testing.T) {
	// In the FFI architecture, signing failures are reported by the FFI layer.
	errFFI := &errorFFIClient{
		submitResponseErr: fmt.Errorf("AuthFailed: sign response unavailable"),
	}

	cl, err := NewClient(
		WithEndpoint("http://localhost:9999"),
		WithJACSID("test-agent-id"),
		WithPrivateKey(make([]byte, ed25519.PrivateKeySize)),
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

func TestBuild4PartAuthHeaderFailsClosedWithoutBackendSignature(t *testing.T) {
	_, _, err := GenerateKeyPair()
	if err != nil {
		t.Fatalf("GenerateKeyPair: %v", err)
	}

	_, err = build4PartAuthHeaderWithBackend(
		"agent-123",
		"v1",
		&stubCryptoBackend{
			signString: func(string) (string, error) {
				return "", errors.New("sign string unavailable")
			},
		},
	)
	if err == nil {
		t.Fatal("expected build4PartAuthHeaderWithBackend to fail closed")
	}
}

var _ CryptoBackend = (*stubCryptoBackend)(nil)
