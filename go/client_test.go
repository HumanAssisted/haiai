package haisdk

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestCheckUsernameEncodesQuery(t *testing.T) {
	username := "alice+ops test@hai.ai"

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/agents/username/check" {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		if got := r.URL.Query().Get("username"); got != username {
			t.Fatalf("username query not preserved; got %q", got)
		}
		if strings.Contains(r.URL.RawQuery, " ") {
			t.Fatalf("raw query should be URL-encoded, got %q", r.URL.RawQuery)
		}

		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"available":true,"username":"alice"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	if _, err := cl.CheckUsername(context.Background(), username); err != nil {
		t.Fatalf("CheckUsername: %v", err)
	}
}

func TestListMessagesEncodesQuery(t *testing.T) {
	folder := "inbox & sent/ops"

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/agents/test-agent-id/email/messages" {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		if r.URL.Query().Get("limit") != "25" || r.URL.Query().Get("offset") != "5" {
			t.Fatalf("unexpected pagination query: %s", r.URL.RawQuery)
		}
		if got := r.URL.Query().Get("folder"); got != folder {
			t.Fatalf("folder query not preserved; got %q", got)
		}
		if strings.Contains(r.URL.RawQuery, " ") {
			t.Fatalf("raw query should be URL-encoded, got %q", r.URL.RawQuery)
		}

		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`[{"id":"m1"}]`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.ListMessages(context.Background(), ListMessagesOptions{
		Limit:  25,
		Offset: 5,
		Folder: folder,
	})
	if err != nil {
		t.Fatalf("ListMessages: %v", err)
	}
}

func TestMarkReadEscapesPathSegments(t *testing.T) {
	messageID := "msg/abc+1"
	var requestURI string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requestURI = r.RequestURI
		w.WriteHeader(http.StatusNoContent)
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	if err := cl.MarkRead(context.Background(), messageID); err != nil {
		t.Fatalf("MarkRead: %v", err)
	}
	if !strings.Contains(requestURI, "msg%2Fabc+1") && !strings.Contains(requestURI, "msg%2Fabc%2B1") {
		t.Fatalf("message id should be escaped in request URI, got %q", requestURI)
	}
}

func TestClaimUsernameEscapesAgentID(t *testing.T) {
	agentID := "agent/with/slashes"
	var requestURI string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requestURI = r.RequestURI
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"username":"u","email":"u@hai.ai","agent_id":"agent/with/slashes"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	if _, err := cl.ClaimUsername(context.Background(), agentID, "u"); err != nil {
		t.Fatalf("ClaimUsername: %v", err)
	}
	if !strings.Contains(requestURI, "/api/v1/agents/agent%2Fwith%2Fslashes/username") {
		t.Fatalf("agent id should be escaped in request URI, got %q", requestURI)
	}
}

func TestRegisterNewAgentWithEndpointBootstrapsWithoutAuthHeader(t *testing.T) {
	var gotAuth string
	var gotBody map[string]interface{}

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/agents/register" {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		gotAuth = r.Header.Get("Authorization")

		body, err := io.ReadAll(r.Body)
		if err != nil {
			t.Fatalf("failed to read body: %v", err)
		}
		if err := json.Unmarshal(body, &gotBody); err != nil {
			t.Fatalf("failed to decode body: %v", err)
		}

		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"agent_id":"agent-123","jacs_id":"jacs-123","dns_verified":false,"signatures":[]}`))
	}))
	defer srv.Close()

	result, err := RegisterNewAgentWithEndpoint(context.Background(), srv.URL, "test-agent", &RegisterNewAgentOptions{
		OwnerEmail:  "owner@hai.ai",
		Domain:      "agent.example",
		Description: "Agent description",
		Quiet:       true,
	})
	if err != nil {
		t.Fatalf("RegisterNewAgentWithEndpoint: %v", err)
	}

	if gotAuth != "" {
		t.Fatalf("expected no Authorization header for bootstrap registration, got %q", gotAuth)
	}
	if gotBody["owner_email"] != "owner@hai.ai" {
		t.Fatalf("expected owner_email in body, got %#v", gotBody["owner_email"])
	}

	rawAgentDoc, _ := gotBody["agent_json"].(string)
	var doc map[string]interface{}
	if err := json.Unmarshal([]byte(rawAgentDoc), &doc); err != nil {
		t.Fatalf("invalid agent_json: %v", err)
	}
	if doc["description"] != "Agent description" {
		t.Fatalf("expected description in agent_json, got %#v", doc["description"])
	}
	if doc["domain"] != "agent.example" {
		t.Fatalf("expected domain in agent_json, got %#v", doc["domain"])
	}

	if result.Registration == nil || result.Registration.AgentID != "agent-123" {
		t.Fatalf("unexpected registration result: %#v", result.Registration)
	}
	if len(result.PrivateKey) == 0 || len(result.PublicKey) == 0 {
		t.Fatal("expected generated key material in result")
	}
}
