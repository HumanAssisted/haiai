package haisdk

import (
	"context"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestSendEmailWithOptionsIncludesJACSSignature(t *testing.T) {
	var gotBody map[string]interface{}

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/agents/test-agent-id/email/send" {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		if r.Method != http.MethodPost {
			t.Fatalf("unexpected method: %s", r.Method)
		}
		body, err := io.ReadAll(r.Body)
		if err != nil {
			t.Fatalf("failed to read body: %v", err)
		}
		if err := json.Unmarshal(body, &gotBody); err != nil {
			t.Fatalf("failed to decode body: %v", err)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"message_id":"msg-1","status":"sent"}`))
	}))
	defer srv.Close()

	cl, pub := newTestClient(t, srv.URL)
	result, err := cl.SendEmailWithOptions(context.Background(), SendEmailOptions{
		To:      "bob@hai.ai",
		Subject: "Hello",
		Body:    "World",
	})
	if err != nil {
		t.Fatalf("SendEmailWithOptions: %v", err)
	}
	if result.MessageID != "msg-1" {
		t.Fatalf("unexpected message_id: %s", result.MessageID)
	}

	// Verify jacs_signature and jacs_timestamp are present in the payload.
	sigB64, ok := gotBody["jacs_signature"].(string)
	if !ok || sigB64 == "" {
		t.Fatalf("expected jacs_signature in body, got %#v", gotBody["jacs_signature"])
	}
	tsFloat, ok := gotBody["jacs_timestamp"].(float64)
	if !ok || tsFloat == 0 {
		t.Fatalf("expected jacs_timestamp in body, got %#v", gotBody["jacs_timestamp"])
	}
	timestamp := int64(tsFloat)

	// Verify the signature is valid using the public key (v2 format includes email).
	h := sha256.Sum256([]byte("Hello\nWorld"))
	contentHash := "sha256:" + hex.EncodeToString(h[:])
	signInput := fmt.Sprintf("%s:%s:%d", contentHash, testAgentEmail, timestamp)

	sigBytes, err := base64.StdEncoding.DecodeString(sigB64)
	if err != nil {
		t.Fatalf("failed to decode signature: %v", err)
	}
	if !ed25519.Verify(pub, []byte(signInput), sigBytes) {
		t.Fatal("JACS content signature verification failed")
	}
}

func TestSendEmailWithOptionsContentHashIsDeterministic(t *testing.T) {
	// Two identical subject+body should produce the same content hash.
	subject := "Subject"
	body := "Body"

	h1 := sha256.Sum256([]byte(subject + "\n" + body))
	h2 := sha256.Sum256([]byte(subject + "\n" + body))

	if h1 != h2 {
		t.Fatal("content hash should be deterministic")
	}
}

func TestSendEmailConvenienceAddsJACSSignature(t *testing.T) {
	var gotBody map[string]interface{}

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		_ = json.Unmarshal(body, &gotBody)
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"message_id":"msg-2","status":"sent"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.SendEmail(context.Background(), "bob@hai.ai", "Hi", "Hey")
	if err != nil {
		t.Fatalf("SendEmail: %v", err)
	}

	if gotBody["jacs_signature"] == nil || gotBody["jacs_signature"] == "" {
		t.Fatal("SendEmail convenience should include jacs_signature")
	}
	if gotBody["jacs_timestamp"] == nil || gotBody["jacs_timestamp"].(float64) == 0 {
		t.Fatal("SendEmail convenience should include jacs_timestamp")
	}
}

func TestGetMessageReturnsEmailMessage(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet {
			t.Fatalf("unexpected method: %s", r.Method)
		}
		if r.URL.Path != "/api/agents/test-agent-id/email/messages/msg-42" {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"id":"msg-42",
			"direction":"inbound",
			"from_address":"alice@hai.ai",
			"to_address":"bob@hai.ai",
			"subject":"Test",
			"body_text":"Hello",
			"is_read":false,
			"delivery_status":"delivered",
			"created_at":"2026-02-24T00:00:00Z"
		}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	msg, err := cl.GetMessage(context.Background(), "msg-42")
	if err != nil {
		t.Fatalf("GetMessage: %v", err)
	}
	if msg.ID != "msg-42" {
		t.Fatalf("unexpected ID: %s", msg.ID)
	}
	if msg.FromAddress != "alice@hai.ai" {
		t.Fatalf("unexpected from: %s", msg.FromAddress)
	}
	if msg.Subject != "Test" {
		t.Fatalf("unexpected subject: %s", msg.Subject)
	}
}

func TestGetMessageEscapesMessageID(t *testing.T) {
	var gotPath string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.EscapedPath()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"id":"msg/special","direction":"inbound","from_address":"a@b","to_address":"c@d","subject":"s","body_text":"b","is_read":false,"delivery_status":"delivered","created_at":"2026-01-01T00:00:00Z"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.GetMessage(context.Background(), "msg/special")
	if err != nil {
		t.Fatalf("GetMessage: %v", err)
	}
	if !strings.Contains(gotPath, "msg%2Fspecial") {
		t.Fatalf("message id should be escaped, got %q", gotPath)
	}
}

func TestDeleteMessageSendsDelete(t *testing.T) {
	var gotMethod string
	var gotPath string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotMethod = r.Method
		gotPath = r.URL.Path
		w.WriteHeader(http.StatusNoContent)
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	err := cl.DeleteMessage(context.Background(), "msg-99")
	if err != nil {
		t.Fatalf("DeleteMessage: %v", err)
	}
	if gotMethod != http.MethodDelete {
		t.Fatalf("expected DELETE, got %s", gotMethod)
	}
	if gotPath != "/api/agents/test-agent-id/email/messages/msg-99" {
		t.Fatalf("unexpected path: %s", gotPath)
	}
}

func TestMarkUnreadSendsPost(t *testing.T) {
	var gotMethod string
	var gotPath string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotMethod = r.Method
		gotPath = r.URL.Path
		w.WriteHeader(http.StatusNoContent)
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	err := cl.MarkUnread(context.Background(), "msg-77")
	if err != nil {
		t.Fatalf("MarkUnread: %v", err)
	}
	if gotMethod != http.MethodPost {
		t.Fatalf("expected POST, got %s", gotMethod)
	}
	if gotPath != "/api/agents/test-agent-id/email/messages/msg-77/unread" {
		t.Fatalf("unexpected path: %s", gotPath)
	}
}

func TestSearchMessagesEncodesQuery(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet {
			t.Fatalf("unexpected method: %s", r.Method)
		}
		q := r.URL.Query()
		if q.Get("q") != "important" {
			t.Fatalf("unexpected q param: %s", q.Get("q"))
		}
		if q.Get("direction") != "inbound" {
			t.Fatalf("unexpected direction param: %s", q.Get("direction"))
		}
		if q.Get("from_address") != "alice@hai.ai" {
			t.Fatalf("unexpected from_address param: %s", q.Get("from_address"))
		}
		if q.Get("limit") != "10" {
			t.Fatalf("unexpected limit param: %s", q.Get("limit"))
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"messages":[{"id":"msg-1","direction":"inbound","from_address":"alice@hai.ai","to_address":"bob@hai.ai","subject":"Important","body_text":"content","is_read":false,"delivery_status":"delivered","created_at":"2026-01-01T00:00:00Z"}],"total":1,"unread":1}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	msgs, err := cl.SearchMessages(context.Background(), SearchOptions{
		Q:          "important",
		Direction:  "inbound",
		FromAddress: "alice@hai.ai",
		Limit:      10,
	})
	if err != nil {
		t.Fatalf("SearchMessages: %v", err)
	}
	if len(msgs) != 1 {
		t.Fatalf("expected 1 message, got %d", len(msgs))
	}
	if msgs[0].ID != "msg-1" {
		t.Fatalf("unexpected message ID: %s", msgs[0].ID)
	}
}

func TestSearchMessagesOmitsEmptyParams(t *testing.T) {
	var gotQuery string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotQuery = r.URL.RawQuery
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"messages":[],"total":0,"unread":0}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.SearchMessages(context.Background(), SearchOptions{
		Q: "test",
	})
	if err != nil {
		t.Fatalf("SearchMessages: %v", err)
	}
	// Only q param should be present, not direction/from_address/to_address/limit/offset.
	if strings.Contains(gotQuery, "direction=") {
		t.Fatalf("empty direction should not be in query: %s", gotQuery)
	}
	if strings.Contains(gotQuery, "from_address=") {
		t.Fatalf("empty from_address should not be in query: %s", gotQuery)
	}
}

func TestGetUnreadCountReturnsCount(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet {
			t.Fatalf("unexpected method: %s", r.Method)
		}
		if r.URL.Path != "/api/agents/test-agent-id/email/unread-count" {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"count":7}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	count, err := cl.GetUnreadCount(context.Background())
	if err != nil {
		t.Fatalf("GetUnreadCount: %v", err)
	}
	if count != 7 {
		t.Fatalf("expected 7, got %d", count)
	}
}

func TestReplyFetchesOriginalAndSends(t *testing.T) {
	calls := 0
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		switch {
		case r.Method == http.MethodGet && strings.HasSuffix(r.URL.Path, "/email/messages/msg-orig"):
			// GetMessage call
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{
				"id":"msg-orig",
				"direction":"inbound",
				"from_address":"alice@hai.ai",
				"to_address":"bob@hai.ai",
				"subject":"Original Subject",
				"body_text":"Original body",
				"is_read":false,
				"delivery_status":"delivered",
				"created_at":"2026-02-24T00:00:00Z"
			}`))
		case r.Method == http.MethodPost && strings.HasSuffix(r.URL.Path, "/email/send"):
			// SendEmailWithOptions call
			body, _ := io.ReadAll(r.Body)
			var payload map[string]interface{}
			_ = json.Unmarshal(body, &payload)
			if payload["to"] != "alice@hai.ai" {
				t.Fatalf("reply should be sent to original sender, got %v", payload["to"])
			}
			if payload["subject"] != "Re: Original Subject" {
				t.Fatalf("reply subject should be prefixed, got %v", payload["subject"])
			}
			if payload["in_reply_to"] != "msg-orig" {
				t.Fatalf("in_reply_to should be set, got %v", payload["in_reply_to"])
			}
			if payload["body"] != "Reply body" {
				t.Fatalf("unexpected body: %v", payload["body"])
			}
			if payload["jacs_signature"] == nil || payload["jacs_signature"] == "" {
				t.Fatal("reply should include jacs_signature")
			}
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"message_id":"msg-reply","status":"sent"}`))
		default:
			t.Fatalf("unexpected request: %s %s", r.Method, r.URL.Path)
		}
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	result, err := cl.Reply(context.Background(), "msg-orig", "Reply body", "")
	if err != nil {
		t.Fatalf("Reply: %v", err)
	}
	if result.MessageID != "msg-reply" {
		t.Fatalf("unexpected message_id: %s", result.MessageID)
	}
	if calls != 2 {
		t.Fatalf("expected 2 HTTP calls (get + send), got %d", calls)
	}
}

func TestReplyUsesSubjectOverride(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.Method == http.MethodGet:
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"id":"msg-1","direction":"inbound","from_address":"a@b","to_address":"c@d","subject":"Old","body_text":"b","is_read":false,"delivery_status":"delivered","created_at":"2026-01-01T00:00:00Z"}`))
		case r.Method == http.MethodPost:
			body, _ := io.ReadAll(r.Body)
			var payload map[string]interface{}
			_ = json.Unmarshal(body, &payload)
			if payload["subject"] != "Custom Override" {
				t.Fatalf("should use subject override, got %v", payload["subject"])
			}
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"message_id":"msg-r","status":"sent"}`))
		}
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.Reply(context.Background(), "msg-1", "body", "Custom Override")
	if err != nil {
		t.Fatalf("Reply: %v", err)
	}
}

func TestReplyUsesRFC5322MessageIDForThreading(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.Method == http.MethodGet:
			w.Header().Set("Content-Type", "application/json")
			// Original message has an RFC 5322 Message-ID.
			_, _ = w.Write([]byte(`{
				"id":"db-uuid-123",
				"direction":"inbound",
				"from_address":"alice@hai.ai",
				"to_address":"bob@hai.ai",
				"subject":"Thread test",
				"body_text":"body",
				"message_id":"<abc123.alice@hai.ai>",
				"is_read":false,
				"delivery_status":"delivered",
				"created_at":"2026-02-24T00:00:00Z"
			}`))
		case r.Method == http.MethodPost:
			body, _ := io.ReadAll(r.Body)
			var payload map[string]interface{}
			_ = json.Unmarshal(body, &payload)
			// Should use the RFC 5322 Message-ID, not the database UUID.
			if payload["in_reply_to"] != "<abc123.alice@hai.ai>" {
				t.Fatalf("in_reply_to should use RFC 5322 Message-ID, got %v", payload["in_reply_to"])
			}
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"message_id":"msg-reply","status":"sent"}`))
		}
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	result, err := cl.Reply(context.Background(), "db-uuid-123", "Reply body", "")
	if err != nil {
		t.Fatalf("Reply: %v", err)
	}
	if result.MessageID != "msg-reply" {
		t.Fatalf("unexpected message_id: %s", result.MessageID)
	}
}

func TestReplyDoesNotDoublePrefixRe(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.Method == http.MethodGet:
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"id":"msg-1","direction":"inbound","from_address":"a@b","to_address":"c@d","subject":"Re: Already replied","body_text":"b","is_read":false,"delivery_status":"delivered","created_at":"2026-01-01T00:00:00Z"}`))
		case r.Method == http.MethodPost:
			body, _ := io.ReadAll(r.Body)
			var payload map[string]interface{}
			_ = json.Unmarshal(body, &payload)
			if payload["subject"] != "Re: Already replied" {
				t.Fatalf("should not double-prefix Re:, got %v", payload["subject"])
			}
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"message_id":"msg-r","status":"sent"}`))
		}
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.Reply(context.Background(), "msg-1", "body", "")
	if err != nil {
		t.Fatalf("Reply: %v", err)
	}
}

func TestSendEmailReturnsErrEmailNotActive(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusForbidden)
		_, _ = w.Write([]byte(`{"message":"Email not provisioned for this agent","error_code":"EMAIL_NOT_ACTIVE"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.SendEmail(context.Background(), "bob@hai.ai", "Hi", "Hello")
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !errors.Is(err, ErrEmailNotActive) {
		t.Fatalf("expected ErrEmailNotActive, got: %v", err)
	}
	if !strings.Contains(err.Error(), "Email not provisioned") {
		t.Fatalf("error should contain API message, got: %v", err)
	}
}

func TestSendEmailReturnsErrRecipientNotFound(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusNotFound)
		_, _ = w.Write([]byte(`{"message":"No agent found with email unknown@hai.ai","error_code":"RECIPIENT_NOT_FOUND"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.SendEmail(context.Background(), "unknown@hai.ai", "Hi", "Hello")
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !errors.Is(err, ErrRecipientNotFound) {
		t.Fatalf("expected ErrRecipientNotFound, got: %v", err)
	}
}

func TestSendEmailReturnsErrEmailRateLimited(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusTooManyRequests)
		_, _ = w.Write([]byte(`{"message":"Daily send limit exceeded","error_code":"RATE_LIMITED"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.SendEmail(context.Background(), "bob@hai.ai", "Hi", "Hello")
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	if !errors.Is(err, ErrEmailRateLimited) {
		t.Fatalf("expected ErrEmailRateLimited, got: %v", err)
	}
}

func TestSendEmailReturnsHaiAPIErrorForUnknownCode(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusBadRequest)
		_, _ = w.Write([]byte(`{"message":"Something weird happened","error_code":"UNKNOWN_CODE"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.SendEmail(context.Background(), "bob@hai.ai", "Hi", "Hello")
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	var apiErr *HaiAPIError
	if !errors.As(err, &apiErr) {
		t.Fatalf("expected *HaiAPIError, got: %T %v", err, err)
	}
	if apiErr.ErrorCode != "UNKNOWN_CODE" {
		t.Fatalf("unexpected error code: %s", apiErr.ErrorCode)
	}
	if apiErr.Status != http.StatusBadRequest {
		t.Fatalf("unexpected status: %d", apiErr.Status)
	}
}

func TestSendEmailFallsBackToGenericErrorForUnstructuredResponse(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
		_, _ = w.Write([]byte(`Internal Server Error`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.SendEmail(context.Background(), "bob@hai.ai", "Hi", "Hello")
	if err == nil {
		t.Fatal("expected error, got nil")
	}
	// Should fall back to classifyHTTPError, returning *Error.
	var sdkErr *Error
	if !errors.As(err, &sdkErr) {
		t.Fatalf("expected *Error for unstructured response, got: %T %v", err, err)
	}
}

// ---------------------------------------------------------------------------
// Content hash tests — exercise the extracted computeContentHash function
// ---------------------------------------------------------------------------

func TestContentHashNoAttachments(t *testing.T) {
	got := computeContentHash("Subject", "Body", nil)
	// sha256("Subject\nBody")
	want := "sha256:ee0c09adcdb487b1c64d7e63dfeaf7eabc1fe5ffc7f6182b0541061e2610bb25"
	if got != want {
		t.Fatalf("content hash mismatch:\n  got:  %s\n  want: %s", got, want)
	}
}

func TestContentHashWithOneAttachment(t *testing.T) {
	att := EmailAttachment{
		Filename:    "a.txt",
		ContentType: "text/plain",
		Data:        []byte("hello"),
	}
	got := computeContentHash("Test", "Hello", []EmailAttachment{att})
	// att_hash = sha256("a.txt:text/plain:hello")
	// expected = sha256("Test\nHello\n" + att_hash)
	want := "sha256:4160c4723a01eb96bd05f707b78790deb16fd92f1ad0362bf5d4881d5ba2f0c7"
	if got != want {
		t.Fatalf("content hash mismatch:\n  got:  %s\n  want: %s", got, want)
	}
}

func TestContentHashAttachmentOrderIndependent(t *testing.T) {
	a := EmailAttachment{Filename: "a.txt", ContentType: "text/plain", Data: []byte("alpha")}
	b := EmailAttachment{Filename: "b.txt", ContentType: "text/plain", Data: []byte("beta")}

	hashAB := computeContentHash("Subject", "Body", []EmailAttachment{a, b})
	hashBA := computeContentHash("Subject", "Body", []EmailAttachment{b, a})

	if hashAB != hashBA {
		t.Fatalf("attachment order should not affect hash:\n  [a,b]: %s\n  [b,a]: %s", hashAB, hashBA)
	}

	// Also verify against the known golden value.
	want := "sha256:0ee8bf6bd5490c7a907d748e4012453226dea38583767e0126b2a26376ea9568"
	if hashAB != want {
		t.Fatalf("content hash mismatch:\n  got:  %s\n  want: %s", hashAB, want)
	}
}

func TestContentHashWithThreeAttachments(t *testing.T) {
	a := EmailAttachment{Filename: "a.txt", ContentType: "text/plain", Data: []byte("alpha")}
	b := EmailAttachment{Filename: "b.txt", ContentType: "text/plain", Data: []byte("beta")}
	c := EmailAttachment{Filename: "c.txt", ContentType: "text/plain", Data: []byte("gamma")}

	got := computeContentHash("Subject", "Body", []EmailAttachment{a, b, c})
	want := "sha256:8a2933b3b884212fb47655c51fd1d0490ebaa941ece2d2bde6cdc8423921a67a"
	if got != want {
		t.Fatalf("content hash mismatch:\n  got:  %s\n  want: %s", got, want)
	}

	// Order permutation should produce the same hash.
	got2 := computeContentHash("Subject", "Body", []EmailAttachment{c, a, b})
	if got != got2 {
		t.Fatalf("three-attachment hash should be order-independent:\n  [a,b,c]: %s\n  [c,a,b]: %s", got, got2)
	}
}

func TestContentHashAttachmentTamperDetected(t *testing.T) {
	original := EmailAttachment{Filename: "a.txt", ContentType: "text/plain", Data: []byte("alpha")}
	tampered := EmailAttachment{Filename: "a.txt", ContentType: "text/plain", Data: []byte("TAMPERED")}

	hashOriginal := computeContentHash("Subject", "Body", []EmailAttachment{original})
	hashTampered := computeContentHash("Subject", "Body", []EmailAttachment{tampered})

	if hashOriginal == hashTampered {
		t.Fatal("changing attachment data MUST produce a different hash")
	}
}

func TestContentHashContentTypeChange(t *testing.T) {
	plain := EmailAttachment{Filename: "a.txt", ContentType: "text/plain", Data: []byte("alpha")}
	octet := EmailAttachment{Filename: "a.txt", ContentType: "application/octet-stream", Data: []byte("alpha")}

	hashPlain := computeContentHash("Subject", "Body", []EmailAttachment{plain})
	hashOctet := computeContentHash("Subject", "Body", []EmailAttachment{octet})

	if hashPlain == hashOctet {
		t.Fatal("changing content_type MUST produce a different hash")
	}
}

func TestContentHashEmptyAttachmentData(t *testing.T) {
	att := EmailAttachment{Filename: "empty.txt", ContentType: "text/plain", Data: []byte{}}
	hashWithEmpty := computeContentHash("Subject", "Body", []EmailAttachment{att})
	hashNoAtt := computeContentHash("Subject", "Body", nil)

	// An empty attachment is still an attachment; the hash must differ from no-attachment.
	if hashWithEmpty == hashNoAtt {
		t.Fatal("empty-data attachment should produce a different hash than no attachments")
	}

	// Verify against golden value.
	want := "sha256:a51415d203b1f74ce4eb2e00edca52fec0c4371ceab2530bf6a53a0ba1f24718"
	if hashWithEmpty != want {
		t.Fatalf("content hash mismatch:\n  got:  %s\n  want: %s", hashWithEmpty, want)
	}
}

// ---------------------------------------------------------------------------
// Signing payload format tests
// ---------------------------------------------------------------------------

func TestV2SigningPayloadFormat(t *testing.T) {
	// v2 format: "{contentHash}:{email}:{timestamp}"
	contentHash := computeContentHash("Hello", "World", nil)
	email := "agent@hai.ai"
	timestamp := int64(1700000000)

	signInput := fmt.Sprintf("%s:%s:%d", contentHash, email, timestamp)

	// Verify all three components are present and correctly ordered.
	parts := strings.SplitN(signInput, ":", 4)
	// parts: ["sha256", "<hex>", "<email>", "<timestamp>"]
	// But the content hash itself contains "sha256:", so split on last two colons.
	if !strings.HasPrefix(signInput, "sha256:") {
		t.Fatalf("sign input should start with 'sha256:', got %q", signInput)
	}
	if !strings.Contains(signInput, ":"+email+":") {
		t.Fatalf("sign input should contain ':%s:', got %q", email, signInput)
	}
	if !strings.HasSuffix(signInput, ":1700000000") {
		t.Fatalf("sign input should end with ':1700000000', got %q", signInput)
	}

	// Verify exact expected format.
	want := contentHash + ":" + email + ":1700000000"
	if signInput != want {
		t.Fatalf("v2 sign input mismatch:\n  got:  %s\n  want: %s", signInput, want)
	}

	// Verify it has exactly the format with 4 colon separators total:
	// sha256:<hex>:<email>:<timestamp>
	if len(parts) != 4 {
		t.Fatalf("expected 4 colon-delimited parts, got %d: %v", len(parts), parts)
	}
}

func TestV1SigningPayloadFormat(t *testing.T) {
	// v1 format (legacy): "{contentHash}:{timestamp}" — no email component.
	contentHash := computeContentHash("Hello", "World", nil)
	timestamp := int64(1700000000)

	signInput := fmt.Sprintf("%s:%d", contentHash, timestamp)

	want := contentHash + ":1700000000"
	if signInput != want {
		t.Fatalf("v1 sign input mismatch:\n  got:  %s\n  want: %s", signInput, want)
	}

	// v1 should have exactly 3 colon-delimited parts: sha256, hex, timestamp.
	parts := strings.SplitN(signInput, ":", 4)
	if len(parts) != 3 {
		t.Fatalf("v1 should have 3 colon-delimited parts, got %d: %v", len(parts), parts)
	}
}

// ---------------------------------------------------------------------------
// Guard tests
// ---------------------------------------------------------------------------

func TestSendEmailErrorsWhenAgentEmailEmpty(t *testing.T) {
	pub, priv, err := GenerateKeyPair()
	if err != nil {
		t.Fatalf("GenerateKeyPair: %v", err)
	}
	_ = pub

	cl, err := NewClient(
		WithEndpoint("http://localhost:9999"),
		WithJACSID("test-agent-id"),
		WithPrivateKey(priv),
	)
	if err != nil {
		t.Fatalf("NewClient: %v", err)
	}
	// Do NOT set agentEmail.

	_, sendErr := cl.SendEmailWithOptions(context.Background(), SendEmailOptions{
		To:      "bob@hai.ai",
		Subject: "Hi",
		Body:    "Hello",
	})
	if sendErr == nil {
		t.Fatal("expected error when agentEmail is empty")
	}
	if !strings.Contains(sendErr.Error(), "agent email not set") {
		t.Fatalf("error should mention 'agent email not set', got: %v", sendErr)
	}
	if !errors.Is(sendErr, ErrEmailNotActive) {
		t.Fatalf("error should wrap ErrEmailNotActive, got: %v", sendErr)
	}
}

// ---------------------------------------------------------------------------
// Serialization tests
// ---------------------------------------------------------------------------

func TestAttachmentDataExcludedFromJSON(t *testing.T) {
	att := EmailAttachment{
		Filename:    "doc.pdf",
		ContentType: "application/pdf",
		Data:        []byte("secret-binary-data"),
	}

	data, err := json.Marshal(att)
	if err != nil {
		t.Fatalf("Marshal: %v", err)
	}

	jsonStr := string(data)

	// The raw Data field has json:"-" so it must not appear.
	if strings.Contains(jsonStr, "secret-binary-data") {
		t.Fatalf("raw Data bytes should not appear in JSON, got: %s", jsonStr)
	}

	// DataBase64 was empty, so with omitempty it should also be absent.
	if strings.Contains(jsonStr, "data_base64") {
		t.Fatalf("data_base64 should be omitted when empty, got: %s", jsonStr)
	}

	// Filename and content_type should be present.
	if !strings.Contains(jsonStr, `"filename":"doc.pdf"`) {
		t.Fatalf("filename should be in JSON, got: %s", jsonStr)
	}
	if !strings.Contains(jsonStr, `"content_type":"application/pdf"`) {
		t.Fatalf("content_type should be in JSON, got: %s", jsonStr)
	}
}

func TestAttachmentDataBase64InJSON(t *testing.T) {
	att := EmailAttachment{
		Filename:    "doc.pdf",
		ContentType: "application/pdf",
		DataBase64:  "cGRmLWNvbnRlbnQ=",
	}

	data, err := json.Marshal(att)
	if err != nil {
		t.Fatalf("Marshal: %v", err)
	}

	jsonStr := string(data)

	if !strings.Contains(jsonStr, `"data_base64":"cGRmLWNvbnRlbnQ="`) {
		t.Fatalf("data_base64 should appear in JSON when set, got: %s", jsonStr)
	}
}

// ---------------------------------------------------------------------------
// Cross-SDK golden value test
// ---------------------------------------------------------------------------

func TestCrossSDKGoldenHash(t *testing.T) {
	// Fixed inputs shared across all SDK implementations.
	subject := "Cross-SDK Test"
	body := "Verify me"
	attachments := []EmailAttachment{
		{Filename: "doc.pdf", ContentType: "application/pdf", Data: []byte("pdf-content")},
		{Filename: "img.png", ContentType: "image/png", Data: []byte("png-content")},
	}

	got := computeContentHash(subject, body, attachments)

	// Golden value computed independently:
	//   att1_hash = sha256("doc.pdf:application/pdf:pdf-content")
	//            = 529fcac3033bb5ced0ae558dafac6b1dc4b87818ac08dc16d877f000dceb608d
	//   att2_hash = sha256("img.png:image/png:png-content")
	//            = 64eb9ccbf4de85131d75c1a6d79cafee46684bec3f0ba4c8c044feb8aa43c706
	//   sorted:  [att1_hash, att2_hash]  (att1 < att2 lexicographically)
	//   content  = "Cross-SDK Test\nVerify me\n" + att1_hash + "\n" + att2_hash
	//   hash     = sha256(content)
	want := "sha256:a0222afb5f569cb89efd21f2bebdcf84e97c4c98cb31cb5ff54e6e4a2b88c8a1"

	if got != want {
		t.Fatalf("cross-SDK golden hash mismatch:\n  got:  %s\n  want: %s\n\n"+
			"If this test fails after code changes, the content hash algorithm\n"+
			"has diverged from other SDKs (Python, Node, Rust). All SDKs must\n"+
			"produce identical hashes for the same inputs.", got, want)
	}

	// Verify the individual attachment hashes for debuggability.
	h1 := sha256.Sum256([]byte("doc.pdf:application/pdf:pdf-content"))
	att1Hash := hex.EncodeToString(h1[:])
	if att1Hash != "529fcac3033bb5ced0ae558dafac6b1dc4b87818ac08dc16d877f000dceb608d" {
		t.Fatalf("doc.pdf attachment hash mismatch: %s", att1Hash)
	}

	h2 := sha256.Sum256([]byte("img.png:image/png:png-content"))
	att2Hash := hex.EncodeToString(h2[:])
	if att2Hash != "64eb9ccbf4de85131d75c1a6d79cafee46684bec3f0ba4c8c044feb8aa43c706" {
		t.Fatalf("img.png attachment hash mismatch: %s", att2Hash)
	}
}
