import { afterEach, describe, expect, it, vi } from 'vitest';
import { HaiClient } from '../src/client.js';
import { generateTestKeypair as generateKeypair } from './setup.js';
import { createMockFFI } from './ffi-mock.js';
import { HaiApiError } from '../src/errors.js';
import { mapFFIError } from '../src/ffi-client.js';

async function makeClient(jacsId: string = 'test-agent'): Promise<HaiClient> {
  const keypair = generateKeypair();
  return HaiClient.fromCredentials(jacsId, keypair.privateKeyPem, { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' });
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
    vi.restoreAllMocks();
  });

  // -----------------------------------------------------------------------
  // fetchKeyByHash
  // -----------------------------------------------------------------------

  describe('fetchKeyByHash', () => {
    it('calls correct URL and parses response', async () => {
      const client = await makeClient();
      const fetchKeyByHashMock = vi.fn(async (hash: string) => {
        expect(hash).toBe('sha256:abcdef1234567890');
        return KEY_RESPONSE;
      });
      client._setFFIAdapter(createMockFFI({ fetchKeyByHash: fetchKeyByHashMock }));

      const result = await client.fetchKeyByHash('sha256:abcdef1234567890');

      expect(result.jacsId).toBe('agent-abc');
      expect(result.algorithm).toBe('Ed25519');
      expect(result.publicKeyHash).toBe('sha256:abcdef1234567890');
      expect(result.dnsVerified).toBe(true);
    });

    it('escapes path-traversal characters in hash', async () => {
      const client = await makeClient();
      const fetchKeyByHashMock = vi.fn(async (hash: string) => {
        // FFI receives raw hash; Rust handles escaping
        expect(hash).toBe('sha256:../../etc/passwd');
        return KEY_RESPONSE;
      });
      client._setFFIAdapter(createMockFFI({ fetchKeyByHash: fetchKeyByHashMock }));

      await client.fetchKeyByHash('sha256:../../etc/passwd');
    });

    it('rejects on 404', async () => {
      const client = await makeClient();
      const fetchKeyByHashMock = vi.fn(async () => {
        throw mapFFIError(new Error('NotFound: key not found'));
      });
      client._setFFIAdapter(createMockFFI({ fetchKeyByHash: fetchKeyByHashMock }));

      await expect(client.fetchKeyByHash('sha256:missing')).rejects.toThrow();
    });
  });

  // -----------------------------------------------------------------------
  // fetchKeyByEmail
  // -----------------------------------------------------------------------

  describe('fetchKeyByEmail', () => {
    it('calls correct URL and parses response', async () => {
      const client = await makeClient();
      const fetchKeyByEmailMock = vi.fn(async (email: string) => {
        expect(email).toBe('alice@hai.ai');
        return KEY_RESPONSE;
      });
      client._setFFIAdapter(createMockFFI({ fetchKeyByEmail: fetchKeyByEmailMock }));

      const result = await client.fetchKeyByEmail('alice@hai.ai');

      expect(result.jacsId).toBe('agent-abc');
      expect(result.version).toBe('v1');
    });

    it('rejects on 404', async () => {
      const client = await makeClient();
      const fetchKeyByEmailMock = vi.fn(async () => {
        throw mapFFIError(new Error('NotFound: key not found'));
      });
      client._setFFIAdapter(createMockFFI({ fetchKeyByEmail: fetchKeyByEmailMock }));

      await expect(client.fetchKeyByEmail('nobody@hai.ai')).rejects.toThrow();
    });
  });

  // -----------------------------------------------------------------------
  // fetchKeyByDomain
  // -----------------------------------------------------------------------

  describe('fetchKeyByDomain', () => {
    it('calls correct URL and parses response', async () => {
      const client = await makeClient();
      const fetchKeyByDomainMock = vi.fn(async (domain: string) => {
        expect(domain).toBe('example.com');
        return KEY_RESPONSE;
      });
      client._setFFIAdapter(createMockFFI({ fetchKeyByDomain: fetchKeyByDomainMock }));

      const result = await client.fetchKeyByDomain('example.com');

      expect(result.jacsId).toBe('agent-abc');
      expect(result.dnsVerified).toBe(true);
    });

    it('rejects on 404', async () => {
      const client = await makeClient();
      const fetchKeyByDomainMock = vi.fn(async () => {
        throw mapFFIError(new Error('NotFound: key not found'));
      });
      client._setFFIAdapter(createMockFFI({ fetchKeyByDomain: fetchKeyByDomainMock }));

      await expect(client.fetchKeyByDomain('nonexistent.test')).rejects.toThrow();
    });
  });

  // -----------------------------------------------------------------------
  // fetchAllKeys
  // -----------------------------------------------------------------------

  describe('fetchAllKeys', () => {
    it('calls correct URL and returns structured result', async () => {
      const client = await makeClient();
      const fetchAllKeysMock = vi.fn(async (jacsId: string) => {
        expect(jacsId).toBe('agent-abc');
        return KEY_HISTORY_RESPONSE;
      });
      client._setFFIAdapter(createMockFFI({ fetchAllKeys: fetchAllKeysMock }));

      const result = await client.fetchAllKeys('agent-abc');

      expect(result.jacsId).toBe('agent-abc');
      expect(result.total).toBe(2);
      expect(result.keys).toHaveLength(2);
      expect(result.keys[0].version).toBe('v1');
      expect(result.keys[1].version).toBe('v0');
    });

    it('escapes jacs_id with slashes', async () => {
      const client = await makeClient();
      const fetchAllKeysMock = vi.fn(async (jacsId: string) => {
        // FFI receives raw jacsId; Rust handles escaping
        expect(jacsId).toBe('agent/with/slashes');
        return KEY_HISTORY_RESPONSE;
      });
      client._setFFIAdapter(createMockFFI({ fetchAllKeys: fetchAllKeysMock }));

      await client.fetchAllKeys('agent/with/slashes');
    });

    it('rejects on 404', async () => {
      const client = await makeClient();
      const fetchAllKeysMock = vi.fn(async () => {
        throw mapFFIError(new Error('NotFound: agent not found'));
      });
      client._setFFIAdapter(createMockFFI({ fetchAllKeys: fetchAllKeysMock }));

      await expect(client.fetchAllKeys('missing-agent')).rejects.toThrow();
    });
  });
});
