import { afterEach, describe, expect, it, vi } from 'vitest';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { verifyEmailSignature, parseJacsSignatureHeader } from '../src/signing.js';

const here = dirname(fileURLToPath(import.meta.url));

function loadFixture(): Record<string, unknown> {
  const fixturePath = resolve(here, '../../contract/email_verification_example.json');
  return JSON.parse(readFileSync(fixturePath, 'utf-8')) as Record<string, unknown>;
}

function mockRegistryResponse(fixture: Record<string, unknown>): void {
  const headers = fixture.headers as Record<string, string>;
  const fetchMock = vi.fn(async (url: string | URL) => {
    return new Response(JSON.stringify({
      email: headers.From,
      jacs_id: 'test-agent-jacs-id',
      public_key: fixture.test_public_key_pem,
      algorithm: 'ed25519',
      reputation_tier: 'established',
      registered_at: '2026-01-15T00:00:00Z',
    }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  });
  vi.stubGlobal('fetch', fetchMock);
}

describe('parseJacsSignatureHeader', () => {
  it('parses all fields', () => {
    const fields = parseJacsSignatureHeader('v=1; a=ed25519; id=test-agent; t=1740000000; s=base64sig');
    expect(fields.v).toBe('1');
    expect(fields.a).toBe('ed25519');
    expect(fields.id).toBe('test-agent');
    expect(fields.t).toBe('1740000000');
    expect(fields.s).toBe('base64sig');
  });
});

describe('verifyEmailSignature', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it('verifies a valid signature from contract fixture', async () => {
    const fixture = loadFixture();
    mockRegistryResponse(fixture);

    // Mock Date.now to be close to the fixture timestamp
    vi.useFakeTimers();
    vi.setSystemTime(new Date((1740393600 + 100) * 1000));

    const headers = fixture.headers as Record<string, string>;
    const result = await verifyEmailSignature(
      headers,
      fixture.subject as string,
      fixture.body as string,
      'https://hai.ai',
    );

    expect(result.valid).toBe(true);
    expect(result.jacsId).toBe('test-agent-jacs-id');
    expect(result.reputationTier).toBe('established');
    expect(result.error).toBeNull();
  });

  it('content hash matches contract fixture', () => {
    const fixture = loadFixture();
    const computed = 'sha256:' + createHash('sha256')
      .update((fixture.subject as string) + '\n' + (fixture.body as string), 'utf8')
      .digest('hex');
    expect(computed).toBe(fixture.expected_content_hash);
  });

  it('rejects content hash mismatch', async () => {
    const fixture = loadFixture();
    const headers = { ...(fixture.headers as Record<string, string>) };
    headers['X-JACS-Content-Hash'] = 'sha256:0000000000000000000000000000000000000000000000000000000000000000';

    const result = await verifyEmailSignature(
      headers,
      fixture.subject as string,
      fixture.body as string,
    );

    expect(result.valid).toBe(false);
    expect(result.error).toBe('Content hash mismatch');
  });

  it('rejects missing X-JACS-Signature header', async () => {
    const result = await verifyEmailSignature(
      { 'X-JACS-Content-Hash': 'sha256:abc', 'From': 'test@hai.ai' },
      'Test', 'Body',
    );
    expect(result.valid).toBe(false);
    expect(result.error).toContain('Missing X-JACS-Signature');
  });

  it('rejects missing X-JACS-Content-Hash header', async () => {
    const result = await verifyEmailSignature(
      { 'X-JACS-Signature': 'v=1; a=ed25519; id=x; t=1; s=abc', 'From': 'test@hai.ai' },
      'Test', 'Body',
    );
    expect(result.valid).toBe(false);
    expect(result.error).toContain('Missing X-JACS-Content-Hash');
  });

  it('rejects stale timestamp', async () => {
    const fixture = loadFixture();
    mockRegistryResponse(fixture);

    // Mock Date.now to be >24h after the fixture timestamp
    vi.useFakeTimers();
    vi.setSystemTime(new Date((1740393600 + 90000) * 1000));

    const headers = fixture.headers as Record<string, string>;
    const result = await verifyEmailSignature(
      headers,
      fixture.subject as string,
      fixture.body as string,
    );

    expect(result.valid).toBe(false);
    expect(result.error).toContain('too old');
  });

  it('rejects when registry fetch fails', async () => {
    const fixture = loadFixture();
    vi.stubGlobal('fetch', vi.fn(async () => { throw new Error('connection refused'); }));

    const headers = fixture.headers as Record<string, string>;
    const result = await verifyEmailSignature(
      headers,
      fixture.subject as string,
      fixture.body as string,
    );

    expect(result.valid).toBe(false);
    expect(result.error).toContain('Failed to fetch public key');
  });

  it('rejects tampered signature', async () => {
    const fixture = loadFixture();
    mockRegistryResponse(fixture);

    vi.useFakeTimers();
    vi.setSystemTime(new Date((1740393600 + 100) * 1000));

    const headers = { ...(fixture.headers as Record<string, string>) };
    // Tamper with the signature by prepending AAAA
    headers['X-JACS-Signature'] = headers['X-JACS-Signature'].replace('s=', 's=AAAA');

    const result = await verifyEmailSignature(
      headers,
      fixture.subject as string,
      fixture.body as string,
    );

    expect(result.valid).toBe(false);
    expect(result.error).toContain('Signature verification failed');
  });
});
