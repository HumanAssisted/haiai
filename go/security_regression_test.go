package haiai

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
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

func TestHelloFailsClosedWhenCryptoBackendCannotBuildAuthHeader(t *testing.T) {
	var requests int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		atomic.AddInt32(&requests, 1)
		t.Fatalf("request should not be sent when auth header generation fails")
	}))
	defer server.Close()

	client, _ := newTestClient(t, server.URL)
	client.crypto = &stubCryptoBackend{
		buildAuthHeader: func() (string, error) {
			return "", errors.New("backend unavailable")
		},
		signString: func(string) (string, error) {
			return "", errors.New("should not fall back to SignString")
		},
	}

	_, err := client.Hello(context.Background())
	if err == nil {
		t.Fatal("expected Hello to fail when auth header generation fails")
	}

	var sdkErr *Error
	if !errors.As(err, &sdkErr) || sdkErr.Kind != ErrSigningFailed {
		t.Fatalf("expected ErrSigningFailed, got %v", err)
	}
	if got := atomic.LoadInt32(&requests); got != 0 {
		t.Fatalf("expected zero requests, got %d", got)
	}
}

func TestSubmitResponseFailsClosedWhenBackendCannotSignResponse(t *testing.T) {
	var requests int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		atomic.AddInt32(&requests, 1)
		t.Fatalf("request should not be sent when response signing fails")
	}))
	defer server.Close()

	client, _ := newTestClient(t, server.URL)
	client.crypto = &stubCryptoBackend{
		buildAuthHeader: func() (string, error) {
			return "JACS test-agent-id:1:signature", nil
		},
		signResponse: func(string) (string, error) {
			return "", errors.New("sign response unavailable")
		},
		signBytes: func([]byte) ([]byte, error) {
			return []byte("fallback-signature"), nil
		},
	}

	_, err := client.SubmitResponse(context.Background(), "job-123", ModerationResponse{
		Message: "safe",
	})
	if err == nil {
		t.Fatal("expected SubmitResponse to fail when SignResponse fails")
	}

	var sdkErr *Error
	if !errors.As(err, &sdkErr) || sdkErr.Kind != ErrSigningFailed {
		t.Fatalf("expected ErrSigningFailed, got %v", err)
	}
	if got := atomic.LoadInt32(&requests); got != 0 {
		t.Fatalf("expected zero requests, got %d", got)
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
