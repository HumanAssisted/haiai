package haisdk

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
