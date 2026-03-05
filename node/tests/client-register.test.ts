import { afterEach, describe, expect, it, vi } from 'vitest';
import { HaiClient } from '../src/client.js';
import { generateTestKeypair as generateKeypair } from './setup.js';

describe('client register bootstrap', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('registerNewAgent works without loading jacs.config.json', async () => {
    const bootstrap = generateKeypair();
    const client = await HaiClient.fromCredentials(
      'bootstrap-agent',
      bootstrap.privateKeyPem,
      { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' },
    );

    const fetchMock = vi.fn(async (_url: string, init?: RequestInit) => {
      const headers = (init?.headers ?? {}) as Record<string, string>;
      expect(headers.Authorization).toBeUndefined();
      expect(headers['Content-Type']).toBe('application/json');

      const payload = JSON.parse(String(init?.body ?? '{}')) as Record<string, unknown>;
      expect(payload.owner_email).toBe('owner@hai.ai');
      expect(payload.domain).toBe('agent.example');
      expect(typeof payload.agent_json).toBe('string');

      return new Response(JSON.stringify({
        agent_id: 'agent-123',
        registration_id: 'reg-1',
        registered_at: '2026-01-01T00:00:00Z',
      }), {
        status: 201,
        headers: { 'Content-Type': 'application/json' },
      });
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await client.registerNewAgent('agent-name', {
      ownerEmail: 'owner@hai.ai',
      domain: 'agent.example',
      description: 'Agent description',
      quiet: true,
    });

    expect(fetchMock).toHaveBeenCalledOnce();
    expect(result.agentId).toBe('agent-123');
    expect(result.jacsId).toBe('agent-name');
    expect(result.registrationId).toBe('reg-1');
  });

  it('exportKeys derives the public key from private key material', async () => {
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials('agent-1', keypair.privateKeyPem, { privateKeyPassphrase: 'keygen-password' });
    const exported = client.exportKeys();
    expect(exported.publicKeyPem).toContain('-----BEGIN PUBLIC KEY-----');
    expect(exported.privateKeyPem).toContain('-----BEGIN PRIVATE KEY-----');
  });
});
