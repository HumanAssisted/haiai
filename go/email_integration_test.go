// Live integration tests for HAI email CRUD operations.
//
// Gated behind HAI_LIVE_TEST=1. Requires a running HAI API at
// HAI_URL (defaults to http://localhost:3000) backed by Stalwart.
//
// Run:
//
//	HAI_LIVE_TEST=1 HAI_URL=http://localhost:3000 go test -run TestEmailIntegration -v

package haisdk

import (
	"context"
	"crypto/ed25519"
	"crypto/x509"
	"encoding/pem"
	"fmt"
	"os"
	"testing"
	"time"
)

func TestEmailIntegration(t *testing.T) {
	if os.Getenv("HAI_LIVE_TEST") != "1" {
		t.Skip("set HAI_LIVE_TEST=1 to run live API tests")
	}

	apiURL := os.Getenv("HAI_URL")
	if apiURL == "" {
		apiURL = "http://localhost:3000"
	}

	ctx := context.Background()
	agentName := fmt.Sprintf("go-integ-%d", time.Now().UnixMilli())

	// ── Setup: register a fresh agent ─────────────────────────────────────
	reg, err := RegisterNewAgentWithEndpoint(ctx, apiURL, agentName, &RegisterNewAgentOptions{
		OwnerEmail:  "test@example.com",
		Description: "Go integration test agent",
		Quiet:       true,
	})
	if err != nil {
		t.Fatalf("RegisterNewAgentWithEndpoint: %v", err)
	}

	jacsID := reg.Registration.JacsID
	if jacsID == "" {
		jacsID = reg.Registration.AgentID
	}
	t.Logf("Registered agent: jacs_id=%s", jacsID)

	// Parse the returned private key PEM back into ed25519.PrivateKey.
	block, _ := pem.Decode(reg.PrivateKey)
	if block == nil {
		t.Fatal("failed to decode private key PEM")
	}
	parsed, err := x509.ParsePKCS8PrivateKey(block.Bytes)
	if err != nil {
		t.Fatalf("ParsePKCS8PrivateKey: %v", err)
	}
	privKey, ok := parsed.(ed25519.PrivateKey)
	if !ok {
		t.Fatal("parsed key is not ed25519.PrivateKey")
	}

	// Build client with explicit credentials.
	client, err := NewClient(
		WithEndpoint(apiURL),
		WithJACSID(jacsID),
		WithPrivateKey(privKey),
	)
	if err != nil {
		t.Fatalf("NewClient: %v", err)
	}

	subject := fmt.Sprintf("go-integ-test-%d", time.Now().UnixMilli())
	body := "Hello from Go integration test!"
	var sentMessageID string

	// ── 1. Send email ─────────────────────────────────────────────────────
	t.Run("SendEmail", func(t *testing.T) {
		result, err := client.SendEmail(ctx, fmt.Sprintf("%s@hai.ai", agentName), subject, body)
		if err != nil {
			t.Fatalf("SendEmail: %v", err)
		}
		sentMessageID = result.MessageID
		t.Logf("Sent email: message_id=%s", sentMessageID)
		if sentMessageID == "" {
			t.Fatal("message_id should not be empty")
		}
	})

	// Small delay for async delivery.
	time.Sleep(2 * time.Second)

	// ── 2. List messages ──────────────────────────────────────────────────
	t.Run("ListMessages", func(t *testing.T) {
		messages, err := client.ListMessages(ctx, ListMessagesOptions{Limit: 10})
		if err != nil {
			t.Fatalf("ListMessages: %v", err)
		}
		t.Logf("Listed %d messages", len(messages))
		if len(messages) == 0 {
			t.Fatal("should have at least one message")
		}
	})

	// ── 3. Get message ────────────────────────────────────────────────────
	var rfcMessageID string
	t.Run("GetMessage", func(t *testing.T) {
		msg, err := client.GetMessage(ctx, sentMessageID)
		if err != nil {
			t.Fatalf("GetMessage: %v", err)
		}
		if msg.Subject != subject {
			t.Fatalf("subject mismatch: got %q, want %q", msg.Subject, subject)
		}
		if msg.BodyText == "" {
			t.Fatal("body_text should not be empty")
		}
		rfcMessageID = msg.MessageID
		if rfcMessageID == "" {
			rfcMessageID = sentMessageID
		}
		t.Logf("Got message: subject=%s", msg.Subject)
	})

	// ── 4. Mark read ──────────────────────────────────────────────────────
	t.Run("MarkRead", func(t *testing.T) {
		if err := client.MarkRead(ctx, sentMessageID); err != nil {
			t.Fatalf("MarkRead: %v", err)
		}
		t.Log("Marked read")
	})

	// ── 5. Mark unread ────────────────────────────────────────────────────
	t.Run("MarkUnread", func(t *testing.T) {
		if err := client.MarkUnread(ctx, sentMessageID); err != nil {
			t.Fatalf("MarkUnread: %v", err)
		}
		t.Log("Marked unread")
	})

	// ── 6. Search messages ────────────────────────────────────────────────
	t.Run("SearchMessages", func(t *testing.T) {
		results, err := client.SearchMessages(ctx, SearchOptions{Q: subject})
		if err != nil {
			t.Fatalf("SearchMessages: %v", err)
		}
		t.Logf("Search found %d results", len(results))
		if len(results) == 0 {
			t.Fatal("search should find the sent message")
		}
	})

	// ── 7. Unread count ───────────────────────────────────────────────────
	t.Run("GetUnreadCount", func(t *testing.T) {
		count, err := client.GetUnreadCount(ctx)
		if err != nil {
			t.Fatalf("GetUnreadCount: %v", err)
		}
		t.Logf("Unread count: %d", count)
		// Just verify it returns without error; count >= 0 always true.
	})

	// ── 8. Email status ───────────────────────────────────────────────────
	t.Run("GetEmailStatus", func(t *testing.T) {
		status, err := client.GetEmailStatus(ctx)
		if err != nil {
			t.Fatalf("GetEmailStatus: %v", err)
		}
		t.Logf("Email status: email=%s, tier=%s", status.Email, status.Tier)
		if status.Email == "" {
			t.Fatal("status should include email")
		}
	})

	// ── 9. Reply ──────────────────────────────────────────────────────────
	t.Run("Reply", func(t *testing.T) {
		result, err := client.Reply(ctx, rfcMessageID, "Reply from Go integration test!", "")
		if err != nil {
			t.Fatalf("Reply: %v", err)
		}
		t.Logf("Reply sent: message_id=%s", result.MessageID)
		if result.MessageID == "" {
			t.Fatal("reply message_id should not be empty")
		}
	})

	// ── 10. Delete ────────────────────────────────────────────────────────
	t.Run("DeleteMessage", func(t *testing.T) {
		if err := client.DeleteMessage(ctx, sentMessageID); err != nil {
			t.Fatalf("DeleteMessage: %v", err)
		}
		t.Log("Deleted message")
	})

	// ── 11. Verify deleted ────────────────────────────────────────────────
	t.Run("VerifyDeleted", func(t *testing.T) {
		_, err := client.GetMessage(ctx, sentMessageID)
		if err == nil {
			t.Fatal("GetMessage on deleted message should return error")
		}
		t.Log("Verified deleted message returns error")
	})
}
