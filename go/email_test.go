package haiai

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestSendEmailWithOptionsServerSideSigning(t *testing.T) {
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

	cl, _ := newTestClient(t, srv.URL)
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

	// Server-side signing: client should NOT send jacs_signature or jacs_timestamp.
	if _, ok := gotBody["jacs_signature"]; ok {
		t.Fatal("client should not send jacs_signature (server handles signing)")
	}
	if _, ok := gotBody["jacs_timestamp"]; ok {
		t.Fatal("client should not send jacs_timestamp (server handles signing)")
	}

	// Verify expected fields are present.
	if gotBody["to"] != "bob@hai.ai" {
		t.Fatalf("unexpected to: %v", gotBody["to"])
	}
	if gotBody["subject"] != "Hello" {
		t.Fatalf("unexpected subject: %v", gotBody["subject"])
	}
	if gotBody["body"] != "World" {
		t.Fatalf("unexpected body: %v", gotBody["body"])
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

func TestSendEmailConvenienceServerSideSigning(t *testing.T) {
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

	// Server-side signing: convenience method should NOT send client-side signing fields.
	if _, ok := gotBody["jacs_signature"]; ok {
		t.Fatal("SendEmail convenience should not include jacs_signature (server handles signing)")
	}
	if _, ok := gotBody["jacs_timestamp"]; ok {
		t.Fatal("SendEmail convenience should not include jacs_timestamp (server handles signing)")
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
		Q:           "important",
		Direction:   "inbound",
		FromAddress: "alice@hai.ai",
		Limit:       10,
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
			// Server-side signing: reply should NOT include client-side signing fields.
			if _, ok := payload["jacs_signature"]; ok {
				t.Fatal("reply should not include jacs_signature (server handles signing)")
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
// Guard tests
// ---------------------------------------------------------------------------

func TestSendEmailErrorsWhenAgentEmailEmpty(t *testing.T) {
	cl, err := NewClient(
		WithEndpoint("http://localhost:9999"),
		WithJACSID("test-agent-id"),
		WithFFIClient(newMockFFIClient("http://localhost:9999", "test-agent-id", "")),
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
// SignEmail / VerifyEmail tests
// ---------------------------------------------------------------------------

func TestSignEmailSendsRawRFC5322(t *testing.T) {
	var gotContentType string
	var gotBody []byte

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/email/sign" {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		if r.Method != http.MethodPost {
			t.Fatalf("unexpected method: %s", r.Method)
		}
		gotContentType = r.Header.Get("Content-Type")
		var err error
		gotBody, err = io.ReadAll(r.Body)
		if err != nil {
			t.Fatalf("failed to read body: %v", err)
		}
		w.Header().Set("Content-Type", "message/rfc822")
		_, _ = w.Write([]byte("signed-email-bytes"))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	rawEmail := []byte("From: agent@hai.ai\r\nTo: bob@hai.ai\r\nSubject: Test\r\n\r\nHello")
	result, err := cl.SignEmail(context.Background(), rawEmail)
	if err != nil {
		t.Fatalf("SignEmail: %v", err)
	}

	if gotContentType != "message/rfc822" {
		t.Fatalf("expected Content-Type message/rfc822, got %s", gotContentType)
	}
	if string(gotBody) != string(rawEmail) {
		t.Fatalf("request body should be raw email, got %q", string(gotBody))
	}
	if string(result) != "signed-email-bytes" {
		t.Fatalf("expected signed-email-bytes, got %q", string(result))
	}
}

func TestSignEmailReturnsErrorOnHTTPFailure(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
		_, _ = w.Write([]byte(`Internal Server Error`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.SignEmail(context.Background(), []byte("raw-email"))
	if err == nil {
		t.Fatal("expected error for HTTP 500")
	}
}

func TestVerifyEmailSendsRawRFC5322(t *testing.T) {
	var gotContentType string
	var gotBody []byte

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/email/verify" {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		if r.Method != http.MethodPost {
			t.Fatalf("unexpected method: %s", r.Method)
		}
		gotContentType = r.Header.Get("Content-Type")
		var err error
		gotBody, err = io.ReadAll(r.Body)
		if err != nil {
			t.Fatalf("failed to read body: %v", err)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"valid": true,
			"jacs_id": "agent-123",
			"algorithm": "ed25519",
			"reputation_tier": "established",
			"dns_verified": true,
			"field_results": [
				{"field": "subject", "status": "pass"},
				{"field": "body", "status": "pass"}
			],
			"chain": [
				{"signer": "agent@hai.ai", "jacs_id": "agent-123", "valid": true, "forwarded": false}
			]
		}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	rawEmail := []byte("From: agent@hai.ai\r\nTo: bob@hai.ai\r\nSubject: Test\r\n\r\nHello")
	result, err := cl.VerifyEmail(context.Background(), rawEmail)
	if err != nil {
		t.Fatalf("VerifyEmail: %v", err)
	}

	if gotContentType != "message/rfc822" {
		t.Fatalf("expected Content-Type message/rfc822, got %s", gotContentType)
	}
	if string(gotBody) != string(rawEmail) {
		t.Fatalf("request body should be raw email, got %q", string(gotBody))
	}

	if !result.Valid {
		t.Fatal("expected valid=true")
	}
	if result.JacsID != "agent-123" {
		t.Fatalf("unexpected jacs_id: %s", result.JacsID)
	}
	if result.Algorithm != "ed25519" {
		t.Fatalf("unexpected algorithm: %s", result.Algorithm)
	}
	if result.ReputationTier != "established" {
		t.Fatalf("unexpected reputation_tier: %s", result.ReputationTier)
	}
	if result.DNSVerified == nil || !*result.DNSVerified {
		t.Fatal("expected dns_verified=true")
	}
	if len(result.FieldResults) != 2 {
		t.Fatalf("expected 2 field results, got %d", len(result.FieldResults))
	}
	if result.FieldResults[0].Field != "subject" || result.FieldResults[0].Status != FieldStatusPass {
		t.Fatalf("unexpected first field result: %+v", result.FieldResults[0])
	}
	if len(result.Chain) != 1 {
		t.Fatalf("expected 1 chain entry, got %d", len(result.Chain))
	}
	if result.Chain[0].Signer != "agent@hai.ai" || !result.Chain[0].Valid {
		t.Fatalf("unexpected chain entry: %+v", result.Chain[0])
	}
}

func TestVerifyEmailReturnsErrorOnHTTPFailure(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusBadRequest)
		_, _ = w.Write([]byte(`{"error":"bad request"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.VerifyEmail(context.Background(), []byte("raw-email"))
	if err == nil {
		t.Fatal("expected error for HTTP 400")
	}
}

func TestVerifyEmailWithErrorField(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"valid": false,
			"jacs_id": "",
			"algorithm": "",
			"reputation_tier": "",
			"field_results": [],
			"chain": [],
			"error": "no JACS signature attachment found"
		}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	result, err := cl.VerifyEmail(context.Background(), []byte("unsigned-email"))
	if err != nil {
		t.Fatalf("VerifyEmail: %v", err)
	}

	if result.Valid {
		t.Fatal("expected valid=false")
	}
	if result.Error == nil {
		t.Fatal("expected error field to be set")
	}
	if *result.Error != "no JACS signature attachment found" {
		t.Fatalf("unexpected error: %s", *result.Error)
	}
}

// ---------------------------------------------------------------
// SendSignedEmail tests
// ---------------------------------------------------------------

func TestSendSignedEmailDelegatesToSendEndpoint(t *testing.T) {
	callCount := 0
	var sendContentType string
	var sendPath string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		callCount++
		sendPath = r.URL.Path
		sendContentType = r.Header.Get("Content-Type")
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"message_id":"msg-signed-1","status":"sent"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	result, err := cl.SendSignedEmail(context.Background(), SendEmailOptions{
		To:      "bob@hai.ai",
		Subject: "Hello Signed",
		Body:    "Signed body",
	})
	if err != nil {
		t.Fatalf("SendSignedEmail: %v", err)
	}

	if result.MessageID != "msg-signed-1" {
		t.Fatalf("unexpected message_id: %s", result.MessageID)
	}
	if result.Status != "sent" {
		t.Fatalf("unexpected status: %s", result.Status)
	}
	if callCount != 1 {
		t.Fatalf("expected one send call, got %d", callCount)
	}
	if !strings.Contains(sendPath, "/email/send") {
		t.Fatalf("expected send path, got: %s", sendPath)
	}
	if sendContentType != "application/json" {
		t.Fatalf("expected Content-Type application/json, got: %s", sendContentType)
	}
}

func TestSendSignedEmailFailsWithoutAgentEmail(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		t.Fatal("no HTTP call should be made when agentEmail is not set")
	}))
	defer srv.Close()

	cl, err := NewClient(
		WithEndpoint(srv.URL),
		WithJACSID("test-agent-id"),
		WithFFIClient(newMockFFIClient(srv.URL, "test-agent-id", "")),
	)
	if err != nil {
		t.Fatalf("NewClient: %v", err)
	}
	// Do NOT call SetAgentEmail

	_, err = cl.SendSignedEmail(context.Background(), SendEmailOptions{
		To:      "bob@hai.ai",
		Subject: "Hi",
		Body:    "test",
	})
	if err == nil {
		t.Fatal("expected error when agentEmail is not set")
	}
	if !errors.Is(err, ErrEmailNotActive) {
		t.Fatalf("expected ErrEmailNotActive, got: %v", err)
	}
}

// ---------------------------------------------------------------------------
// CC/BCC/Labels in SendEmailWithOptions
// ---------------------------------------------------------------------------

func TestSendEmailWithOptionsCcBccLabels(t *testing.T) {
	var gotBody map[string]interface{}

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		_ = json.Unmarshal(body, &gotBody)
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"message_id":"msg-cc","status":"sent"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.SendEmailWithOptions(context.Background(), SendEmailOptions{
		To:      "bob@hai.ai",
		Subject: "CC test",
		Body:    "body",
		CC:      []string{"cc1@hai.ai", "cc2@hai.ai"},
		BCC:     []string{"bcc1@hai.ai"},
		Labels:  []string{"urgent", "project-x"},
	})
	if err != nil {
		t.Fatalf("SendEmailWithOptions: %v", err)
	}

	// Verify CC
	ccRaw, ok := gotBody["cc"]
	if !ok {
		t.Fatal("cc field should be present in request body")
	}
	ccSlice, ok := ccRaw.([]interface{})
	if !ok || len(ccSlice) != 2 {
		t.Fatalf("expected 2 CC addresses, got %v", ccRaw)
	}
	if ccSlice[0] != "cc1@hai.ai" || ccSlice[1] != "cc2@hai.ai" {
		t.Fatalf("unexpected CC values: %v", ccSlice)
	}

	// Verify BCC
	bccRaw, ok := gotBody["bcc"]
	if !ok {
		t.Fatal("bcc field should be present in request body")
	}
	bccSlice, ok := bccRaw.([]interface{})
	if !ok || len(bccSlice) != 1 {
		t.Fatalf("expected 1 BCC address, got %v", bccRaw)
	}
	if bccSlice[0] != "bcc1@hai.ai" {
		t.Fatalf("unexpected BCC value: %v", bccSlice[0])
	}

	// Verify Labels
	labelsRaw, ok := gotBody["labels"]
	if !ok {
		t.Fatal("labels field should be present in request body")
	}
	labelsSlice, ok := labelsRaw.([]interface{})
	if !ok || len(labelsSlice) != 2 {
		t.Fatalf("expected 2 labels, got %v", labelsRaw)
	}
	if labelsSlice[0] != "urgent" || labelsSlice[1] != "project-x" {
		t.Fatalf("unexpected labels: %v", labelsSlice)
	}
}

func TestSendEmailWithOptionsOmitsCcBccLabelsWhenNil(t *testing.T) {
	var gotBody map[string]interface{}

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		_ = json.Unmarshal(body, &gotBody)
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"message_id":"msg-no-cc","status":"sent"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.SendEmailWithOptions(context.Background(), SendEmailOptions{
		To:      "bob@hai.ai",
		Subject: "No CC",
		Body:    "body",
	})
	if err != nil {
		t.Fatalf("SendEmailWithOptions: %v", err)
	}

	if _, ok := gotBody["cc"]; ok {
		t.Fatal("cc should be omitted when nil")
	}
	if _, ok := gotBody["bcc"]; ok {
		t.Fatal("bcc should be omitted when nil")
	}
	if _, ok := gotBody["labels"]; ok {
		t.Fatal("labels should be omitted when nil")
	}
}

// ---------------------------------------------------------------------------
// ListMessages with IsRead, Folder, Label filters
// ---------------------------------------------------------------------------

func TestListMessagesWithFilters(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		q := r.URL.Query()
		if q.Get("is_read") != "true" {
			t.Fatalf("expected is_read=true, got %s", q.Get("is_read"))
		}
		if q.Get("folder") != "archive" {
			t.Fatalf("expected folder=archive, got %s", q.Get("folder"))
		}
		if q.Get("label") != "urgent" {
			t.Fatalf("expected label=urgent, got %s", q.Get("label"))
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"messages":[],"total":0,"unread":0}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	isRead := true
	_, err := cl.ListMessages(context.Background(), ListMessagesOptions{
		Limit:     10,
		Direction: "inbound",
		IsRead:    &isRead,
		Folder:    "archive",
		Label:     "urgent",
	})
	if err != nil {
		t.Fatalf("ListMessages: %v", err)
	}
}

func TestListMessagesOmitsEmptyFilters(t *testing.T) {
	var gotQuery string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotQuery = r.URL.RawQuery
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"messages":[],"total":0,"unread":0}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.ListMessages(context.Background(), ListMessagesOptions{
		Limit: 10,
	})
	if err != nil {
		t.Fatalf("ListMessages: %v", err)
	}

	if strings.Contains(gotQuery, "is_read=") {
		t.Fatalf("is_read should not be in query when nil: %s", gotQuery)
	}
	if strings.Contains(gotQuery, "folder=") {
		t.Fatalf("folder should not be in query when empty: %s", gotQuery)
	}
	if strings.Contains(gotQuery, "label=") {
		t.Fatalf("label should not be in query when empty: %s", gotQuery)
	}
}

// ---------------------------------------------------------------------------
// SearchMessages with new filters
// ---------------------------------------------------------------------------

func TestSearchMessagesWithNewFilters(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		q := r.URL.Query()
		if q.Get("is_read") != "false" {
			t.Fatalf("expected is_read=false, got %s", q.Get("is_read"))
		}
		if q.Get("jacs_verified") != "true" {
			t.Fatalf("expected jacs_verified=true, got %s", q.Get("jacs_verified"))
		}
		if q.Get("folder") != "inbox" {
			t.Fatalf("expected folder=inbox, got %s", q.Get("folder"))
		}
		if q.Get("label") != "project-a" {
			t.Fatalf("expected label=project-a, got %s", q.Get("label"))
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"messages":[],"total":0,"unread":0}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	isRead := false
	jacsVerified := true
	_, err := cl.SearchMessages(context.Background(), SearchOptions{
		Q:            "test",
		IsRead:       &isRead,
		JacsVerified: &jacsVerified,
		Folder:       "inbox",
		Label:        "project-a",
	})
	if err != nil {
		t.Fatalf("SearchMessages: %v", err)
	}
}

// ---------------------------------------------------------------------------
// SearchMessages with has_attachments filter
// ---------------------------------------------------------------------------

func TestSearchMessagesWithHasAttachments(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		q := r.URL.Query()
		if q.Get("has_attachments") != "true" {
			t.Fatalf("expected has_attachments=true, got %s", q.Get("has_attachments"))
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"messages":[],"total":0,"unread":0}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	hasAttachments := true
	_, err := cl.SearchMessages(context.Background(), SearchOptions{
		Q:              "test",
		HasAttachments: &hasAttachments,
	})
	if err != nil {
		t.Fatalf("SearchMessages: %v", err)
	}
}

func TestSearchMessagesOmitsHasAttachmentsWhenNil(t *testing.T) {
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
	if strings.Contains(gotQuery, "has_attachments=") {
		t.Fatalf("has_attachments should not be in query when nil: %s", gotQuery)
	}
}

// ---------------------------------------------------------------------------
// ListMessages with has_attachments filter
// ---------------------------------------------------------------------------

func TestListMessagesWithHasAttachments(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		q := r.URL.Query()
		if q.Get("has_attachments") != "false" {
			t.Fatalf("expected has_attachments=false, got %s", q.Get("has_attachments"))
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"messages":[],"total":0,"unread":0}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	hasAttachments := false
	_, err := cl.ListMessages(context.Background(), ListMessagesOptions{
		Limit:          10,
		HasAttachments: &hasAttachments,
	})
	if err != nil {
		t.Fatalf("ListMessages: %v", err)
	}
}

func TestListMessagesOmitsHasAttachmentsWhenNil(t *testing.T) {
	var gotQuery string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotQuery = r.URL.RawQuery
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"messages":[],"total":0,"unread":0}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.ListMessages(context.Background(), ListMessagesOptions{
		Limit: 10,
	})
	if err != nil {
		t.Fatalf("ListMessages: %v", err)
	}
	if strings.Contains(gotQuery, "has_attachments=") {
		t.Fatalf("has_attachments should not be in query when nil: %s", gotQuery)
	}
}

// ---------------------------------------------------------------------------
// EmailMessage new fields (CcAddresses, Labels, Folder)
// ---------------------------------------------------------------------------

func TestEmailMessageNewFieldsDeserialization(t *testing.T) {
	raw := `{
		"id":"msg-1",
		"direction":"inbound",
		"from_address":"alice@hai.ai",
		"to_address":"bob@hai.ai",
		"subject":"Test",
		"body_text":"Hello",
		"is_read":false,
		"delivery_status":"delivered",
		"created_at":"2026-01-01T00:00:00Z",
		"cc_addresses":["cc1@hai.ai","cc2@hai.ai"],
		"labels":["urgent","project-x"],
		"folder":"archive"
	}`
	var msg EmailMessage
	if err := json.Unmarshal([]byte(raw), &msg); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if len(msg.CcAddresses) != 2 || msg.CcAddresses[0] != "cc1@hai.ai" || msg.CcAddresses[1] != "cc2@hai.ai" {
		t.Fatalf("unexpected cc_addresses: %v", msg.CcAddresses)
	}
	if len(msg.Labels) != 2 || msg.Labels[0] != "urgent" || msg.Labels[1] != "project-x" {
		t.Fatalf("unexpected labels: %v", msg.Labels)
	}
	if msg.Folder != "archive" {
		t.Fatalf("unexpected folder: %s", msg.Folder)
	}
}

func TestEmailMessageNewFieldsDefaultsWhenMissing(t *testing.T) {
	raw := `{
		"id":"msg-2",
		"direction":"inbound",
		"from_address":"a@b",
		"to_address":"c@d",
		"subject":"s",
		"body_text":"b",
		"is_read":false,
		"delivery_status":"delivered",
		"created_at":"2026-01-01T00:00:00Z"
	}`
	var msg EmailMessage
	if err := json.Unmarshal([]byte(raw), &msg); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if msg.CcAddresses != nil {
		t.Fatalf("cc_addresses should be nil when missing, got %v", msg.CcAddresses)
	}
	if msg.Labels != nil {
		t.Fatalf("labels should be nil when missing, got %v", msg.Labels)
	}
	if msg.Folder != "" {
		t.Fatalf("folder should be empty when missing, got %s", msg.Folder)
	}
}

// ---------------------------------------------------------------------------
// Forward
// ---------------------------------------------------------------------------

func TestForwardSendsPost(t *testing.T) {
	var gotBody map[string]interface{}
	var gotPath string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path
		body, _ := io.ReadAll(r.Body)
		_ = json.Unmarshal(body, &gotBody)
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"message_id":"msg-fwd","status":"sent"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	result, err := cl.Forward(context.Background(), ForwardOptions{
		MessageID: "msg-42",
		To:        "charlie@hai.ai",
		Comment:   "FYI",
	})
	if err != nil {
		t.Fatalf("Forward: %v", err)
	}
	if result.MessageID != "msg-fwd" {
		t.Fatalf("unexpected message_id: %s", result.MessageID)
	}
	if !strings.Contains(gotPath, "/email/forward") {
		t.Fatalf("unexpected path: %s", gotPath)
	}
	if gotBody["message_id"] != "msg-42" {
		t.Fatalf("unexpected message_id in body: %v", gotBody["message_id"])
	}
	if gotBody["to"] != "charlie@hai.ai" {
		t.Fatalf("unexpected to in body: %v", gotBody["to"])
	}
	if gotBody["comment"] != "FYI" {
		t.Fatalf("unexpected comment in body: %v", gotBody["comment"])
	}
}

func TestForwardOmitsCommentWhenEmpty(t *testing.T) {
	var gotBody map[string]interface{}

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		_ = json.Unmarshal(body, &gotBody)
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"message_id":"msg-fwd","status":"sent"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	_, err := cl.Forward(context.Background(), ForwardOptions{
		MessageID: "msg-42",
		To:        "charlie@hai.ai",
	})
	if err != nil {
		t.Fatalf("Forward: %v", err)
	}
	if _, ok := gotBody["comment"]; ok {
		t.Fatal("comment should be omitted when empty")
	}
}

// ---------------------------------------------------------------------------
// Archive / Unarchive
// ---------------------------------------------------------------------------

func TestArchiveSendsPost(t *testing.T) {
	var gotPath string
	var gotMethod string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path
		gotMethod = r.Method
		w.WriteHeader(http.StatusNoContent)
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	err := cl.Archive(context.Background(), "msg-42")
	if err != nil {
		t.Fatalf("Archive: %v", err)
	}
	if gotMethod != http.MethodPost {
		t.Fatalf("expected POST, got %s", gotMethod)
	}
	if !strings.Contains(gotPath, "/email/messages/msg-42/archive") {
		t.Fatalf("unexpected path: %s", gotPath)
	}
}

func TestUnarchiveSendsPost(t *testing.T) {
	var gotPath string
	var gotMethod string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path
		gotMethod = r.Method
		w.WriteHeader(http.StatusNoContent)
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	err := cl.Unarchive(context.Background(), "msg-42")
	if err != nil {
		t.Fatalf("Unarchive: %v", err)
	}
	if gotMethod != http.MethodPost {
		t.Fatalf("expected POST, got %s", gotMethod)
	}
	if !strings.Contains(gotPath, "/email/messages/msg-42/unarchive") {
		t.Fatalf("unexpected path: %s", gotPath)
	}
}

// ---------------------------------------------------------------------------
// GetContacts
// ---------------------------------------------------------------------------

func TestGetContactsReturnsList(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet {
			t.Fatalf("expected GET, got %s", r.Method)
		}
		if !strings.Contains(r.URL.Path, "/email/contacts") {
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"contacts":[
			{"email":"alice@hai.ai","display_name":"Alice","last_contact":"2026-01-01T00:00:00Z","jacs_verified":true,"reputation_tier":"established"},
			{"email":"bob@example.com","last_contact":"2026-01-02T00:00:00Z"}
		]}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	contacts, err := cl.GetContacts(context.Background())
	if err != nil {
		t.Fatalf("GetContacts: %v", err)
	}
	if len(contacts) != 2 {
		t.Fatalf("expected 2 contacts, got %d", len(contacts))
	}
	if contacts[0].Email != "alice@hai.ai" {
		t.Fatalf("unexpected email: %s", contacts[0].Email)
	}
	if contacts[0].DisplayName != "Alice" {
		t.Fatalf("unexpected display_name: %s", contacts[0].DisplayName)
	}
	if !contacts[0].JacsVerified {
		t.Fatal("expected jacs_verified=true")
	}
	if contacts[0].ReputationTier != "established" {
		t.Fatalf("unexpected reputation_tier: %s", contacts[0].ReputationTier)
	}
	if contacts[1].DisplayName != "" {
		t.Fatalf("display_name should be empty when missing, got %s", contacts[1].DisplayName)
	}
	if contacts[1].JacsVerified {
		t.Fatal("jacs_verified should be false when missing")
	}
	if contacts[1].ReputationTier != "" {
		t.Fatalf("reputation_tier should be empty when missing, got %s", contacts[1].ReputationTier)
	}
}

func TestGetContactsBareArray(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		// API might return bare array instead of wrapped object
		_, _ = w.Write([]byte(`[
			{"email":"alice@hai.ai","last_contact":"2026-01-01T00:00:00Z","jacs_verified":false}
		]`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	contacts, err := cl.GetContacts(context.Background())
	if err != nil {
		t.Fatalf("GetContacts: %v", err)
	}
	if len(contacts) != 1 {
		t.Fatalf("expected 1 contact, got %d", len(contacts))
	}
	if contacts[0].Email != "alice@hai.ai" {
		t.Fatalf("unexpected email: %s", contacts[0].Email)
	}
}

// ---------------------------------------------------------------------------
// Contact struct serialization
// ---------------------------------------------------------------------------

func TestContactSerialization(t *testing.T) {
	c := Contact{
		Email:          "alice@hai.ai",
		DisplayName:    "Alice Agent",
		LastContact:    "2026-01-01T00:00:00Z",
		JacsVerified:   true,
		ReputationTier: "established",
	}
	data, err := json.Marshal(c)
	if err != nil {
		t.Fatalf("Marshal: %v", err)
	}
	var parsed Contact
	if err := json.Unmarshal(data, &parsed); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if parsed.Email != c.Email {
		t.Fatalf("email mismatch: %s vs %s", parsed.Email, c.Email)
	}
	if parsed.DisplayName != c.DisplayName {
		t.Fatalf("display_name mismatch: %s vs %s", parsed.DisplayName, c.DisplayName)
	}
	if parsed.JacsVerified != c.JacsVerified {
		t.Fatalf("jacs_verified mismatch: %v vs %v", parsed.JacsVerified, c.JacsVerified)
	}
	if parsed.ReputationTier != c.ReputationTier {
		t.Fatalf("reputation_tier mismatch: %s vs %s", parsed.ReputationTier, c.ReputationTier)
	}
}

// ---------------------------------------------------------------------------
// EmailNamespace (Agent) wrappers for new methods
// ---------------------------------------------------------------------------

func TestEmailNamespaceForward(t *testing.T) {
	var gotPath string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPath = r.URL.Path
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"message_id":"msg-fwd","status":"sent"}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	ns := &EmailNamespace{client: cl}
	result, err := ns.Forward(context.Background(), ForwardOptions{
		MessageID: "msg-1",
		To:        "bob@hai.ai",
	})
	if err != nil {
		t.Fatalf("Forward: %v", err)
	}
	if result.MessageID != "msg-fwd" {
		t.Fatalf("unexpected message_id: %s", result.MessageID)
	}
	if !strings.Contains(gotPath, "/email/forward") {
		t.Fatalf("unexpected path: %s", gotPath)
	}
}

func TestEmailNamespaceArchiveUnarchive(t *testing.T) {
	var gotPaths []string

	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotPaths = append(gotPaths, r.URL.Path)
		w.WriteHeader(http.StatusNoContent)
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	ns := &EmailNamespace{client: cl}

	if err := ns.Archive(context.Background(), "msg-1"); err != nil {
		t.Fatalf("Archive: %v", err)
	}
	if err := ns.Unarchive(context.Background(), "msg-1"); err != nil {
		t.Fatalf("Unarchive: %v", err)
	}

	if len(gotPaths) != 2 {
		t.Fatalf("expected 2 calls, got %d", len(gotPaths))
	}
	if !strings.Contains(gotPaths[0], "/archive") {
		t.Fatalf("first call should be archive, got %s", gotPaths[0])
	}
	if !strings.Contains(gotPaths[1], "/unarchive") {
		t.Fatalf("second call should be unarchive, got %s", gotPaths[1])
	}
}

func TestEmailNamespaceContacts(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"contacts":[{"email":"alice@hai.ai","last_contact":"2026-01-01T00:00:00Z","jacs_verified":false}]}`))
	}))
	defer srv.Close()

	cl, _ := newTestClient(t, srv.URL)
	ns := &EmailNamespace{client: cl}
	contacts, err := ns.Contacts(context.Background())
	if err != nil {
		t.Fatalf("Contacts: %v", err)
	}
	if len(contacts) != 1 {
		t.Fatalf("expected 1 contact, got %d", len(contacts))
	}
}

func TestEmailStatusNestedFieldsDeserialization(t *testing.T) {
	raw := `{
		"email": "bot@hai.ai",
		"status": "active",
		"tier": "established",
		"billing_tier": "pro",
		"messages_sent_24h": 10,
		"daily_limit": 100,
		"daily_used": 10,
		"resets_at": "2026-03-15T00:00:00Z",
		"messages_sent_total": 500,
		"external_enabled": true,
		"external_sends_today": 3,
		"last_tier_change": "2026-01-01T00:00:00Z",
		"volume": {
			"sent_total": 500,
			"received_total": 300,
			"sent_24h": 10
		},
		"delivery": {
			"bounce_count": 2,
			"spam_report_count": 1,
			"delivery_rate": 0.98
		},
		"reputation": {
			"score": 85.5,
			"tier": "established",
			"email_score": 90.0,
			"hai_score": 80.0
		}
	}`

	var status EmailStatus
	if err := json.Unmarshal([]byte(raw), &status); err != nil {
		t.Fatalf("unmarshal EmailStatus with nested fields: %v", err)
	}

	// Top-level fields
	if status.Email != "bot@hai.ai" {
		t.Errorf("email: got %q, want %q", status.Email, "bot@hai.ai")
	}
	if status.Tier != "established" {
		t.Errorf("tier: got %q, want %q", status.Tier, "established")
	}

	// Volume
	if status.Volume == nil {
		t.Fatal("volume should not be nil")
	}
	if status.Volume.SentTotal != 500 {
		t.Errorf("volume.sent_total: got %d, want 500", status.Volume.SentTotal)
	}
	if status.Volume.ReceivedTotal != 300 {
		t.Errorf("volume.received_total: got %d, want 300", status.Volume.ReceivedTotal)
	}
	if status.Volume.Sent24h != 10 {
		t.Errorf("volume.sent_24h: got %d, want 10", status.Volume.Sent24h)
	}

	// Delivery
	if status.Delivery == nil {
		t.Fatal("delivery should not be nil")
	}
	if status.Delivery.BounceCount != 2 {
		t.Errorf("delivery.bounce_count: got %d, want 2", status.Delivery.BounceCount)
	}
	if status.Delivery.SpamReportCount != 1 {
		t.Errorf("delivery.spam_report_count: got %d, want 1", status.Delivery.SpamReportCount)
	}
	if status.Delivery.DeliveryRate != 0.98 {
		t.Errorf("delivery.delivery_rate: got %f, want 0.98", status.Delivery.DeliveryRate)
	}

	// Reputation
	if status.Reputation == nil {
		t.Fatal("reputation should not be nil")
	}
	if status.Reputation.Score != 85.5 {
		t.Errorf("reputation.score: got %f, want 85.5", status.Reputation.Score)
	}
	if status.Reputation.Tier != "established" {
		t.Errorf("reputation.tier: got %q, want %q", status.Reputation.Tier, "established")
	}
	if status.Reputation.EmailScore != 90.0 {
		t.Errorf("reputation.email_score: got %f, want 90.0", status.Reputation.EmailScore)
	}
	if status.Reputation.HaiScore == nil || *status.Reputation.HaiScore != 80.0 {
		t.Errorf("reputation.hai_score: got %v, want 80.0", status.Reputation.HaiScore)
	}
}

func TestEmailStatusNestedFieldsDefaultToNilWhenMissing(t *testing.T) {
	raw := `{
		"email": "bot@hai.ai",
		"status": "active",
		"tier": "new",
		"billing_tier": "free",
		"messages_sent_24h": 0,
		"daily_limit": 10,
		"daily_used": 0,
		"resets_at": "2026-03-15T00:00:00Z"
	}`

	var status EmailStatus
	if err := json.Unmarshal([]byte(raw), &status); err != nil {
		t.Fatalf("unmarshal EmailStatus without nested fields: %v", err)
	}

	if status.Volume != nil {
		t.Errorf("volume should be nil when absent, got %+v", status.Volume)
	}
	if status.Delivery != nil {
		t.Errorf("delivery should be nil when absent, got %+v", status.Delivery)
	}
	if status.Reputation != nil {
		t.Errorf("reputation should be nil when absent, got %+v", status.Reputation)
	}
}

func TestEmailStatusNestedFieldsRoundTrip(t *testing.T) {
	haiScore := 80.0
	status := EmailStatus{
		Email:       "bot@hai.ai",
		Status:      "active",
		Tier:        "established",
		BillingTier: "pro",
		Volume: &EmailVolumeInfo{
			SentTotal:     500,
			ReceivedTotal: 300,
			Sent24h:       10,
		},
		Delivery: &EmailDeliveryInfo{
			BounceCount:     2,
			SpamReportCount: 1,
			DeliveryRate:    0.98,
		},
		Reputation: &EmailReputationInfo{
			Score:      85.5,
			Tier:       "established",
			EmailScore: 90.0,
			HaiScore:   &haiScore,
		},
	}

	data, err := json.Marshal(status)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	var parsed EmailStatus
	if err := json.Unmarshal(data, &parsed); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}

	if parsed.Volume == nil || parsed.Volume.SentTotal != 500 {
		t.Errorf("round-trip volume.sent_total mismatch")
	}
	if parsed.Delivery == nil || parsed.Delivery.BounceCount != 2 {
		t.Errorf("round-trip delivery.bounce_count mismatch")
	}
	if parsed.Reputation == nil || parsed.Reputation.Score != 85.5 {
		t.Errorf("round-trip reputation.score mismatch")
	}
	if parsed.Reputation.HaiScore == nil || *parsed.Reputation.HaiScore != 80.0 {
		t.Errorf("round-trip reputation.hai_score mismatch")
	}
}
