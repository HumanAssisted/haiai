import { afterEach, describe, expect, it, vi } from 'vitest';
import { HaiClient } from '../src/client.js';
import { HaiError, HaiConnectionError, RateLimitedError } from '../src/errors.js';
import { generateTestKeypair } from './setup.js';
import { createSseBody } from './setup.js';

async function makeClient(
  jacsId: string = 'test-agent',
  opts?: { url?: string; maxRetries?: number; maxReconnectAttempts?: number },
): Promise<HaiClient> {
  const keypair = generateTestKeypair();
  return HaiClient.fromCredentials(jacsId, keypair.privateKeyPem, {
    url: opts?.url ?? 'https://hai.example',
    privateKeyPassphrase: undefined,
    maxRetries: opts?.maxRetries,
    maxReconnectAttempts: opts?.maxReconnectAttempts,
  });
}

// =============================================================================
// #5 — 429 should be retried, not thrown immediately
// =============================================================================
describe('#5: fetchWithRetry retries on 429', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('retries 429 responses instead of throwing immediately', async () => {
    const client = await makeClient('agent-429', { maxRetries: 3 });

    let callCount = 0;
    const fetchMock = vi.fn(async () => {
      callCount++;
      if (callCount < 3) {
        return new Response('Rate limited', { status: 429 });
      }
      return new Response(JSON.stringify({ message: 'ok' }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });
    });
    vi.stubGlobal('fetch', fetchMock);

    // Should succeed after retrying the 429s
    const result = await client.hello();
    expect(callCount).toBe(3);
  });

  it('throws RateLimitedError after exhausting all retries on 429', async () => {
    const client = await makeClient('agent-429-exhaust', { maxRetries: 3 });

    const fetchMock = vi.fn(async () => {
      return new Response('Rate limited', { status: 429 });
    });
    vi.stubGlobal('fetch', fetchMock);

    await expect(client.hello()).rejects.toThrow(RateLimitedError);
    // All 3 attempts should have been made
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });
});

// =============================================================================
// #11 — Private key not enumerable on client
// =============================================================================
describe('#11: private key not stored as plain property', () => {
  it('does not expose _privateKeyPem or privateKeyPem as enumerable properties', async () => {
    const client = await makeClient();
    const keys = Object.keys(client);
    expect(keys).not.toContain('_privateKeyPem');
    expect(keys).not.toContain('privateKeyPem');
  });

  it('private key is not accessible via direct property access', async () => {
    const client = await makeClient();
    expect((client as any)._privateKeyPem).toBeUndefined();
    expect((client as any).privateKeyPem).toBeUndefined();
  });

  it('does not include private key as a top-level property in JSON serialization', async () => {
    const client = await makeClient();
    const parsed = JSON.parse(JSON.stringify(client));
    // The private key should not appear as a direct property on the client
    expect(parsed).not.toHaveProperty('_privateKeyPem');
    expect(parsed).not.toHaveProperty('privateKeyPem');
  });
});

// =============================================================================
// #12 — Temp key cleanup uses random passphrase, not hardcoded
// =============================================================================
describe('#12: register temp passphrase is random', () => {
  it('does not use hardcoded register-temp passphrase', async () => {
    // We read the source at compile time; this test verifies behavior:
    // When JACS_PRIVATE_KEY_PASSWORD is not set, the fallback should be random.
    // We can't easily test register() without mocking JACS, but we can verify
    // the source code doesn't contain the hardcoded fallback.
    const { readFileSync } = await import('node:fs');
    const source = readFileSync(
      new URL('../src/client.ts', import.meta.url),
      'utf-8',
    );
    expect(source).not.toContain("'register-temp'");
  });
});

// =============================================================================
// #13 — Base URL validation
// =============================================================================
describe('#13: base URL validation', () => {
  it('rejects a URL without http:// or https://', async () => {
    await expect(makeClient('agent', { url: 'ftp://bad.example' }))
      .rejects.toThrow(/https?:\/\//);
  });

  it('rejects a bare hostname', async () => {
    await expect(makeClient('agent', { url: 'hai.example.com' }))
      .rejects.toThrow();
  });

  it('accepts http:// URL', async () => {
    const client = await makeClient('agent', { url: 'http://localhost:8080' });
    expect(client).toBeDefined();
  });

  it('accepts https:// URL', async () => {
    const client = await makeClient('agent', { url: 'https://hai.ai' });
    expect(client).toBeDefined();
  });
});

// =============================================================================
// #14 — SSE/WS max reconnect attempts
// =============================================================================
describe('#14: SSE/WS max reconnect attempts', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('stops reconnecting SSE after maxReconnectAttempts', async () => {
    const client = await makeClient('agent-sse', { maxReconnectAttempts: 3 });

    let fetchCallCount = 0;
    const fetchMock = vi.fn(async () => {
      fetchCallCount++;
      // Simulate a connection error each time
      throw new TypeError('fetch failed');
    });
    vi.stubGlobal('fetch', fetchMock);

    const events: unknown[] = [];
    await expect(async () => {
      for await (const event of client.connect({ transport: 'sse' })) {
        events.push(event);
      }
    }).rejects.toThrow(HaiConnectionError);

    // Should have attempted exactly maxReconnectAttempts times
    expect(fetchCallCount).toBe(3);
  });
});

// =============================================================================
// #18 — searchMessages and listMessages missing query params
// =============================================================================
describe('#18: search/list messages support has_attachments, since, until', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('listMessages passes has_attachments, since, until to query string', async () => {
    const client = await makeClient();
    // Need to set haiAgentId for email methods
    (client as any)._haiAgentId = 'agent-uuid';

    const fetchMock = vi.fn(async (url: string | URL) => {
      const urlStr = String(url);
      expect(urlStr).toContain('has_attachments=true');
      expect(urlStr).toContain('since=2026-01-01T00%3A00%3A00Z');
      expect(urlStr).toContain('until=2026-03-01T00%3A00%3A00Z');
      return new Response(JSON.stringify({ messages: [] }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });
    });
    vi.stubGlobal('fetch', fetchMock);

    await client.listMessages({
      hasAttachments: true,
      since: '2026-01-01T00:00:00Z',
      until: '2026-03-01T00:00:00Z',
    });
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it('searchMessages passes has_attachments, since, until to query string', async () => {
    const client = await makeClient();
    (client as any)._haiAgentId = 'agent-uuid';

    const fetchMock = vi.fn(async (url: string | URL) => {
      const urlStr = String(url);
      expect(urlStr).toContain('has_attachments=true');
      expect(urlStr).toContain('since=2026-01-01T00%3A00%3A00Z');
      expect(urlStr).toContain('until=2026-03-01T00%3A00%3A00Z');
      return new Response(JSON.stringify({ messages: [] }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });
    });
    vi.stubGlobal('fetch', fetchMock);

    await client.searchMessages({
      query: 'test',
      hasAttachments: true,
      since: '2026-01-01T00:00:00Z',
      until: '2026-03-01T00:00:00Z',
    });
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});
