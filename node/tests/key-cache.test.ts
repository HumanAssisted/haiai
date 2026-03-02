import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { HaiClient } from '../src/client.js';
import { generateKeypair } from '../src/crypt.js';

function makeClient(): HaiClient {
  const keypair = generateKeypair();
  return HaiClient.fromCredentials('test-agent', keypair.privateKeyPem, { url: 'https://hai.example' });
}

const KEY_RESPONSE_V1 = {
  jacs_id: 'agent-abc',
  version: 'v1',
  public_key: 'pem-v1',
  public_key_raw_b64: '',
  algorithm: 'Ed25519',
  public_key_hash: 'sha256:abc',
  status: 'active',
  dns_verified: true,
  created_at: '2026-01-15T10:30:00Z',
};

const KEY_RESPONSE_V2 = {
  ...KEY_RESPONSE_V1,
  version: 'v2',
  public_key: 'pem-v2',
};

describe('agent key cache', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  // -----------------------------------------------------------------------
  // fetchRemoteKey caching
  // -----------------------------------------------------------------------

  describe('fetchRemoteKey cache', () => {
    it('caches result and avoids second HTTP call', async () => {
      const client = makeClient();
      const fetchMock = vi.fn()
        .mockResolvedValueOnce(new Response(JSON.stringify(KEY_RESPONSE_V1), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }))
        .mockResolvedValueOnce(new Response(JSON.stringify(KEY_RESPONSE_V2), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }));
      vi.stubGlobal('fetch', fetchMock);

      const r1 = await client.fetchRemoteKey('agent-abc', 'latest');
      const r2 = await client.fetchRemoteKey('agent-abc', 'latest');

      expect(fetchMock).toHaveBeenCalledTimes(1);
      expect(r1.version).toBe('v1');
      expect(r2.version).toBe('v1');
    });

    it('refetches after cache TTL expires', async () => {
      vi.useFakeTimers();
      const client = makeClient();

      const fetchMock = vi.fn()
        .mockResolvedValueOnce(new Response(JSON.stringify(KEY_RESPONSE_V1), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }))
        .mockResolvedValueOnce(new Response(JSON.stringify(KEY_RESPONSE_V2), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }));
      vi.stubGlobal('fetch', fetchMock);

      const r1 = await client.fetchRemoteKey('agent-abc', 'latest');
      expect(r1.version).toBe('v1');

      // Advance past TTL (5 minutes = 300,000ms)
      vi.advanceTimersByTime(300_001);

      const r2 = await client.fetchRemoteKey('agent-abc', 'latest');
      expect(r2.version).toBe('v2');
      expect(fetchMock).toHaveBeenCalledTimes(2);
    });

    it('clearAgentKeyCache forces refetch', async () => {
      const client = makeClient();

      const fetchMock = vi.fn()
        .mockResolvedValueOnce(new Response(JSON.stringify(KEY_RESPONSE_V1), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }))
        .mockResolvedValueOnce(new Response(JSON.stringify(KEY_RESPONSE_V2), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }));
      vi.stubGlobal('fetch', fetchMock);

      const r1 = await client.fetchRemoteKey('agent-abc', 'latest');
      expect(r1.version).toBe('v1');

      client.clearAgentKeyCache();

      const r2 = await client.fetchRemoteKey('agent-abc', 'latest');
      expect(r2.version).toBe('v2');
      expect(fetchMock).toHaveBeenCalledTimes(2);
    });

    it('caches different (id, version) pairs separately', async () => {
      const client = makeClient();

      const fetchMock = vi.fn()
        .mockResolvedValueOnce(new Response(JSON.stringify(KEY_RESPONSE_V1), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }))
        .mockResolvedValueOnce(new Response(JSON.stringify(KEY_RESPONSE_V2), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }));
      vi.stubGlobal('fetch', fetchMock);

      const r1 = await client.fetchRemoteKey('agent-1', 'latest');
      const r2 = await client.fetchRemoteKey('agent-2', 'latest');

      expect(fetchMock).toHaveBeenCalledTimes(2);
      expect(r1.version).toBe('v1');
      expect(r2.version).toBe('v2');

      // Repeated calls should use cache
      await client.fetchRemoteKey('agent-1', 'latest');
      await client.fetchRemoteKey('agent-2', 'latest');
      expect(fetchMock).toHaveBeenCalledTimes(2);
    });
  });

  // -----------------------------------------------------------------------
  // fetchKeyByHash caching
  // -----------------------------------------------------------------------

  describe('fetchKeyByHash cache', () => {
    it('caches result on second call', async () => {
      const client = makeClient();
      const fetchMock = vi.fn()
        .mockResolvedValue(new Response(JSON.stringify(KEY_RESPONSE_V1), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }));
      vi.stubGlobal('fetch', fetchMock);

      await client.fetchKeyByHash('sha256:abc');
      await client.fetchKeyByHash('sha256:abc');

      expect(fetchMock).toHaveBeenCalledTimes(1);
    });
  });

  // -----------------------------------------------------------------------
  // fetchKeyByEmail caching
  // -----------------------------------------------------------------------

  describe('fetchKeyByEmail cache', () => {
    it('caches result on second call', async () => {
      const client = makeClient();
      const fetchMock = vi.fn()
        .mockResolvedValue(new Response(JSON.stringify(KEY_RESPONSE_V1), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }));
      vi.stubGlobal('fetch', fetchMock);

      await client.fetchKeyByEmail('alice@hai.ai');
      await client.fetchKeyByEmail('alice@hai.ai');

      expect(fetchMock).toHaveBeenCalledTimes(1);
    });
  });

  // -----------------------------------------------------------------------
  // fetchKeyByDomain caching
  // -----------------------------------------------------------------------

  describe('fetchKeyByDomain cache', () => {
    it('caches result on second call', async () => {
      const client = makeClient();
      const fetchMock = vi.fn()
        .mockResolvedValue(new Response(JSON.stringify(KEY_RESPONSE_V1), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }));
      vi.stubGlobal('fetch', fetchMock);

      await client.fetchKeyByDomain('example.com');
      await client.fetchKeyByDomain('example.com');

      expect(fetchMock).toHaveBeenCalledTimes(1);
    });
  });
});
