import { describe, it, expect } from 'vitest';
import { buildRfc5322Email } from '../src/mime.js';
import type { MimeSendEmailOptions } from '../src/mime.js';

describe('buildRfc5322Email', () => {
  it('produces valid RFC 5322 for simple text email', () => {
    const opts: MimeSendEmailOptions = {
      to: 'recipient@hai.ai',
      subject: 'Test Subject',
      body: 'Hello, world!',
    };
    const raw = buildRfc5322Email(opts, 'sender@hai.ai');
    const text = raw.toString('utf-8');

    expect(text).toContain('From: <sender@hai.ai>\r\n');
    expect(text).toContain('To: recipient@hai.ai\r\n');
    expect(text).toContain('Subject: Test Subject\r\n');
    expect(text).toContain('Date: ');
    expect(text).toContain('Message-ID: <');
    expect(text).toContain('MIME-Version: 1.0\r\n');
    expect(text).toContain('Content-Type: text/plain; charset=utf-8\r\n');
    expect(text).toContain('Hello, world!');
  });

  it('handles attachments', () => {
    const opts: MimeSendEmailOptions = {
      to: 'recipient@hai.ai',
      subject: 'With Attachments',
      body: 'See attached.',
      attachments: [
        {
          filename: 'file1.txt',
          contentType: 'text/plain',
          data: Buffer.from('content of file 1'),
        },
        {
          filename: 'file2.pdf',
          contentType: 'application/pdf',
          data: Buffer.from('fake pdf content'),
        },
      ],
    };

    const raw = buildRfc5322Email(opts, 'sender@hai.ai');
    const text = raw.toString('utf-8');

    expect(text).toContain('Content-Type: multipart/mixed; boundary=');
    expect(text).toContain('Content-Disposition: attachment; filename="file1.txt"');
    expect(text).toContain('Content-Disposition: attachment; filename="file2.pdf"');
    expect(text).toContain('Content-Transfer-Encoding: base64');
    expect(text).toContain('See attached.');
  });

  it('handles reply threading', () => {
    const opts: MimeSendEmailOptions = {
      to: 'recipient@hai.ai',
      subject: 'Re: Original',
      body: 'Reply body',
      inReplyTo: '<original-id@hai.ai>',
    };

    const raw = buildRfc5322Email(opts, 'sender@hai.ai');
    const text = raw.toString('utf-8');

    expect(text).toContain('In-Reply-To: <original-id@hai.ai>\r\n');
    expect(text).toContain('References: <original-id@hai.ai>\r\n');
  });

  it('sanitizes CRLF injection', () => {
    const opts: MimeSendEmailOptions = {
      to: 'recipient@hai.ai',
      subject: 'Bad\r\nBcc: attacker@evil.com',
      body: 'Body',
    };

    const raw = buildRfc5322Email(opts, 'sender@hai.ai');
    const text = raw.toString('utf-8');

    // No line should start with "Bcc:" (CRLF injection prevented)
    const lines = text.split('\r\n');
    for (const line of lines) {
      expect(line.startsWith('Bcc:')).toBe(false);
    }
    // Subject should be sanitized
    expect(text).toContain('Subject: BadBcc: attacker@evil.com\r\n');
  });

  it('uses CRLF line endings', () => {
    const opts: MimeSendEmailOptions = {
      to: 'recipient@hai.ai',
      subject: 'Test',
      body: 'Body',
    };

    const raw = buildRfc5322Email(opts, 'sender@hai.ai');
    const text = raw.toString('utf-8');

    expect(text).toContain('\r\n');
    // Split by CRLF, no remaining bare \n in any part (except within body text)
    const headerSection = text.split('\r\n\r\n')[0];
    const headerParts = headerSection.split('\r\n');
    for (const part of headerParts) {
      expect(part).not.toContain('\n');
    }
  });
});
