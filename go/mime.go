package haiai

import (
	"bytes"
	"encoding/base64"
	"fmt"
	"strings"
	"time"

	"github.com/google/uuid"
)

// sanitizeHeader strips \r and \n from a header value to prevent CRLF injection.
func sanitizeHeader(value string) string {
	value = strings.ReplaceAll(value, "\r", "")
	value = strings.ReplaceAll(value, "\n", "")
	return value
}

// BuildRFC5322Email builds an RFC 5322 email from structured fields.
//
// Produces raw bytes with CRLF line endings suitable for JACS signing
// and parseable by net/mail.ReadMessage.
func BuildRFC5322Email(opts SendEmailOptions, fromEmail string) ([]byte, error) {
	safeTo := sanitizeHeader(opts.To)
	safeFrom := sanitizeHeader(fromEmail)
	safeSubject := sanitizeHeader(opts.Subject)
	messageID := fmt.Sprintf("<%s@hai.ai>", uuid.New().String())
	date := time.Now().UTC().Format(time.RFC1123Z)

	var buf bytes.Buffer

	if len(opts.Attachments) == 0 {
		// Simple text/plain email
		fmt.Fprintf(&buf, "From: <%s>\r\n", safeFrom)
		fmt.Fprintf(&buf, "To: %s\r\n", safeTo)
		fmt.Fprintf(&buf, "Subject: %s\r\n", safeSubject)
		fmt.Fprintf(&buf, "Date: %s\r\n", date)
		fmt.Fprintf(&buf, "Message-ID: %s\r\n", messageID)
		if opts.InReplyTo != "" {
			safeReply := sanitizeHeader(opts.InReplyTo)
			fmt.Fprintf(&buf, "In-Reply-To: %s\r\n", safeReply)
			fmt.Fprintf(&buf, "References: %s\r\n", safeReply)
		}
		buf.WriteString("MIME-Version: 1.0\r\n")
		buf.WriteString("Content-Type: text/plain; charset=utf-8\r\n")
		buf.WriteString("Content-Transfer-Encoding: 8bit\r\n")
		buf.WriteString("\r\n") // end of headers
		buf.WriteString(opts.Body)
		buf.WriteString("\r\n")
	} else {
		// multipart/mixed with text body + attachments
		boundary := fmt.Sprintf("hai-boundary-%s", strings.ReplaceAll(uuid.New().String(), "-", ""))

		fmt.Fprintf(&buf, "From: <%s>\r\n", safeFrom)
		fmt.Fprintf(&buf, "To: %s\r\n", safeTo)
		fmt.Fprintf(&buf, "Subject: %s\r\n", safeSubject)
		fmt.Fprintf(&buf, "Date: %s\r\n", date)
		fmt.Fprintf(&buf, "Message-ID: %s\r\n", messageID)
		if opts.InReplyTo != "" {
			safeReply := sanitizeHeader(opts.InReplyTo)
			fmt.Fprintf(&buf, "In-Reply-To: %s\r\n", safeReply)
			fmt.Fprintf(&buf, "References: %s\r\n", safeReply)
		}
		buf.WriteString("MIME-Version: 1.0\r\n")
		fmt.Fprintf(&buf, "Content-Type: multipart/mixed; boundary=\"%s\"\r\n", boundary)
		buf.WriteString("\r\n") // end of headers

		// Body part
		fmt.Fprintf(&buf, "--%s\r\n", boundary)
		buf.WriteString("Content-Type: text/plain; charset=utf-8\r\n")
		buf.WriteString("Content-Transfer-Encoding: 8bit\r\n")
		buf.WriteString("\r\n")
		buf.WriteString(opts.Body)
		buf.WriteString("\r\n")

		// Attachment parts
		for _, att := range opts.Attachments {
			rawData := att.Data
			if len(rawData) == 0 && att.DataBase64 != "" {
				decoded, err := base64.StdEncoding.DecodeString(att.DataBase64)
				if err != nil {
					return nil, fmt.Errorf("invalid base64 in attachment %q: %w", att.Filename, err)
				}
				rawData = decoded
			}
			b64 := base64.StdEncoding.EncodeToString(rawData)
			safeFilename := sanitizeHeader(att.Filename)
			safeContentType := sanitizeHeader(att.ContentType)

			fmt.Fprintf(&buf, "--%s\r\n", boundary)
			fmt.Fprintf(&buf, "Content-Type: %s; name=\"%s\"\r\n", safeContentType, safeFilename)
			fmt.Fprintf(&buf, "Content-Disposition: attachment; filename=\"%s\"\r\n", safeFilename)
			buf.WriteString("Content-Transfer-Encoding: base64\r\n")
			buf.WriteString("\r\n")
			// Write base64 in 76-char lines (RFC 2045)
			for i := 0; i < len(b64); i += 76 {
				end := i + 76
				if end > len(b64) {
					end = len(b64)
				}
				buf.WriteString(b64[i:end])
				buf.WriteString("\r\n")
			}
		}

		// Closing boundary
		fmt.Fprintf(&buf, "--%s--\r\n", boundary)
	}

	return buf.Bytes(), nil
}
