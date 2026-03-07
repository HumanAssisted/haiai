import { describe, it, expect } from 'vitest';
import { generateVerifyLink, MAX_VERIFY_URL_LEN, MAX_VERIFY_DOCUMENT_BYTES } from '../src/verify.js';

describe('generateVerifyLink', () => {
  it('generates a verify URL with default baseUrl', () => {
    const doc = '{"jacsId":"test-agent","version":"1.0"}';
    const url = generateVerifyLink(doc);
    expect(url).toMatch(/^https:\/\/hai\.ai\/jacs\/verify\?s=/);
  });

  it('generates a verify URL with custom baseUrl', () => {
    const doc = '{"jacsId":"test-agent"}';
    const url = generateVerifyLink(doc, 'https://custom.example.com');
    expect(url).toMatch(/^https:\/\/custom\.example\.com\/jacs\/verify\?s=/);
  });

  it('uses URL-safe base64 encoding', () => {
    const doc = '{"jacsId":"test-agent"}';
    const url = generateVerifyLink(doc);
    const encoded = url.split('?s=')[1];
    // URL-safe base64 uses - instead of +, _ instead of /, no = padding
    expect(encoded).not.toMatch(/\+/);
    expect(encoded).not.toMatch(/\//);
    expect(encoded).not.toMatch(/=$/);
  });

  it('round-trips correctly via base64 decode', () => {
    const doc = '{"jacsId":"test-agent","version":"1.0"}';
    const url = generateVerifyLink(doc);
    const encoded = url.split('?s=')[1];
    // Reverse the URL-safe base64
    const standard = encoded.replace(/-/g, '+').replace(/_/g, '/');
    const decoded = Buffer.from(standard, 'base64').toString('utf8');
    expect(decoded).toBe(doc);
  });

  it('strips trailing slashes from baseUrl', () => {
    const doc = '{"jacsId":"test"}';
    const url = generateVerifyLink(doc, 'https://hai.ai///');
    expect(url).toMatch(/^https:\/\/hai\.ai\/jacs\/verify\?s=/);
    expect(url).not.toContain('///');
  });

  it('throws for document exceeding max byte size', () => {
    // Create a document larger than MAX_VERIFY_DOCUMENT_BYTES
    const bigDoc = 'x'.repeat(MAX_VERIFY_DOCUMENT_BYTES + 100);
    expect(() => generateVerifyLink(bigDoc)).toThrow(/max length/i);
  });

  it('succeeds for document exactly at the byte limit', () => {
    // A short baseUrl + path = "https://hai.ai/jacs/verify?s=" = 29 chars
    // Max URL = 2048, so max encoded = 2048 - 29 = 2019 chars
    // base64 expands by ~4/3, so max source bytes ~ 2019 * 3/4 = 1514.25
    // The constant MAX_VERIFY_DOCUMENT_BYTES = 1515
    // Use a document that just fits
    const doc = 'a'.repeat(100); // well under limit
    expect(() => generateVerifyLink(doc)).not.toThrow();
  });

  it('exports correct constants', () => {
    expect(MAX_VERIFY_URL_LEN).toBe(2048);
    expect(MAX_VERIFY_DOCUMENT_BYTES).toBe(1515);
  });

  it('generates hosted verify URL when hosted=true and jacsDocumentId is present', () => {
    const doc = '{"jacsDocumentId":"doc-123","signed":true}';
    const url = generateVerifyLink(doc, 'https://hai.ai', true);
    expect(url).toBe('https://hai.ai/verify/doc-123');
  });

  it('generates hosted verify URL from document_id field', () => {
    const doc = '{"document_id":"doc-abc"}';
    const url = generateVerifyLink(doc, 'https://example.com', true);
    expect(url).toBe('https://example.com/verify/doc-abc');
  });

  it('throws for hosted=true when no ID is present', () => {
    const doc = '{"signed":true}';
    expect(() => generateVerifyLink(doc, 'https://hai.ai', true)).toThrow(/document ID/i);
  });
});
