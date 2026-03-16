package haiai

import (
	"crypto/sha256"
	"fmt"
	"sort"
	"strings"
)

// ContentHashAttachment represents a single attachment for content hash computation.
type ContentHashAttachment struct {
	Filename    string
	ContentType string
	Data        []byte
}

// ComputeContentHash computes a deterministic content hash for email content.
//
// All SDKs must produce identical content hashes for the same inputs.
// Algorithm mirrors JACS's compute_attachment_hash convention:
//
//  1. Per-attachment hash: sha256(filename_utf8 + ":" + content_type_lower + ":" + raw_bytes)
//  2. Sort attachment hashes lexicographically
//  3. Overall hash:
//     - No attachments: sha256(subject + "\n" + body)
//     - With attachments: sha256(subject + "\n" + body + "\n" + sorted_hashes.join("\n"))
//
// Returns "sha256:<hex>" format.
func ComputeContentHash(subject, body string, attachments []ContentHashAttachment) string {
	// Compute per-attachment hashes
	attHashes := make([]string, 0, len(attachments))
	for _, att := range attachments {
		contentType := strings.ToLower(att.ContentType)
		h := sha256.New()
		h.Write([]byte(att.Filename))
		h.Write([]byte(":"))
		h.Write([]byte(contentType))
		h.Write([]byte(":"))
		h.Write(att.Data)
		attHashes = append(attHashes, fmt.Sprintf("sha256:%x", h.Sum(nil)))
	}
	sort.Strings(attHashes)

	// Compute overall content hash
	h := sha256.New()
	h.Write([]byte(subject))
	h.Write([]byte("\n"))
	h.Write([]byte(body))
	for _, ah := range attHashes {
		h.Write([]byte("\n"))
		h.Write([]byte(ah))
	}
	return fmt.Sprintf("sha256:%x", h.Sum(nil))
}
