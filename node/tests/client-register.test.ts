import { verify as cryptoVerify } from 'node:crypto';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { HaiClient } from '../src/client.js';
import { generateTestKeypair as generateKeypair } from './setup.js';
import { createMockFFI } from './ffi-mock.js';

describe('client register bootstrap', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('registerNewAgent works without loading jacs.config.json', async () => {
    const bootstrap = generateKeypair();
    const client = await HaiClient.fromCredentials(
      'bootstrap-agent',
      bootstrap.privateKeyPem,
      { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' },
    );

    const registerMock = vi.fn(async (options: Record<string, unknown>) => {
      expect(options.owner_email).toBe('owner@hai.ai');
      expect(options.domain).toBe('agent.example');
      return {
        agent_id: 'agent-123',
        jacs_id: 'agent-name',
        registration_id: 'reg-1',
        registered_at: '2026-01-01T00:00:00Z',
      };
    });
    client._setFFIAdapter(createMockFFI({ register: registerMock }));

    const result = await client.registerNewAgent('agent-name', {
      ownerEmail: 'owner@hai.ai',
      domain: 'agent.example',
      description: 'Agent description',
      quiet: true,
    });

    expect(registerMock).toHaveBeenCalledOnce();
    expect(result.agentId).toBe('agent-123');
    expect(result.jacsId).toBe('agent-name');
    expect(result.registrationId).toBe('reg-1');
  });

  it('exportKeys derives the public key from private key material', async () => {
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials('agent-1', keypair.privateKeyPem, { privateKeyPassphrase: 'keygen-password' });
    const exported = client.exportKeys();
    expect(exported.publicKeyPem).toContain('-----BEGIN PUBLIC KEY-----');
  });

  it('signMessage uses the supplied credential pair', async () => {
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials('agent-1', keypair.privateKeyPem);

    const message = 'credential-backed message';
    const signature = client.signMessage(message);
    const verified = cryptoVerify(
      null,
      Buffer.from(message, 'utf-8'),
      keypair.publicKeyPem,
      Buffer.from(signature, 'base64'),
    );

    expect(verified).toBe(true);
  });
});
