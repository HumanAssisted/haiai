import { afterEach, describe, expect, it, vi } from 'vitest';
import { HaiClient } from '../src/client.js';
import { generateKeypair } from '../src/crypt.js';

function makeClient(jacsId: string = 'agent/with/slash'): HaiClient {
  const keypair = generateKeypair();
  return HaiClient.fromCredentials(jacsId, keypair.privateKeyPem, { url: 'https://hai.example' });
}

describe('client additional API methods', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('escapes updateUsername agentId and uses PUT', async () => {
    const client = makeClient();
    const fetchMock = vi.fn(async (url: string | URL, init?: RequestInit) => {
      expect(String(url)).toBe('https://hai.example/api/v1/agents/agent%2F..%2Fescape/username');
      expect(init?.method).toBe('PUT');
      expect(init?.body).toBe(JSON.stringify({ username: 'new-name' }));
      return new Response(
        JSON.stringify({
          username: 'new-name',
          email: 'new-name@hai.ai',
          previous_username: 'old-name',
        }),
        {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        },
      );
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await client.updateUsername('agent/../escape', 'new-name');
    expect(result.username).toBe('new-name');
    expect(result.previousUsername).toBe('old-name');
  });

  it('escapes deleteUsername agentId and uses DELETE', async () => {
    const client = makeClient();
    const fetchMock = vi.fn(async (url: string | URL, init?: RequestInit) => {
      expect(String(url)).toBe('https://hai.example/api/v1/agents/agent%2F..%2Fescape/username');
      expect(init?.method).toBe('DELETE');
      return new Response(
        JSON.stringify({
          released_username: 'old-name',
          cooldown_until: '2026-03-01T00:00:00Z',
          message: 'released',
        }),
        {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        },
      );
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await client.deleteUsername('agent/../escape');
    expect(result.releasedUsername).toBe('old-name');
    expect(result.cooldownUntil).toBe('2026-03-01T00:00:00Z');
  });

  it('verifyDocument POSTs to public /api/jacs/verify without auth header', async () => {
    const client = makeClient();
    const fetchMock = vi.fn(async (url: string | URL, init?: RequestInit) => {
      expect(String(url)).toBe('https://hai.example/api/jacs/verify');
      expect(init?.method).toBe('POST');
      expect(init?.body).toBe(JSON.stringify({ document: '{"jacsId":"a"}' }));
      const headers = new Headers(init?.headers as HeadersInit | undefined);
      expect(headers.has('Authorization')).toBe(false);
      return new Response(
        JSON.stringify({
          valid: true,
          verified_at: '2026-01-01T00:00:00Z',
          document_type: 'JacsDocument',
          issuer_verified: true,
          signature_verified: true,
          signer_id: 'agent-1',
          signed_at: '2026-01-01T00:00:00Z',
        }),
        {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        },
      );
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await client.verifyDocument({ jacsId: 'a' });
    expect(result.valid).toBe(true);
    expect(result.documentType).toBe('JacsDocument');
    expect(result.signerId).toBe('agent-1');
  });

  it('getVerification GETs public advanced verification endpoint without auth header', async () => {
    const client = makeClient();
    const fetchMock = vi.fn(async (url: string | URL, init?: RequestInit) => {
      expect(String(url)).toBe('https://hai.example/api/v1/agents/agent%2F..%2Fescape/verification');
      expect(init?.method).toBe('GET');
      const headers = new Headers(init?.headers as HeadersInit | undefined);
      expect(headers.has('Authorization')).toBe(false);
      return new Response(
        JSON.stringify({
          agent_id: 'agent/../escape',
          verification: {
            jacs_valid: true,
            dns_valid: true,
            hai_registered: false,
            badge: 'domain',
          },
          hai_signatures: ['ed25519:abc...'],
          verified_at: '2026-01-02T00:00:00Z',
          errors: [],
        }),
        {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        },
      );
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await client.getVerification('agent/../escape');
    expect(result.agentId).toBe('agent/../escape');
    expect(result.verification.badge).toBe('domain');
    expect(result.verification.jacsValid).toBe(true);
    expect(result.verification.dnsValid).toBe(true);
    expect(result.verification.haiRegistered).toBe(false);
    expect(result.haiSignatures).toEqual(['ed25519:abc...']);
  });

  it('verifyAgentDocumentOnHai POSTs public /api/v1/agents/verify without auth header', async () => {
    const client = makeClient();
    const fetchMock = vi.fn(async (url: string | URL, init?: RequestInit) => {
      expect(String(url)).toBe('https://hai.example/api/v1/agents/verify');
      expect(init?.method).toBe('POST');
      expect(init?.body).toBe(
        JSON.stringify({
          agent_json: '{"jacsId":"agent-1","jacsAgentDomain":"example.com"}',
          domain: 'override.example.com',
        }),
      );
      const headers = new Headers(init?.headers as HeadersInit | undefined);
      expect(headers.has('Authorization')).toBe(false);
      return new Response(
        JSON.stringify({
          agent_id: 'agent-1',
          verification: {
            jacs_valid: true,
            dns_valid: true,
            hai_registered: true,
            badge: 'attested',
          },
          hai_signatures: ['ed25519:def...'],
          verified_at: '2026-01-02T00:00:00Z',
          errors: [],
        }),
        {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        },
      );
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await client.verifyAgentDocumentOnHai(
      { jacsId: 'agent-1', jacsAgentDomain: 'example.com' },
      { domain: 'override.example.com' },
    );
    expect(result.agentId).toBe('agent-1');
    expect(result.verification.badge).toBe('attested');
    expect(result.verification.haiRegistered).toBe(true);
  });
});
