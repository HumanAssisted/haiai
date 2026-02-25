// Live integration tests for HAI email CRUD operations.
//
// Gated behind HAI_LIVE_TEST=1. Requires a running HAI API at
// HAI_URL (defaults to http://localhost:3000) backed by Stalwart.
//
// Run:
//
//	HAI_LIVE_TEST=1 HAI_URL=http://localhost:3000 go test -run TestEmailIntegration -v -count=1

package haisdk

import (
	"context"
	"crypto/ed25519"
	"crypto/x509"
	"encoding/pem"
	"fmt"
	"os"
	"strings"
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

	// ── Setup: register a fresh JACS agent ────────────────────────────────
	ownerEmail := os.Getenv("HAI_OWNER_EMAIL")
	if ownerEmail == "" {
		ownerEmail = "jonathan@hai.io"
	}

	reg, err := RegisterNewAgentWithEndpoint(ctx, apiURL, agentName, &RegisterNewAgentOptions{
		Description: "Go integration test agent",
		OwnerEmail:  ownerEmail,
		Quiet:       true,
	})
	if err != nil {
		t.Fatalf("RegisterNewAgentWithEndpoint: %v", err)
	}

	jacsID := reg.Registration.JacsID
	if jacsID == "" {
		jacsID = reg.Registration.AgentID
	}
	haiAgentID := reg.Registration.AgentID
	t.Logf("Registered agent: jacs_id=%s, agent_id=%s", jacsID, haiAgentID)

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
	// jacsID is used for auth headers; haiAgentID (the HAI-assigned UUID) is
	// used for email URL paths.
	client, err := NewClient(
		WithEndpoint(apiURL),
		WithJACSID(jacsID),
		WithHaiAgentID(haiAgentID),
		WithPrivateKey(privKey),
	)
	if err != nil {
		t.Fatalf("NewClient: %v", err)
	}

	// ── 0. Claim username (provisions the @hai.ai email address) ─────────
	claimResult, err := client.ClaimUsername(ctx, haiAgentID, agentName)
	if err != nil {
		t.Fatalf("ClaimUsername: %v", err)
	}
	t.Logf("Claimed username: %s, email=%s", claimResult.Username, claimResult.Email)

	subject := fmt.Sprintf("go-integ-test-%d", time.Now().UnixMilli())
	body := "Hello from Go integration test!"
	selfAddr := fmt.Sprintf("%s@hai.ai", agentName)

	// sentMessageID holds the database UUID returned by SendEmail.
	// It is set in the SendEmail subtest and consumed by later subtests.
	var sentMessageID string

	// ── 1. Send email ─────────────────────────────────────────────────────
	t.Run("SendEmail", func(t *testing.T) {
		result, err := client.SendEmail(ctx, selfAddr, subject, body)
		if err != nil {
			t.Fatalf("SendEmail: %v", err)
		}
		if result.MessageID == "" {
			t.Fatal("expected non-empty message_id")
		}
		sentMessageID = result.MessageID
		t.Logf("Sent email: message_id=%s", sentMessageID)
	})

	// Small delay for async delivery through Stalwart.
	time.Sleep(2 * time.Second)

	// ── 2. List messages ──────────────────────────────────────────────────
	t.Run("ListMessages", func(t *testing.T) {
		messages, err := client.ListMessages(ctx, ListMessagesOptions{Limit: 20})
		if err != nil {
			t.Fatalf("ListMessages: %v", err)
		}
		t.Logf("Listed %d messages", len(messages))
		if len(messages) == 0 {
			t.Fatal("expected at least one message after send")
		}
		// Verify the sent message appears in the listing.
		found := false
		for _, m := range messages {
			if m.Subject == subject {
				found = true
				break
			}
		}
		if !found {
			t.Fatalf("sent message with subject %q not found in listing", subject)
		}
	})

	// ── 3. Get message ────────────────────────────────────────────────────
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
		if !strings.Contains(msg.BodyText, body) {
			t.Fatalf("body_text should contain sent body: got %q, want substring %q", msg.BodyText, body)
		}
		t.Logf("Got message: id=%s, subject=%s, body_len=%d", msg.ID, msg.Subject, len(msg.BodyText))
	})

	// ── 4. Mark read ──────────────────────────────────────────────────────
	t.Run("MarkRead", func(t *testing.T) {
		if err := client.MarkRead(ctx, sentMessageID); err != nil {
			t.Fatalf("MarkRead: %v", err)
		}
		t.Log("Marked read -- no error")
	})

	// ── 5. Mark unread ────────────────────────────────────────────────────
	t.Run("MarkUnread", func(t *testing.T) {
		if err := client.MarkUnread(ctx, sentMessageID); err != nil {
			t.Fatalf("MarkUnread: %v", err)
		}
		t.Log("Marked unread -- no error")
	})

	// ── 6. Search messages ────────────────────────────────────────────────
	t.Run("SearchMessages", func(t *testing.T) {
		results, err := client.SearchMessages(ctx, SearchOptions{Q: subject})
		if err != nil {
			t.Fatalf("SearchMessages: %v", err)
		}
		t.Logf("Search for %q found %d results", subject, len(results))
		if len(results) == 0 {
			t.Fatal("search should find the sent message")
		}
		found := false
		for _, m := range results {
			if m.Subject == subject {
				found = true
				break
			}
		}
		if !found {
			t.Fatal("search results should contain the sent message by subject")
		}
	})

	// ── 7. Unread count ───────────────────────────────────────────────────
	t.Run("GetUnreadCount", func(t *testing.T) {
		count, err := client.GetUnreadCount(ctx)
		if err != nil {
			t.Fatalf("GetUnreadCount: %v", err)
		}
		if count < 0 {
			t.Fatalf("unread count should be non-negative, got %d", count)
		}
		t.Logf("Unread count: %d", count)
	})

	// ── 8. Email status ───────────────────────────────────────────────────
	t.Run("GetEmailStatus", func(t *testing.T) {
		status, err := client.GetEmailStatus(ctx)
		if err != nil {
			t.Fatalf("GetEmailStatus: %v", err)
		}
		if status.Email == "" {
			t.Fatal("email status should include an email address")
		}
		if status.Status == "" {
			t.Fatal("email status should include a status string")
		}
		t.Logf("Email status: email=%s, status=%s, tier=%s", status.Email, status.Status, status.Tier)
	})

	// ── 9. Reply ──────────────────────────────────────────────────────────
	t.Run("Reply", func(t *testing.T) {
		// Pass the database UUID (sentMessageID), not the RFC 5322 Message-ID.
		// Reply() internally calls GetMessage to fetch the original, then sends
		// to original.FromAddress with an In-Reply-To header for threading.
		result, err := client.Reply(ctx, sentMessageID, "Reply from Go integration test!", "")
		if err != nil {
			t.Fatalf("Reply: %v", err)
		}
		if result.MessageID == "" {
			t.Fatal("reply message_id should not be empty")
		}
		t.Logf("Reply sent: message_id=%s", result.MessageID)
	})

	// ── 10. Delete ────────────────────────────────────────────────────────
	t.Run("DeleteMessage", func(t *testing.T) {
		if err := client.DeleteMessage(ctx, sentMessageID); err != nil {
			t.Fatalf("DeleteMessage: %v", err)
		}
		t.Log("Deleted message -- no error")
	})

	// ── 11. Get deleted ──────────────────────────────────────────────────
	t.Run("GetDeletedMessage", func(t *testing.T) {
		_, err := client.GetMessage(ctx, sentMessageID)
		if err == nil {
			t.Fatal("GetMessage on deleted message should return an error")
		}
		t.Logf("Verified deleted message returns error: %v", err)
	})
}
