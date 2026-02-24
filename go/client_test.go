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
	direction := "inbound"

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/agents/test-agent-id/email/messages" {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		if r.URL.Query().Get("limit") != "25" || r.URL.Query().Get("offset") != "5" {
			t.Fatalf("unexpected pagination query: %s", r.URL.RawQuery)
		}
		if got := r.URL.Query().Get("direction"); got != direction {
			t.Fatalf("direction query not preserved; got %q", got)
		}
		if strings.Contains(r.URL.RawQuery, " ") {
			t.Fatalf("raw query should be URL-encoded, got %q", r.URL.RawQuery)
		}

		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"messages":[{"id":"m1"}],"total":1,"unread":0}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.ListMessages(context.Background(), ListMessagesOptions{
		Limit:     25,
		Offset:    5,
		Direction: direction,
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

func TestUpdateUsernameEscapesAgentID(t *testing.T) {
	agentID := "agent/with/slashes"
	var requestURI string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requestURI = r.RequestURI
		if r.Method != http.MethodPut {
			t.Fatalf("unexpected method: %s", r.Method)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"username":"new","email":"new@hai.ai","previous_username":"old"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	result, err := cl.UpdateUsername(context.Background(), agentID, "new")
	if err != nil {
		t.Fatalf("UpdateUsername: %v", err)
	}
	if result.Username != "new" || result.PreviousUsername != "old" {
		t.Fatalf("unexpected result: %#v", result)
	}
	if !strings.Contains(requestURI, "/api/v1/agents/agent%2Fwith%2Fslashes/username") {
		t.Fatalf("agent id should be escaped in request URI, got %q", requestURI)
	}
}

func TestDeleteUsernameEscapesAgentID(t *testing.T) {
	agentID := "agent/with/slashes"
	var requestURI string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requestURI = r.RequestURI
		if r.Method != http.MethodDelete {
			t.Fatalf("unexpected method: %s", r.Method)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"released_username":"old","cooldown_until":"2026-03-01T00:00:00Z","message":"released"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	result, err := cl.DeleteUsername(context.Background(), agentID)
	if err != nil {
		t.Fatalf("DeleteUsername: %v", err)
	}
	if result.ReleasedUsername != "old" {
		t.Fatalf("unexpected result: %#v", result)
	}
	if !strings.Contains(requestURI, "/api/v1/agents/agent%2Fwith%2Fslashes/username") {
		t.Fatalf("agent id should be escaped in request URI, got %q", requestURI)
	}
}

func TestVerifyDocumentUsesPublicEndpoint(t *testing.T) {
	var gotAuth string
	var gotBody map[string]any

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/jacs/verify" {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		if r.Method != http.MethodPost {
			t.Fatalf("unexpected method: %s", r.Method)
		}
		gotAuth = r.Header.Get("Authorization")

		body, err := io.ReadAll(r.Body)
		if err != nil {
			t.Fatalf("failed to read body: %v", err)
		}
		if err := json.Unmarshal(body, &gotBody); err != nil {
			t.Fatalf("invalid request body: %v", err)
		}

		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"valid":true,"verified_at":"2026-01-01T00:00:00Z","document_type":"JacsDocument","issuer_verified":true,"signature_verified":true,"signer_id":"agent-1","signed_at":"2026-01-01T00:00:00Z"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	result, err := cl.VerifyDocument(context.Background(), `{"jacsId":"agent-1"}`)
	if err != nil {
		t.Fatalf("VerifyDocument: %v", err)
	}
	if gotAuth != "" {
		t.Fatalf("expected no auth header for public verify endpoint, got %q", gotAuth)
	}
	if gotBody["document"] != `{"jacsId":"agent-1"}` {
		t.Fatalf("unexpected request payload: %#v", gotBody)
	}
	if !result.Valid {
		t.Fatalf("expected valid=true response, got %#v", result)
	}
}

func TestGetVerificationUsesPublicEndpointAndEscapesAgentID(t *testing.T) {
	var gotAuth string
	var gotPath string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotAuth = r.Header.Get("Authorization")
		gotPath = r.URL.EscapedPath()
		if r.Method != http.MethodGet {
			t.Fatalf("unexpected method: %s", r.Method)
		}

		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"agent_id":"agent/with/slash",
			"verification":{
				"jacs_valid":true,
				"dns_valid":true,
				"hai_registered":false,
				"badge":"domain"
			},
			"hai_signatures":["ed25519:abc..."],
			"verified_at":"2026-01-02T00:00:00Z",
			"errors":[]
		}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	result, err := cl.GetVerification(context.Background(), "agent/with/slash")
	if err != nil {
		t.Fatalf("GetVerification: %v", err)
	}
	if gotAuth != "" {
		t.Fatalf("expected no auth header for public verification endpoint, got %q", gotAuth)
	}
	if gotPath != "/api/v1/agents/agent%2Fwith%2Fslash/verification" {
		t.Fatalf("unexpected path: %s", gotPath)
	}
	if result.AgentID != "agent/with/slash" {
		t.Fatalf("unexpected agent id: %#v", result)
	}
	if result.Verification.Badge != "domain" {
		t.Fatalf("unexpected badge: %#v", result.Verification)
	}
}

func TestVerifyAgentDocumentUsesPublicEndpoint(t *testing.T) {
	var gotAuth string
	var gotBody map[string]any

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/agents/verify" {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		if r.Method != http.MethodPost {
			t.Fatalf("unexpected method: %s", r.Method)
		}
		gotAuth = r.Header.Get("Authorization")

		body, err := io.ReadAll(r.Body)
		if err != nil {
			t.Fatalf("failed to read body: %v", err)
		}
		if err := json.Unmarshal(body, &gotBody); err != nil {
			t.Fatalf("invalid request body: %v", err)
		}

		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"agent_id":"agent-1",
			"verification":{
				"jacs_valid":true,
				"dns_valid":true,
				"hai_registered":true,
				"badge":"attested"
			},
			"hai_signatures":["ed25519:def..."],
			"verified_at":"2026-01-02T00:00:00Z",
			"errors":[]
		}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	result, err := cl.VerifyAgentDocument(context.Background(), VerifyAgentDocumentRequest{
		AgentJSON: `{"jacsId":"agent-1"}`,
		Domain:    "example.com",
	})
	if err != nil {
		t.Fatalf("VerifyAgentDocument: %v", err)
	}
	if gotAuth != "" {
		t.Fatalf("expected no auth header for public verify endpoint, got %q", gotAuth)
	}
	if gotBody["agent_json"] != `{"jacsId":"agent-1"}` {
		t.Fatalf("unexpected request payload: %#v", gotBody)
	}
	if gotBody["domain"] != "example.com" {
		t.Fatalf("unexpected domain payload: %#v", gotBody)
	}
	if result.Verification.Badge != "attested" {
		t.Fatalf("unexpected badge: %#v", result.Verification)
	}
}

func TestFetchRemoteKeyParsesCurrentAPIResponseShape(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/jacs/v1/agents/agent-123/keys/latest" {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"jacs_id":"agent-123",
			"version":"latest",
			"public_key":"-----BEGIN PUBLIC KEY-----\nZm9v\n-----END PUBLIC KEY-----\n",
			"public_key_raw_b64":"Zm9v",
			"algorithm":"ed25519",
			"public_key_hash":"sha256:abc"
		}`))
	}))
	defer srv.Close()

	key, err := FetchRemoteKeyFromURL(context.Background(), nil, srv.URL, "agent-123", "latest")
	if err != nil {
		t.Fatalf("FetchRemoteKeyFromURL: %v", err)
	}
	if key.AgentID != "agent-123" {
		t.Fatalf("expected jacs_id to populate AgentID, got %#v", key)
	}
	if string(key.PublicKey) != "foo" {
		t.Fatalf("expected raw key bytes from public_key_raw_b64, got %q", string(key.PublicKey))
	}
}
