package haiai

import (
	"bytes"
	"net/mail"
	"strings"
	"testing"
)

func TestBuildSimpleTextEmail(t *testing.T) {
	opts := SendEmailOptions{
		To:      "recipient@hai.ai",
		Subject: "Test Subject",
		Body:    "Hello, world!",
	}

	raw, err := BuildRFC5322Email(opts, "sender@hai.ai")
	if err != nil {
		t.Fatalf("BuildRFC5322Email failed: %v", err)
	}

	msg, err := mail.ReadMessage(bytes.NewReader(raw))
	if err != nil {
		t.Fatalf("failed to parse email: %v", err)
	}

	if got := msg.Header.Get("To"); got != "recipient@hai.ai" {
		t.Errorf("To = %q, want %q", got, "recipient@hai.ai")
	}
	if got := msg.Header.Get("Subject"); got != "Test Subject" {
		t.Errorf("Subject = %q, want %q", got, "Test Subject")
	}
	if got := msg.Header.Get("Date"); got == "" {
		t.Error("Date header is empty")
	}
	if got := msg.Header.Get("Message-Id"); got == "" {
		t.Error("Message-ID header is empty")
	}
	if got := msg.Header.Get("Mime-Version"); got != "1.0" {
		t.Errorf("MIME-Version = %q, want %q", got, "1.0")
	}
}

func TestBuildEmailWithAttachments(t *testing.T) {
	opts := SendEmailOptions{
		To:      "recipient@hai.ai",
		Subject: "With Attachments",
		Body:    "See attached.",
		Attachments: []EmailAttachment{
			{
				Filename:    "file1.txt",
				ContentType: "text/plain",
				Data:        []byte("content of file 1"),
			},
			{
				Filename:    "file2.pdf",
				ContentType: "application/pdf",
				Data:        []byte("fake pdf content"),
			},
		},
	}

	raw, err := BuildRFC5322Email(opts, "sender@hai.ai")
	if err != nil {
		t.Fatalf("BuildRFC5322Email failed: %v", err)
	}

	text := string(raw)
	if !strings.Contains(text, "Content-Type: multipart/mixed; boundary=") {
		t.Error("missing multipart/mixed content type")
	}
	if !strings.Contains(text, `Content-Disposition: attachment; filename="file1.txt"`) {
		t.Error("missing file1.txt attachment")
	}
	if !strings.Contains(text, `Content-Disposition: attachment; filename="file2.pdf"`) {
		t.Error("missing file2.pdf attachment")
	}
	if !strings.Contains(text, "Content-Transfer-Encoding: base64") {
		t.Error("missing base64 transfer encoding")
	}
	if !strings.Contains(text, "See attached.") {
		t.Error("missing body text")
	}
}

func TestBuildReplyEmail(t *testing.T) {
	opts := SendEmailOptions{
		To:        "recipient@hai.ai",
		Subject:   "Re: Original",
		Body:      "Reply body",
		InReplyTo: "<original-id@hai.ai>",
	}

	raw, err := BuildRFC5322Email(opts, "sender@hai.ai")
	if err != nil {
		t.Fatalf("BuildRFC5322Email failed: %v", err)
	}

	msg, err := mail.ReadMessage(bytes.NewReader(raw))
	if err != nil {
		t.Fatalf("failed to parse email: %v", err)
	}

	if got := msg.Header.Get("In-Reply-To"); got != "<original-id@hai.ai>" {
		t.Errorf("In-Reply-To = %q, want %q", got, "<original-id@hai.ai>")
	}
	if got := msg.Header.Get("References"); got != "<original-id@hai.ai>" {
		t.Errorf("References = %q, want %q", got, "<original-id@hai.ai>")
	}
}

func TestCRLFInjectionSanitized(t *testing.T) {
	opts := SendEmailOptions{
		To:      "recipient@hai.ai",
		Subject: "Bad\r\nBcc: attacker@evil.com",
		Body:    "Body",
	}

	raw, err := BuildRFC5322Email(opts, "sender@hai.ai")
	if err != nil {
		t.Fatalf("BuildRFC5322Email failed: %v", err)
	}

	text := string(raw)
	// No line should start with "Bcc:" (CRLF injection prevented)
	for _, line := range strings.Split(text, "\r\n") {
		if strings.HasPrefix(line, "Bcc:") {
			t.Error("CRLF injection succeeded: found header line starting with Bcc:")
		}
	}
	// Subject should be sanitized
	if !strings.Contains(text, "Subject: BadBcc: attacker@evil.com\r\n") {
		t.Error("subject was not properly sanitized")
	}
}

func TestCRLFLineEndings(t *testing.T) {
	opts := SendEmailOptions{
		To:      "recipient@hai.ai",
		Subject: "Test",
		Body:    "Body",
	}

	raw, err := BuildRFC5322Email(opts, "sender@hai.ai")
	if err != nil {
		t.Fatalf("BuildRFC5322Email failed: %v", err)
	}

	text := string(raw)
	if !strings.Contains(text, "\r\n") {
		t.Error("output does not contain CRLF line endings")
	}
}
