import { afterEach, describe, expect, it, vi } from 'vitest';
import { HaiClient } from '../src/client.js';
import { generateKeypair } from '../src/crypt.js';

function makeClient(jacsId: string = 'test-agent'): HaiClient {
  const keypair = generateKeypair();
  return HaiClient.fromCredentials(jacsId, keypair.privateKeyPem, { url: 'https://hai.example' });
}

function stubFetch(expectedUrl: string, payload: Record<string, unknown> = {}): void {
  const fetchMock = vi.fn(async (url: string | URL) => {
    expect(String(url)).toBe(expectedUrl);
    return new Response(JSON.stringify(payload), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  });
  vi.stubGlobal('fetch', fetchMock);
}

function stubFetch404(expectedUrl: string): void {
  const fetchMock = vi.fn(async (url: string | URL) => {
    expect(String(url)).toBe(expectedUrl);
    return new Response(JSON.stringify({ error: 'not found' }), {
      status: 404,
      headers: { 'Content-Type': 'application/json' },
    });
  });
  vi.stubGlobal('fetch', fetchMock);
}

const KEY_RESPONSE = {
  jacs_id: 'agent-abc',
  version: 'v1',
  public_key: '-----BEGIN PUBLIC KEY-----\nZm9v\n-----END PUBLIC KEY-----\n',
  public_key_raw_b64: 'Zm9v',
  algorithm: 'Ed25519',
  public_key_hash: 'sha256:abcdef1234567890',
  status: 'active',
  dns_verified: true,
  created_at: '2026-01-15T10:30:00Z',
};

const KEY_HISTORY_RESPONSE = {
  jacs_id: 'agent-abc',
  keys: [
    KEY_RESPONSE,
    { ...KEY_RESPONSE, version: 'v0', status: 'rotated' },
  ],
  total: 2,
};

describe('key lookup methods', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  // -----------------------------------------------------------------------
  // fetchKeyByHash
  // -----------------------------------------------------------------------

  describe('fetchKeyByHash', () => {
    it('calls correct URL and parses response', async () => {
      const client = makeClient();
      stubFetch(
        'https://hai.example/jacs/v1/keys/by-hash/sha256%3Aabcdef1234567890',
        KEY_RESPONSE,
      );

      const result = await client.fetchKeyByHash('sha256:abcdef1234567890');

      expect(result.jacsId).toBe('agent-abc');
      expect(result.algorithm).toBe('Ed25519');
      expect(result.publicKeyHash).toBe('sha256:abcdef1234567890');
      expect(result.dnsVerified).toBe(true);
    });

    it('escapes path-traversal characters in hash', async () => {
      const client = makeClient();
      stubFetch(
        'https://hai.example/jacs/v1/keys/by-hash/sha256%3A..%2F..%2Fetc%2Fpasswd',
        KEY_RESPONSE,
      );

      await client.fetchKeyByHash('sha256:../../etc/passwd');
    });

    it('rejects on 404', async () => {
      const client = makeClient();
      stubFetch404('https://hai.example/jacs/v1/keys/by-hash/sha256%3Amissing');

      await expect(client.fetchKeyByHash('sha256:missing')).rejects.toThrow();
    });
  });

  // -----------------------------------------------------------------------
  // fetchKeyByEmail
  // -----------------------------------------------------------------------

  describe('fetchKeyByEmail', () => {
    it('calls correct URL and parses response', async () => {
      const client = makeClient();
      stubFetch(
        'https://hai.example/api/agents/keys/alice%40hai.ai',
        KEY_RESPONSE,
      );

      const result = await client.fetchKeyByEmail('alice@hai.ai');

      expect(result.jacsId).toBe('agent-abc');
      expect(result.version).toBe('v1');
    });

    it('rejects on 404', async () => {
      const client = makeClient();
      stubFetch404('https://hai.example/api/agents/keys/nobody%40hai.ai');

      await expect(client.fetchKeyByEmail('nobody@hai.ai')).rejects.toThrow();
    });
  });

  // -----------------------------------------------------------------------
  // fetchKeyByDomain
  // -----------------------------------------------------------------------

  describe('fetchKeyByDomain', () => {
    it('calls correct URL and parses response', async () => {
      const client = makeClient();
      stubFetch(
        'https://hai.example/jacs/v1/agents/by-domain/example.com',
        KEY_RESPONSE,
      );

      const result = await client.fetchKeyByDomain('example.com');

      expect(result.jacsId).toBe('agent-abc');
      expect(result.dnsVerified).toBe(true);
    });

    it('rejects on 404', async () => {
      const client = makeClient();
      stubFetch404('https://hai.example/jacs/v1/agents/by-domain/nonexistent.test');

      await expect(client.fetchKeyByDomain('nonexistent.test')).rejects.toThrow();
    });
  });

  // -----------------------------------------------------------------------
  // fetchAllKeys
  // -----------------------------------------------------------------------

  describe('fetchAllKeys', () => {
    it('calls correct URL and returns structured result', async () => {
      const client = makeClient();
      stubFetch(
        'https://hai.example/jacs/v1/agents/agent-abc/keys',
        KEY_HISTORY_RESPONSE,
      );

      const result = await client.fetchAllKeys('agent-abc');

      expect(result.jacsId).toBe('agent-abc');
      expect(result.total).toBe(2);
      expect(result.keys).toHaveLength(2);
      expect(result.keys[0].version).toBe('v1');
      expect(result.keys[1].version).toBe('v0');
    });

    it('escapes jacs_id with slashes', async () => {
      const client = makeClient();
      stubFetch(
        'https://hai.example/jacs/v1/agents/agent%2Fwith%2Fslashes/keys',
        KEY_HISTORY_RESPONSE,
      );

      await client.fetchAllKeys('agent/with/slashes');
    });

    it('rejects on 404', async () => {
      const client = makeClient();
      stubFetch404('https://hai.example/jacs/v1/agents/missing-agent/keys');

      await expect(client.fetchAllKeys('missing-agent')).rejects.toThrow();
    });
  });
});
