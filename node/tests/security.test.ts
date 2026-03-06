import { afterEach, describe, expect, it, vi } from 'vitest';
import { HaiClient } from '../src/client.js';
import { generateTestKeypair as generateKeypair } from './setup.js';

async function makeClient(): Promise<HaiClient> {
  const kp = generateKeypair();
  const client = await HaiClient.fromCredentials('security-agent', kp.privateKeyPem, {
    url: 'https://hai.example',
    privateKeyPassphrase: 'keygen-password',
  });
  (client as any)._publicKeyPem = kp.publicKeyPem;
  return client;
}

describe('security behaviors (node)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('register does not send private key material and keeps bootstrap request unauthenticated', async () => {
    const client = await makeClient();

    const fetchMock = vi.fn(async (_url: string | URL, init?: RequestInit) => {
      const headers = new Headers(init?.headers);
      expect(headers.get('Authorization')).toBeNull();
      expect(headers.get('Content-Type')).toBe('application/json');

      const rawBody = String(init?.body ?? '');
      expect(rawBody).not.toContain('BEGIN PRIVATE KEY');

      const payload = JSON.parse(rawBody) as Record<string, string>;
      expect(typeof payload.agent_json).toBe('string');
      expect(payload.public_key).toBeTypeOf('string');

      return new Response(JSON.stringify({
        agent_id: 'agent-123',
        jacs_id: 'security-agent',
        registered_at: '2026-01-01T00:00:00Z',
      }), {
        status: 201,
        headers: { 'Content-Type': 'application/json' },
      });
    });
    vi.stubGlobal('fetch', fetchMock);

    await client.register({
      ownerEmail: 'owner@hai.ai',
      domain: 'agent.example',
      description: 'Security test agent',
    });
  });

  it('checkUsername remains unauthenticated public endpoint', async () => {
    const client = await makeClient();

    const fetchMock = vi.fn(async (_url: string | URL, init?: RequestInit) => {
      const headers = new Headers(init?.headers);
      expect(headers.get('Authorization')).toBeNull();

      return new Response(JSON.stringify({
        available: true,
        username: 'agent',
      }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await client.checkUsername('agent');
    expect(result.available).toBe(true);
  });

  it('registerNewAgent omits Authorization and sends base64 public key only', async () => {
    const client = await makeClient();

    const fetchMock = vi.fn(async (_url: string | URL, init?: RequestInit) => {
      const headers = new Headers(init?.headers);
      expect(headers.get('Authorization')).toBeNull();
      expect(headers.get('Content-Type')).toBe('application/json');

      const payload = JSON.parse(String(init?.body ?? '{}')) as Record<string, string>;
      const decodedPublicKey = Buffer.from(payload.public_key, 'base64').toString('utf-8');
      expect(decodedPublicKey).toContain('BEGIN PUBLIC KEY');
      expect(decodedPublicKey).not.toContain('BEGIN PRIVATE KEY');

      return new Response(JSON.stringify({
        agent_id: 'agent-999',
        jacs_id: 'security-agent',
        registration_id: 'reg-999',
        registered_at: '2026-01-01T00:00:00Z',
      }), {
        status: 201,
        headers: { 'Content-Type': 'application/json' },
      });
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await client.registerNewAgent('security-agent', {
      ownerEmail: 'owner@hai.ai',
      domain: 'agent.example',
      description: 'Security bootstrap',
      quiet: true,
    });

    expect(result.agentId).toBe('agent-999');
  });
});
