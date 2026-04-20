import { afterEach, describe, expect, it, vi } from 'vitest';
import { HaiClient } from '../src/client.js';
import { HaiError, HaiConnectionError, RateLimitedError } from '../src/errors.js';
import { generateTestKeypair } from './setup.js';
import { createSseBody } from './setup.js';
import { createMockFFI } from './ffi-mock.js';
import { mapFFIError } from '../src/ffi-client.js';

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
// #5 — 429 retry is now handled by the Rust FFI layer.
// These tests verify the FFI error mapping preserves RateLimitedError.
// =============================================================================
describe('#5: FFI rate-limit error mapping', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('retries are handled by FFI; successful call returns result', async () => {
    const client = await makeClient('agent-429', { maxRetries: 3 });

    const helloMock = vi.fn(async () => ({
      timestamp: '2026-01-01T00:00:00Z',
      client_ip: '127.0.0.1',
      message: 'ok',
      hello_id: 'h1',
    }));
    client._setFFIAdapter(createMockFFI({ hello: helloMock }));

    const result = await client.hello();
    expect(result.message).toBe('ok');
  });

  it('throws RateLimitedError when FFI reports rate limit', async () => {
    const client = await makeClient('agent-429-exhaust', { maxRetries: 3 });

    const helloMock = vi.fn(async () => {
      throw mapFFIError(new Error('RateLimited: Rate limited'));
    });
    client._setFFIAdapter(createMockFFI({ hello: helloMock }));

    await expect(client.hello()).rejects.toThrow(RateLimitedError);
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
// #14 — SSE/WS max reconnect attempts (uses FFI connectSse)
// =============================================================================
describe('#14: SSE/WS max reconnect attempts', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('stops reconnecting SSE after maxReconnectAttempts', async () => {
    vi.useFakeTimers();
    const client = await makeClient('agent-sse', { maxReconnectAttempts: 3 });

    let connectCallCount = 0;
    const connectSseMock = vi.fn(async () => {
      connectCallCount++;
      throw new HaiConnectionError('SSE connection failed');
    });
    client._setFFIAdapter(createMockFFI({ connectSse: connectSseMock }));

    // Attach the rejection handler immediately to avoid unhandled rejection.
    const connectPromise = (async () => {
      const events: unknown[] = [];
      for await (const event of client.connect({ transport: 'sse' })) {
        events.push(event);
      }
    })().catch((err: unknown) => { throw err; });

    // Keep a reference to the caught promise to avoid unhandled rejection warnings.
    const settled = connectPromise.catch(() => {});

    // Advance through all reconnect backoff delays until the error propagates.
    // Each failed attempt triggers exponential backoff (1s, 2s, 4s, ...).
    for (let i = 0; i < 10; i++) {
      await vi.advanceTimersByTimeAsync(60_000);
    }

    await settled;
    await expect(connectPromise).rejects.toThrow(HaiConnectionError);

    // Initial attempt + maxReconnectAttempts retries = 4 total
    expect(connectCallCount).toBe(4);
  });

  it('stops reconnecting WS after maxReconnectAttempts', async () => {
    vi.useFakeTimers();
    const client = await makeClient('agent-ws', { maxReconnectAttempts: 2 });

    let connectCallCount = 0;
    const connectWsMock = vi.fn(async () => {
      connectCallCount++;
      throw new HaiConnectionError('WS connection failed');
    });
    client._setFFIAdapter(createMockFFI({ connectWs: connectWsMock }));

    const connectPromise = (async () => {
      const events: unknown[] = [];
      for await (const event of client.connect({ transport: 'ws' })) {
        events.push(event);
      }
    })().catch((err: unknown) => { throw err; });

    const settled = connectPromise.catch(() => {});

    for (let i = 0; i < 10; i++) {
      await vi.advanceTimersByTimeAsync(60_000);
    }

    await settled;
    await expect(connectPromise).rejects.toThrow(HaiConnectionError);
    expect(connectCallCount).toBe(3);
  });

  it.each([
    ['sse', 'connectSse', 'sseClose'],
    ['ws', 'connectWs', 'wsClose'],
  ] as const)('yields %s events and closes the opaque handle once', async (
    transport,
    connectMethod,
    closeMethod,
  ) => {
    const client = await makeClient(`agent-${transport}`);

    const closeMock = vi.fn(async () => {});
    const nextMethod = transport === 'sse' ? 'sseNextEvent' : 'wsNextEvent';
    const connectMock = vi.fn(async () => 42);
    const nextMock = vi.fn(async () => ({
      event_type: 'connected',
      data: { agent_id: `agent-${transport}` },
      id: `${transport}-evt-1`,
      raw: '{"agent_id":"test"}',
    }));

    client._setFFIAdapter(createMockFFI({
      [closeMethod]: closeMock,
      [connectMethod]: connectMock,
      [nextMethod]: nextMock,
    }));

    const stream = client.connect({ transport });
    const first = await stream.next();
    expect(first.done).toBe(false);
    expect(first.value?.eventType).toBe('connected');
    expect(first.value?.id).toBe(`${transport}-evt-1`);

    await stream.return(undefined);

    expect(connectMock).toHaveBeenCalledTimes(1);
    expect(closeMock).toHaveBeenCalledTimes(1);
    expect(closeMock).toHaveBeenCalledWith(42);
  });
});

// =============================================================================
// #18 — searchMessages and listMessages query params
// =============================================================================
describe('#18: search/list messages support has_attachments, since, until', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('listMessages passes has_attachments, since, until to FFI', async () => {
    const client = await makeClient();

    const listMessagesMock = vi.fn(async (options: Record<string, unknown>) => {
      expect(options.has_attachments).toBe(true);
      expect(options.since).toBe('2026-01-01T00:00:00Z');
      expect(options.until).toBe('2026-03-01T00:00:00Z');
      return [];
    });
    client._setFFIAdapter(createMockFFI({ listMessages: listMessagesMock }));

    await client.listMessages({
      hasAttachments: true,
      since: '2026-01-01T00:00:00Z',
      until: '2026-03-01T00:00:00Z',
    });
    expect(listMessagesMock).toHaveBeenCalledTimes(1);
  });

  it('searchMessages passes has_attachments, since, until to FFI', async () => {
    const client = await makeClient();

    const searchMessagesMock = vi.fn(async (options: Record<string, unknown>) => {
      expect(options.has_attachments).toBe(true);
      expect(options.since).toBe('2026-01-01T00:00:00Z');
      expect(options.until).toBe('2026-03-01T00:00:00Z');
      return [];
    });
    client._setFFIAdapter(createMockFFI({ searchMessages: searchMessagesMock }));

    await client.searchMessages({
      query: 'test',
      hasAttachments: true,
      since: '2026-01-01T00:00:00Z',
      until: '2026-03-01T00:00:00Z',
    });
    expect(searchMessagesMock).toHaveBeenCalledTimes(1);
  });
});
