package haisdk

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"
)

type endpointContract struct {
	Method       string `json:"method"`
	Path         string `json:"path"`
	AuthRequired bool   `json:"auth_required"`
}

type sdkContract struct {
	BaseURL       string           `json:"base_url"`
	Hello         endpointContract `json:"hello"`
	CheckUsername endpointContract `json:"check_username"`
	SubmitResp    endpointContract `json:"submit_response"`
}

func loadContractFixture(t *testing.T) sdkContract {
	t.Helper()

	data, err := os.ReadFile("../fixtures/contract_endpoints.json")
	if err != nil {
		t.Fatalf("read contract fixture: %v", err)
	}

	var fixture sdkContract
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatalf("decode contract fixture: %v", err)
	}
	return fixture
}

func TestHelloContract(t *testing.T) {
	contract := loadContractFixture(t)
	if DefaultEndpoint != contract.BaseURL {
		t.Fatalf("DefaultEndpoint = %q, want %q", DefaultEndpoint, contract.BaseURL)
	}

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != contract.Hello.Method {
			t.Fatalf("unexpected method: %s", r.Method)
		}
		if r.URL.Path != contract.Hello.Path {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		auth := r.Header.Get("Authorization")
		if contract.Hello.AuthRequired && auth == "" {
			t.Fatal("expected Authorization header")
		}
		if !contract.Hello.AuthRequired && auth != "" {
			t.Fatalf("expected no Authorization header, got %q", auth)
		}

		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"timestamp":"2026-01-01T00:00:00Z","client_ip":"127.0.0.1","hai_public_key_fingerprint":"fp","message":"ok","hai_signed_ack":"sig","hello_id":"h1"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	if _, err := cl.Hello(context.Background()); err != nil {
		t.Fatalf("Hello: %v", err)
	}
}

func TestCheckUsernameContract(t *testing.T) {
	contract := loadContractFixture(t)

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != contract.CheckUsername.Method {
			t.Fatalf("unexpected method: %s", r.Method)
		}
		if r.URL.Path != contract.CheckUsername.Path {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		if got := r.URL.Query().Get("username"); got != "alice" {
			t.Fatalf("unexpected username query: %q", got)
		}

		auth := r.Header.Get("Authorization")
		if contract.CheckUsername.AuthRequired && auth == "" {
			t.Fatal("expected Authorization header")
		}
		if !contract.CheckUsername.AuthRequired && auth != "" {
			t.Fatalf("expected no Authorization header, got %q", auth)
		}

		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"available":true,"username":"alice"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	if _, err := cl.CheckUsername(context.Background(), "alice"); err != nil {
		t.Fatalf("CheckUsername: %v", err)
	}
}

func TestSubmitResponseContract(t *testing.T) {
	contract := loadContractFixture(t)
	jobID := "job-123"
	expectedPath := strings.ReplaceAll(contract.SubmitResp.Path, "{job_id}", jobID)

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != contract.SubmitResp.Method {
			t.Fatalf("unexpected method: %s", r.Method)
		}
		if r.URL.Path != expectedPath {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}

		auth := r.Header.Get("Authorization")
		if contract.SubmitResp.AuthRequired && auth == "" {
			t.Fatal("expected Authorization header")
		}
		if !contract.SubmitResp.AuthRequired && auth != "" {
			t.Fatalf("expected no Authorization header, got %q", auth)
		}

		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"success":true,"job_id":"job-123","message":"ok"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.SubmitResponse(context.Background(), jobID, ModerationResponse{
		Message: "response body",
	})
	if err != nil {
		t.Fatalf("SubmitResponse: %v", err)
	}
}
