import { afterEach, describe, expect, it, vi } from 'vitest';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { verifyEmailSignature, parseJacsSignatureHeader, computeContentHash } from '../src/signing.js';
import { generateKeypair, signString } from '../src/crypt.js';

const here = dirname(fileURLToPath(import.meta.url));

function loadFixture(): Record<string, unknown> {
  const fixturePath = resolve(here, '../../contract/email_verification_example.json');
  return JSON.parse(readFileSync(fixturePath, 'utf-8')) as Record<string, unknown>;
}

function mockRegistryResponse(
  fixture: Record<string, unknown>,
  overrides: { jacsId?: string } = {},
): void {
  const headers = fixture.headers as Record<string, string>;
  const fetchMock = vi.fn(async (url: string | URL) => {
    return new Response(JSON.stringify({
      email: headers.From,
      jacs_id: overrides.jacsId ?? 'test-agent-jacs-id',
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

/** Mock the registry fetch to return a specific public key PEM for a given jacsId. */
function mockRegistryForKey(
  publicKeyPem: string,
  jacsId: string,
  reputationTier: string = 'established',
): void {
  const fetchMock = vi.fn(async () => {
    return new Response(JSON.stringify({
      email: 'sender@hai.ai',
      jacs_id: jacsId,
      public_key: publicKeyPem,
      algorithm: 'ed25519',
      reputation_tier: reputationTier,
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

  it('rejects missing From header', async () => {
    const result = await verifyEmailSignature(
      { 'X-JACS-Signature': 'v=1; a=ed25519; id=x; t=1; s=abc', 'X-JACS-Content-Hash': 'sha256:abc' },
      'Test',
      'Body',
    );
    expect(result.valid).toBe(false);
    expect(result.error).toBe('Missing From header');
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

  it('rejects id mismatch between signature header and registry', async () => {
    const fixture = loadFixture();
    mockRegistryResponse(fixture, { jacsId: 'different-agent-id' });

    vi.useFakeTimers();
    vi.setSystemTime(new Date((1740393600 + 100) * 1000));

    const headers = fixture.headers as Record<string, string>;
    const result = await verifyEmailSignature(
      headers,
      fixture.subject as string,
      fixture.body as string,
    );

    expect(result.valid).toBe(false);
    expect(result.error).toBe('Signature id does not match registry jacs_id');
  });

  // =========================================================================
  // New: generated-keypair v1/v2 tests
  // =========================================================================

  it('verifies v1 signed email', async () => {
    const keypair = generateKeypair();
    const jacsId = 'v1-test-agent';
    const subject = 'V1 Subject';
    const body = 'V1 body text';
    const timestamp = Math.floor(Date.now() / 1000);

    // v1 content hash: sha256(subject + "\n" + body)
    const contentHash = computeContentHash(subject, body);

    // v1 signing payload: "{content_hash}:{timestamp}"
    const signInput = `${contentHash}:${timestamp}`;
    const signature = signString(keypair.privateKeyPem, signInput);

    const sigHeader = `v=1; a=ed25519; id=${jacsId}; t=${timestamp}; s=${signature}`;
    const headers: Record<string, string> = {
      'X-JACS-Signature': sigHeader,
      'X-JACS-Content-Hash': contentHash,
      'From': 'sender@hai.ai',
    };

    mockRegistryForKey(keypair.publicKeyPem, jacsId);

    const result = await verifyEmailSignature(headers, subject, body, 'https://hai.ai');

    expect(result.valid).toBe(true);
    expect(result.jacsId).toBe(jacsId);
    expect(result.reputationTier).toBe('established');
    expect(result.error).toBeNull();
  });

  it('verifies v2 signed email', async () => {
    const keypair = generateKeypair();
    const jacsId = 'v2-test-agent';
    const fromEmail = 'sender@hai.ai';
    const subject = 'V2 Subject';
    const body = 'V2 body text';
    const timestamp = Math.floor(Date.now() / 1000);

    // v2 content hash
    const contentHash = computeContentHash(subject, body);

    // v2 signing payload: "{content_hash}:{from_email}:{timestamp}"
    const signInput = `${contentHash}:${fromEmail}:${timestamp}`;
    const signature = signString(keypair.privateKeyPem, signInput);

    // v2 header includes h= and from= fields
    const sigHeader = `v=2; a=ed25519; id=${jacsId}; t=${timestamp}; h=${contentHash}; from=${fromEmail}; s=${signature}`;
    const headers: Record<string, string> = {
      'X-JACS-Signature': sigHeader,
      'From': fromEmail,
    };

    mockRegistryForKey(keypair.publicKeyPem, jacsId);

    const result = await verifyEmailSignature(headers, subject, body, 'https://hai.ai');

    expect(result.valid).toBe(true);
    expect(result.jacsId).toBe(jacsId);
    expect(result.reputationTier).toBe('established');
    expect(result.error).toBeNull();
  });

  it('rejects tampered body', async () => {
    const keypair = generateKeypair();
    const jacsId = 'tamper-agent';
    const subject = 'Tamper Test';
    const body = 'Original body';
    const timestamp = Math.floor(Date.now() / 1000);

    const contentHash = computeContentHash(subject, body);
    const signInput = `${contentHash}:${timestamp}`;
    const signature = signString(keypair.privateKeyPem, signInput);

    const sigHeader = `v=1; a=ed25519; id=${jacsId}; t=${timestamp}; s=${signature}`;
    const headers: Record<string, string> = {
      'X-JACS-Signature': sigHeader,
      'X-JACS-Content-Hash': contentHash,
      'From': 'sender@hai.ai',
    };

    mockRegistryForKey(keypair.publicKeyPem, jacsId);

    // Pass tampered body -- content hash recomputation will mismatch
    const result = await verifyEmailSignature(headers, subject, 'TAMPERED body', 'https://hai.ai');

    expect(result.valid).toBe(false);
    expect(result.error).toBe('Content hash mismatch');
  });

  it('rejects expired timestamp', async () => {
    const keypair = generateKeypair();
    const jacsId = 'expired-agent';
    const subject = 'Expired Test';
    const body = 'Body';
    // Timestamp 48 hours in the past
    const timestamp = Math.floor(Date.now() / 1000) - 48 * 3600;

    const contentHash = computeContentHash(subject, body);
    const signInput = `${contentHash}:${timestamp}`;
    const signature = signString(keypair.privateKeyPem, signInput);

    const sigHeader = `v=1; a=ed25519; id=${jacsId}; t=${timestamp}; s=${signature}`;
    const headers: Record<string, string> = {
      'X-JACS-Signature': sigHeader,
      'X-JACS-Content-Hash': contentHash,
      'From': 'sender@hai.ai',
    };

    mockRegistryForKey(keypair.publicKeyPem, jacsId);

    const result = await verifyEmailSignature(headers, subject, body, 'https://hai.ai');

    expect(result.valid).toBe(false);
    expect(result.error).toContain('too old');
  });

  it('handles missing X-JACS-Content-Hash for v1', async () => {
    // v1 header (no h= field) without X-JACS-Content-Hash => error
    const sigHeader = 'v=1; a=ed25519; id=agent-x; t=1740000000; s=fakesig';
    const headers: Record<string, string> = {
      'X-JACS-Signature': sigHeader,
      'From': 'sender@hai.ai',
      // No X-JACS-Content-Hash
    };

    const result = await verifyEmailSignature(headers, 'Sub', 'Body', 'https://hai.ai');

    expect(result.valid).toBe(false);
    expect(result.error).toContain('Missing X-JACS-Content-Hash');
  });

  it('v2 uses h= field instead of X-JACS-Content-Hash', async () => {
    const keypair = generateKeypair();
    const jacsId = 'v2-h-field-agent';
    const fromEmail = 'sender@hai.ai';
    const subject = 'V2 Hash Test';
    const body = 'V2 hash body';
    const timestamp = Math.floor(Date.now() / 1000);

    const contentHash = computeContentHash(subject, body);
    const signInput = `${contentHash}:${fromEmail}:${timestamp}`;
    const signature = signString(keypair.privateKeyPem, signInput);

    // v2 header with h= but NO X-JACS-Content-Hash header
    const sigHeader = `v=2; a=ed25519; id=${jacsId}; t=${timestamp}; h=${contentHash}; from=${fromEmail}; s=${signature}`;
    const headers: Record<string, string> = {
      'X-JACS-Signature': sigHeader,
      'From': fromEmail,
      // No X-JACS-Content-Hash header
    };

    mockRegistryForKey(keypair.publicKeyPem, jacsId);

    const result = await verifyEmailSignature(headers, subject, body, 'https://hai.ai');

    expect(result.valid).toBe(true);
    expect(result.jacsId).toBe(jacsId);
    expect(result.error).toBeNull();
  });
});
