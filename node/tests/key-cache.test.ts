import { afterEach, describe, expect, it, vi } from 'vitest';
import { HaiClient } from '../src/client.js';
import { generateTestKeypair as generateKeypair } from './setup.js';
import { createMockFFI } from './ffi-mock.js';

async function makeClient(): Promise<HaiClient> {
  const keypair = generateKeypair();
  return HaiClient.fromCredentials('test-agent', keypair.privateKeyPem, { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' });
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
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  // -----------------------------------------------------------------------
  // fetchRemoteKey caching
  // -----------------------------------------------------------------------

  describe('fetchRemoteKey cache', () => {
    it('caches result and avoids second FFI call', async () => {
      const client = await makeClient();
      const fetchRemoteKeyMock = vi.fn()
        .mockResolvedValueOnce(KEY_RESPONSE_V1)
        .mockResolvedValueOnce(KEY_RESPONSE_V2);
      client._setFFIAdapter(createMockFFI({ fetchRemoteKey: fetchRemoteKeyMock }));

      const r1 = await client.fetchRemoteKey('agent-abc', 'latest');
      const r2 = await client.fetchRemoteKey('agent-abc', 'latest');

      expect(fetchRemoteKeyMock).toHaveBeenCalledTimes(1);
      expect(r1.version).toBe('v1');
      expect(r2.version).toBe('v1');
    });

    it('refetches after cache TTL expires', async () => {
      vi.useFakeTimers();
      const client = await makeClient();

      const fetchRemoteKeyMock = vi.fn()
        .mockResolvedValueOnce(KEY_RESPONSE_V1)
        .mockResolvedValueOnce(KEY_RESPONSE_V2);
      client._setFFIAdapter(createMockFFI({ fetchRemoteKey: fetchRemoteKeyMock }));

      const r1 = await client.fetchRemoteKey('agent-abc', 'latest');
      expect(r1.version).toBe('v1');

      // Advance past TTL (5 minutes = 300,000ms)
      vi.advanceTimersByTime(300_001);

      const r2 = await client.fetchRemoteKey('agent-abc', 'latest');
      expect(r2.version).toBe('v2');
      expect(fetchRemoteKeyMock).toHaveBeenCalledTimes(2);
    });

    it('clearAgentKeyCache forces refetch', async () => {
      const client = await makeClient();

      const fetchRemoteKeyMock = vi.fn()
        .mockResolvedValueOnce(KEY_RESPONSE_V1)
        .mockResolvedValueOnce(KEY_RESPONSE_V2);
      client._setFFIAdapter(createMockFFI({ fetchRemoteKey: fetchRemoteKeyMock }));

      const r1 = await client.fetchRemoteKey('agent-abc', 'latest');
      expect(r1.version).toBe('v1');

      client.clearAgentKeyCache();

      const r2 = await client.fetchRemoteKey('agent-abc', 'latest');
      expect(r2.version).toBe('v2');
      expect(fetchRemoteKeyMock).toHaveBeenCalledTimes(2);
    });

    it('caches different (id, version) pairs separately', async () => {
      const client = await makeClient();

      const fetchRemoteKeyMock = vi.fn()
        .mockResolvedValueOnce(KEY_RESPONSE_V1)
        .mockResolvedValueOnce(KEY_RESPONSE_V2);
      client._setFFIAdapter(createMockFFI({ fetchRemoteKey: fetchRemoteKeyMock }));

      const r1 = await client.fetchRemoteKey('agent-1', 'latest');
      const r2 = await client.fetchRemoteKey('agent-2', 'latest');

      expect(fetchRemoteKeyMock).toHaveBeenCalledTimes(2);
      expect(r1.version).toBe('v1');
      expect(r2.version).toBe('v2');

      // Repeated calls should use cache
      await client.fetchRemoteKey('agent-1', 'latest');
      await client.fetchRemoteKey('agent-2', 'latest');
      expect(fetchRemoteKeyMock).toHaveBeenCalledTimes(2);
    });
  });

  // -----------------------------------------------------------------------
  // fetchKeyByHash caching
  // -----------------------------------------------------------------------

  describe('fetchKeyByHash cache', () => {
    it('caches result on second call', async () => {
      const client = await makeClient();
      const fetchKeyByHashMock = vi.fn().mockResolvedValue(KEY_RESPONSE_V1);
      client._setFFIAdapter(createMockFFI({ fetchKeyByHash: fetchKeyByHashMock }));

      await client.fetchKeyByHash('sha256:abc');
      await client.fetchKeyByHash('sha256:abc');

      expect(fetchKeyByHashMock).toHaveBeenCalledTimes(1);
    });
  });

  // -----------------------------------------------------------------------
  // fetchKeyByEmail caching
  // -----------------------------------------------------------------------

  describe('fetchKeyByEmail cache', () => {
    it('caches result on second call', async () => {
      const client = await makeClient();
      const fetchKeyByEmailMock = vi.fn().mockResolvedValue(KEY_RESPONSE_V1);
      client._setFFIAdapter(createMockFFI({ fetchKeyByEmail: fetchKeyByEmailMock }));

      await client.fetchKeyByEmail('alice@hai.ai');
      await client.fetchKeyByEmail('alice@hai.ai');

      expect(fetchKeyByEmailMock).toHaveBeenCalledTimes(1);
    });
  });

  // -----------------------------------------------------------------------
  // fetchKeyByDomain caching
  // -----------------------------------------------------------------------

  describe('fetchKeyByDomain cache', () => {
    it('caches result on second call', async () => {
      const client = await makeClient();
      const fetchKeyByDomainMock = vi.fn().mockResolvedValue(KEY_RESPONSE_V1);
      client._setFFIAdapter(createMockFFI({ fetchKeyByDomain: fetchKeyByDomainMock }));

      await client.fetchKeyByDomain('example.com');
      await client.fetchKeyByDomain('example.com');

      expect(fetchKeyByDomainMock).toHaveBeenCalledTimes(1);
    });
  });
});
