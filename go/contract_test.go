package haisdk

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
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

// ---------------------------------------------------------------------------
// Contract tests: deserialization of shared JSON fixtures
// ---------------------------------------------------------------------------

// contractDir returns the absolute path to the shared contract fixtures directory.
func contractDir() string {
	return filepath.Join("..", "contract")
}

func TestContractDeserializeEmailMessage(t *testing.T) {
	data, err := os.ReadFile(filepath.Join(contractDir(), "email_message.json"))
	if err != nil {
		t.Fatalf("read email_message.json: %v", err)
	}

	var msg EmailMessage
	if err := json.Unmarshal(data, &msg); err != nil {
		t.Fatalf("unmarshal EmailMessage: %v", err)
	}

	if msg.ID != "550e8400-e29b-41d4-a716-446655440000" {
		t.Fatalf("ID = %q, want %q", msg.ID, "550e8400-e29b-41d4-a716-446655440000")
	}
	if msg.Direction != "inbound" {
		t.Fatalf("Direction = %q, want %q", msg.Direction, "inbound")
	}
	if msg.FromAddress != "sender@hai.ai" {
		t.Fatalf("FromAddress = %q, want %q", msg.FromAddress, "sender@hai.ai")
	}
	if msg.ToAddress != "recipient@hai.ai" {
		t.Fatalf("ToAddress = %q, want %q", msg.ToAddress, "recipient@hai.ai")
	}
	if msg.Subject != "Test Subject" {
		t.Fatalf("Subject = %q, want %q", msg.Subject, "Test Subject")
	}
	if msg.BodyText != "Hello, this is a test email body." {
		t.Fatalf("BodyText = %q, want %q", msg.BodyText, "Hello, this is a test email body.")
	}
	if msg.MessageID != "<550e8400@hai.ai>" {
		t.Fatalf("MessageID = %q, want %q", msg.MessageID, "<550e8400@hai.ai>")
	}
	if msg.InReplyTo != "" {
		t.Fatalf("InReplyTo = %q, want empty (null in JSON)", msg.InReplyTo)
	}
	if msg.IsRead != false {
		t.Fatalf("IsRead = %v, want false", msg.IsRead)
	}
	if msg.DeliveryStatus != "delivered" {
		t.Fatalf("DeliveryStatus = %q, want %q", msg.DeliveryStatus, "delivered")
	}
	if msg.CreatedAt != "2026-02-24T12:00:00Z" {
		t.Fatalf("CreatedAt = %q, want %q", msg.CreatedAt, "2026-02-24T12:00:00Z")
	}
	if msg.ReadAt != nil {
		t.Fatalf("ReadAt = %v, want nil (null in JSON)", msg.ReadAt)
	}
	if msg.JacsVerified == nil {
		t.Fatal("JacsVerified should not be nil")
	}
	if *msg.JacsVerified != true {
		t.Fatalf("JacsVerified = %v, want true", *msg.JacsVerified)
	}
}

func TestContractDeserializeListMessagesResponse(t *testing.T) {
	data, err := os.ReadFile(filepath.Join(contractDir(), "list_messages_response.json"))
	if err != nil {
		t.Fatalf("read list_messages_response.json: %v", err)
	}

	var resp ListMessagesResponse
	if err := json.Unmarshal(data, &resp); err != nil {
		t.Fatalf("unmarshal ListMessagesResponse: %v", err)
	}

	if len(resp.Messages) != 1 {
		t.Fatalf("len(Messages) = %d, want 1", len(resp.Messages))
	}
	if resp.Total != 1 {
		t.Fatalf("Total = %d, want 1", resp.Total)
	}
	if resp.Unread != 1 {
		t.Fatalf("Unread = %d, want 1", resp.Unread)
	}

	// Verify the nested message matches the same contract values.
	msg := resp.Messages[0]
	if msg.ID != "550e8400-e29b-41d4-a716-446655440000" {
		t.Fatalf("Messages[0].ID = %q, want %q", msg.ID, "550e8400-e29b-41d4-a716-446655440000")
	}
	if msg.Subject != "Test Subject" {
		t.Fatalf("Messages[0].Subject = %q, want %q", msg.Subject, "Test Subject")
	}
	if msg.Direction != "inbound" {
		t.Fatalf("Messages[0].Direction = %q, want %q", msg.Direction, "inbound")
	}
}

func TestContractDeserializeEmailStatus(t *testing.T) {
	data, err := os.ReadFile(filepath.Join(contractDir(), "email_status_response.json"))
	if err != nil {
		t.Fatalf("read email_status_response.json: %v", err)
	}

	var status EmailStatus
	if err := json.Unmarshal(data, &status); err != nil {
		t.Fatalf("unmarshal EmailStatus: %v", err)
	}

	if status.Email != "testbot@hai.ai" {
		t.Fatalf("Email = %q, want %q", status.Email, "testbot@hai.ai")
	}
	if status.Status != "active" {
		t.Fatalf("Status = %q, want %q", status.Status, "active")
	}
	if status.Tier != "new" {
		t.Fatalf("Tier = %q, want %q", status.Tier, "new")
	}
	if status.BillingTier != "free" {
		t.Fatalf("BillingTier = %q, want %q", status.BillingTier, "free")
	}
	if status.MessagesSent24h != 5 {
		t.Fatalf("MessagesSent24h = %d, want 5", status.MessagesSent24h)
	}
	if status.DailyLimit != 10 {
		t.Fatalf("DailyLimit = %d, want 10", status.DailyLimit)
	}
	if status.DailyUsed != 5 {
		t.Fatalf("DailyUsed = %d, want 5", status.DailyUsed)
	}
	if status.ResetsAt != "2026-02-25T00:00:00Z" {
		t.Fatalf("ResetsAt = %q, want %q", status.ResetsAt, "2026-02-25T00:00:00Z")
	}
	if status.MessagesSentTotal != 42 {
		t.Fatalf("MessagesSentTotal = %d, want 42", status.MessagesSentTotal)
	}
	if status.ExternalEnabled != false {
		t.Fatalf("ExternalEnabled = %v, want false", status.ExternalEnabled)
	}
	if status.ExternalSendsToday != 0 {
		t.Fatalf("ExternalSendsToday = %d, want 0", status.ExternalSendsToday)
	}
	if status.LastTierChange != nil {
		t.Fatalf("LastTierChange = %v, want nil", status.LastTierChange)
	}
}

func TestContractDeserializeKeyRegistryResponse(t *testing.T) {
	data, err := os.ReadFile(filepath.Join(contractDir(), "key_registry_response.json"))
	if err != nil {
		t.Fatalf("read key_registry_response.json: %v", err)
	}

	var resp KeyRegistryResponse
	if err := json.Unmarshal(data, &resp); err != nil {
		t.Fatalf("unmarshal KeyRegistryResponse: %v", err)
	}

	if resp.Email != "testbot@hai.ai" {
		t.Fatalf("Email = %q, want %q", resp.Email, "testbot@hai.ai")
	}
	if resp.JacsID != "test-agent-jacs-id" {
		t.Fatalf("JacsID = %q, want %q", resp.JacsID, "test-agent-jacs-id")
	}
	if resp.PublicKey != "MCowBQYDK2VwAyEAExampleBase64PublicKeyData1234567890ABCDEF" {
		t.Fatalf("PublicKey = %q, want %q", resp.PublicKey, "MCowBQYDK2VwAyEAExampleBase64PublicKeyData1234567890ABCDEF")
	}
	if resp.Algorithm != "ed25519" {
		t.Fatalf("Algorithm = %q, want %q", resp.Algorithm, "ed25519")
	}
	if resp.ReputationTier != "new" {
		t.Fatalf("ReputationTier = %q, want %q", resp.ReputationTier, "new")
	}
	if resp.RegisteredAt != "2026-01-15T00:00:00Z" {
		t.Fatalf("RegisteredAt = %q, want %q", resp.RegisteredAt, "2026-01-15T00:00:00Z")
	}
}

func TestContractDeserializeVerificationResult(t *testing.T) {
	data, err := os.ReadFile(filepath.Join(contractDir(), "verification_result.json"))
	if err != nil {
		t.Fatalf("read verification_result.json: %v", err)
	}

	var result EmailVerificationResult
	if err := json.Unmarshal(data, &result); err != nil {
		t.Fatalf("unmarshal EmailVerificationResult: %v", err)
	}

	if result.Valid != true {
		t.Fatalf("Valid = %v, want true", result.Valid)
	}
	if result.JacsID != "test-agent-jacs-id" {
		t.Fatalf("JacsID = %q, want %q", result.JacsID, "test-agent-jacs-id")
	}
	if result.ReputationTier != "established" {
		t.Fatalf("ReputationTier = %q, want %q", result.ReputationTier, "established")
	}
	if result.Error != nil {
		t.Fatalf("Error = %v, want nil", result.Error)
	}
}

// contentHashFixture mirrors the JSON structure in content_hash_example.json.
type contentHashFixture struct {
	Subject          string `json:"subject"`
	Body             string `json:"body"`
	CanonicalInput   string `json:"canonical_input"`
	ExpectedHash     string `json:"expected_hash"`
	Timestamp        int64  `json:"timestamp"`
	SignInputFormat  string `json:"sign_input_format"`
	SignInputExample string `json:"sign_input_example"`
}

func TestContractContentHashComputation(t *testing.T) {
	data, err := os.ReadFile(filepath.Join(contractDir(), "content_hash_example.json"))
	if err != nil {
		t.Fatalf("read content_hash_example.json: %v", err)
	}

	var fixture contentHashFixture
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatalf("unmarshal content_hash_example: %v", err)
	}

	// Verify the canonical input is subject + "\n" + body.
	canonicalInput := fixture.Subject + "\n" + fixture.Body
	if canonicalInput != fixture.CanonicalInput {
		t.Fatalf("canonical input mismatch:\n  got:  %q\n  want: %q", canonicalInput, fixture.CanonicalInput)
	}

	// Compute sha256 of the canonical input and prepend "sha256:".
	h := sha256.Sum256([]byte(canonicalInput))
	contentHash := "sha256:" + hex.EncodeToString(h[:])

	if contentHash != fixture.ExpectedHash {
		t.Fatalf("content hash mismatch:\n  got:  %q\n  want: %q", contentHash, fixture.ExpectedHash)
	}
}

func TestContractSignInputFormat(t *testing.T) {
	data, err := os.ReadFile(filepath.Join(contractDir(), "content_hash_example.json"))
	if err != nil {
		t.Fatalf("read content_hash_example.json: %v", err)
	}

	var fixture contentHashFixture
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatalf("unmarshal content_hash_example: %v", err)
	}

	// Recompute the content hash from subject + body (same as TestContractContentHashComputation).
	h := sha256.Sum256([]byte(fixture.Subject + "\n" + fixture.Body))
	contentHash := "sha256:" + hex.EncodeToString(h[:])

	// Verify sign_input matches "{content_hash}:{timestamp}".
	signInput := fmt.Sprintf("%s:%d", contentHash, fixture.Timestamp)
	if signInput != fixture.SignInputExample {
		t.Fatalf("sign_input mismatch:\n  got:  %q\n  want: %q", signInput, fixture.SignInputExample)
	}
}
