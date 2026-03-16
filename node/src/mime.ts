/**
 * RFC 5322 MIME email construction.
 *
 * Builds standards-compliant email messages from structured fields.
 * Zero external dependencies — uses only Node.js built-ins.
 */

import { randomUUID } from 'node:crypto';

/** Options for building an RFC 5322 email. */
export interface MimeSendEmailOptions {
  /** Recipient email address. */
  to: string;
  /** Email subject line. */
  subject: string;
  /** Email body text. */
  body: string;
  /** Message ID to reply to (for threading). */
  inReplyTo?: string;
  /** File attachments to include with the email. */
  attachments?: MimeEmailAttachment[];
}

/** An email attachment for MIME construction. */
export interface MimeEmailAttachment {
  /** Attachment file name. */
  filename: string;
  /** MIME content type. */
  contentType: string;
  /** Raw attachment data. */
  data: Buffer;
}

/**
 * Strip `\r` and `\n` from a header value to prevent CRLF injection.
 */
function sanitizeHeader(value: string): string {
  return value.replace(/[\r\n"]/g, '');
}

/**
 * Build an RFC 5322 email from structured fields.
 *
 * Produces raw bytes with CRLF line endings suitable for JACS signing
 * and parseable by standard email parsers.
 *
 * @param opts - Email options (to, subject, body, attachments, etc.)
 * @param fromEmail - The sender's email address
 * @returns Buffer containing the raw RFC 5322 email
 */
export function buildRfc5322Email(opts: MimeSendEmailOptions, fromEmail: string): Buffer {
  const safeTo = sanitizeHeader(opts.to);
  const safeFrom = sanitizeHeader(fromEmail);
  const safeSubject = sanitizeHeader(opts.subject);
  const messageId = `<${randomUUID()}@hai.ai>`;
  const date = new Date().toUTCString();

  const attachments = opts.attachments ?? [];

  if (attachments.length === 0) {
    // Simple text/plain email
    let email = '';
    email += `From: <${safeFrom}>\r\n`;
    email += `To: ${safeTo}\r\n`;
    email += `Subject: ${safeSubject}\r\n`;
    email += `Date: ${date}\r\n`;
    email += `Message-ID: ${messageId}\r\n`;
    if (opts.inReplyTo) {
      const safeReply = sanitizeHeader(opts.inReplyTo);
      email += `In-Reply-To: ${safeReply}\r\n`;
      email += `References: ${safeReply}\r\n`;
    }
    email += 'MIME-Version: 1.0\r\n';
    email += 'Content-Type: text/plain; charset=utf-8\r\n';
    email += 'Content-Transfer-Encoding: 8bit\r\n';
    email += '\r\n'; // end of headers
    email += opts.body;
    email += '\r\n';

    return Buffer.from(email, 'utf-8');
  } else {
    // multipart/mixed with text body + attachments
    const boundary = `hai-boundary-${randomUUID().replace(/-/g, '')}`;

    let email = '';
    email += `From: <${safeFrom}>\r\n`;
    email += `To: ${safeTo}\r\n`;
    email += `Subject: ${safeSubject}\r\n`;
    email += `Date: ${date}\r\n`;
    email += `Message-ID: ${messageId}\r\n`;
    if (opts.inReplyTo) {
      const safeReply = sanitizeHeader(opts.inReplyTo);
      email += `In-Reply-To: ${safeReply}\r\n`;
      email += `References: ${safeReply}\r\n`;
    }
    email += 'MIME-Version: 1.0\r\n';
    email += `Content-Type: multipart/mixed; boundary="${boundary}"\r\n`;
    email += '\r\n'; // end of headers

    // Body part
    email += `--${boundary}\r\n`;
    email += 'Content-Type: text/plain; charset=utf-8\r\n';
    email += 'Content-Transfer-Encoding: 8bit\r\n';
    email += '\r\n';
    email += opts.body;
    email += '\r\n';

    // Attachment parts
    for (const att of attachments) {
      const safeFilename = sanitizeHeader(att.filename);
      const safeContentType = sanitizeHeader(att.contentType);
      const b64 = att.data.toString('base64');

      email += `--${boundary}\r\n`;
      email += `Content-Type: ${safeContentType}; name="${safeFilename}"\r\n`;
      email += `Content-Disposition: attachment; filename="${safeFilename}"\r\n`;
      email += 'Content-Transfer-Encoding: base64\r\n';
      email += '\r\n';
      // Write base64 in 76-char lines (RFC 2045)
      for (let i = 0; i < b64.length; i += 76) {
        email += b64.slice(i, i + 76);
        email += '\r\n';
      }
    }

    // Closing boundary
    email += `--${boundary}--\r\n`;

    return Buffer.from(email, 'utf-8');
  }
}
