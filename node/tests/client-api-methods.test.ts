import { afterEach, describe, expect, it, vi } from 'vitest';
import { HaiClient } from '../src/client.js';
import { generateTestKeypair as generateKeypair } from './setup.js';
import { createMockFFI } from './ffi-mock.js';

async function makeClient(jacsId: string = 'agent/with/slash'): Promise<HaiClient> {
  const keypair = generateKeypair();
  return HaiClient.fromCredentials(jacsId, keypair.privateKeyPem, { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' });
}

describe('client additional API methods', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('escapes updateUsername agentId and uses PUT', async () => {
    const client = await makeClient();
    const updateUsernameMock = vi.fn(async (agentId: string, username: string) => {
      expect(agentId).toBe('agent/../escape');
      expect(username).toBe('new-name');
      return {
        username: 'new-name',
        email: 'new-name@hai.ai',
        previous_username: 'old-name',
      };
    });
    client._setFFIAdapter(createMockFFI({ updateUsername: updateUsernameMock }));

    const result = await client.updateUsername('agent/../escape', 'new-name');
    expect(result.username).toBe('new-name');
    expect(result.previousUsername).toBe('old-name');
  });

  it('escapes deleteUsername agentId and uses DELETE', async () => {
    const client = await makeClient();
    const deleteUsernameMock = vi.fn(async (agentId: string) => {
      expect(agentId).toBe('agent/../escape');
      return {
        released_username: 'old-name',
        cooldown_until: '2026-03-01T00:00:00Z',
        message: 'released',
      };
    });
    client._setFFIAdapter(createMockFFI({ deleteUsername: deleteUsernameMock }));

    const result = await client.deleteUsername('agent/../escape');
    expect(result.releasedUsername).toBe('old-name');
    expect(result.cooldownUntil).toBe('2026-03-01T00:00:00Z');
  });

  it('verifyDocument POSTs to public /api/jacs/verify without auth header', async () => {
    const client = await makeClient();
    const verifyDocumentMock = vi.fn(async (document: string) => {
      expect(document).toBe('{"jacsId":"a"}');
      return {
        valid: true,
        verified_at: '2026-01-01T00:00:00Z',
        document_type: 'JacsDocument',
        issuer_verified: true,
        signature_verified: true,
        signer_id: 'agent-1',
        signed_at: '2026-01-01T00:00:00Z',
      };
    });
    client._setFFIAdapter(createMockFFI({ verifyDocument: verifyDocumentMock }));

    const result = await client.verifyDocument({ jacsId: 'a' });
    expect(result.valid).toBe(true);
    expect(result.documentType).toBe('JacsDocument');
    expect(result.signerId).toBe('agent-1');
  });

  it('getVerification GETs public advanced verification endpoint without auth header', async () => {
    const client = await makeClient();
    const getVerificationMock = vi.fn(async (agentId: string) => {
      expect(agentId).toBe('agent/../escape');
      return {
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
      };
    });
    client._setFFIAdapter(createMockFFI({ getVerification: getVerificationMock }));

    const result = await client.getVerification('agent/../escape');
    expect(result.agentId).toBe('agent/../escape');
    expect(result.verification.badge).toBe('domain');
    expect(result.verification.jacsValid).toBe(true);
    expect(result.verification.dnsValid).toBe(true);
    expect(result.verification.haiRegistered).toBe(false);
    expect(result.haiSignatures).toEqual(['ed25519:abc...']);
  });

  it('verifyAgentDocumentOnHai POSTs public /api/v1/agents/verify without auth header', async () => {
    const client = await makeClient();
    const verifyAgentDocumentMock = vi.fn(async (requestJson: string) => {
      const parsed = JSON.parse(requestJson);
      expect(parsed.agent_json).toBe('{"jacsId":"agent-1","jacsAgentDomain":"example.com"}');
      expect(parsed.domain).toBe('override.example.com');
      return {
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
      };
    });
    client._setFFIAdapter(createMockFFI({ verifyAgentDocument: verifyAgentDocumentMock }));

    const result = await client.verifyAgentDocumentOnHai(
      { jacsId: 'agent-1', jacsAgentDomain: 'example.com' },
      { domain: 'override.example.com' },
    );
    expect(result.agentId).toBe('agent-1');
    expect(result.verification.badge).toBe('attested');
    expect(result.verification.haiRegistered).toBe(true);
  });
});
