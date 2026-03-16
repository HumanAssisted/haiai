/**
 * Content hash computation for cross-SDK email conformance.
 *
 * All SDKs must produce identical content hashes for the same inputs.
 * Algorithm mirrors JACS's compute_attachment_hash convention:
 *
 * 1. Per-attachment hash: sha256(filename_utf8 + ":" + content_type_lower + ":" + raw_bytes)
 * 2. Sort attachment hashes lexicographically
 * 3. Overall hash:
 *    - No attachments: sha256(subject + "\n" + body)
 *    - With attachments: sha256(subject + "\n" + body + "\n" + sorted_hashes.join("\n"))
 *
 * Returns "sha256:<hex>" format.
 */

import { createHash } from 'node:crypto';

/** Input for a single attachment in computeContentHash. */
export interface ContentHashAttachment {
  filename: string;
  content_type: string;
  /** Raw attachment bytes. */
  data?: Buffer;
  /** UTF-8 string data (used when data is not provided). */
  data_utf8?: string;
}

/**
 * Compute a deterministic content hash for email content.
 *
 * @param subject - Email subject line.
 * @param body - Email body text.
 * @param attachments - Attachments with filename, content_type, and data/data_utf8.
 * @returns "sha256:<hex>" hash string.
 */
export function computeContentHash(
  subject: string,
  body: string,
  attachments: ContentHashAttachment[] = [],
): string {
  // Compute per-attachment hashes
  const attHashes: string[] = attachments.map((att) => {
    const contentType = att.content_type.toLowerCase();
    const data = att.data ?? Buffer.from(att.data_utf8 ?? '', 'utf-8');

    const h = createHash('sha256');
    h.update(att.filename, 'utf-8');
    h.update(':');
    h.update(contentType, 'utf-8');
    h.update(':');
    h.update(data);
    return `sha256:${h.digest('hex')}`;
  });
  attHashes.sort();

  // Compute overall content hash
  const h = createHash('sha256');
  h.update(subject, 'utf-8');
  h.update('\n');
  h.update(body, 'utf-8');
  for (const ah of attHashes) {
    h.update('\n');
    h.update(ah, 'utf-8');
  }
  return `sha256:${h.digest('hex')}`;
}
