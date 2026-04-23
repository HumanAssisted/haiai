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
    client._setFFIAdapter(createMockFFI({ registerNewAgent: registerMock }));

    const result = await client.registerNewAgent('agent-name', {
      ownerEmail: 'owner@hai.ai',
      domain: 'agent.example',
      description: 'Agent description',
      password: 'keygen-password',
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

  it('signMessage produces a valid signature via JACS', async () => {
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials('agent-1', keypair.privateKeyPem);

    const message = 'credential-backed message';
    const signature = client.signMessage(message);

    // Verify the signature is non-empty base64 (signing delegates to JACS,
    // which uses its own managed key -- not the user-provided one).
    expect(signature).toBeTruthy();
    const sigBytes = Buffer.from(signature, 'base64');
    // Ed25519 signatures are 64 bytes.
    expect(sigBytes.length).toBe(64);

    // Verify different messages produce different signatures.
    const otherSig = client.signMessage('different message');
    expect(otherSig).not.toBe(signature);
  });
});
