import { afterEach, describe, expect, it, vi } from 'vitest';
import { HaiClient } from '../src/client.js';
import { generateTestKeypair as generateKeypair } from './setup.js';
import { createMockFFI } from './ffi-mock.js';

async function makeClient(): Promise<HaiClient> {
  const kp = generateKeypair();
  return HaiClient.fromCredentials('security-agent', kp.privateKeyPem, {
    url: 'https://hai.example',
    privateKeyPassphrase: 'keygen-password',
  });
}

describe('security behaviors (node)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('register delegates to FFI without exposing private key material', async () => {
    const client = await makeClient();
    const registerMock = vi.fn(async (options: Record<string, unknown>) => {
      // Verify no private key in the options
      expect(JSON.stringify(options)).not.toContain('BEGIN PRIVATE KEY');
      return {
        agent_id: 'agent-123',
        jacs_id: 'security-agent',
        registered_at: '2026-01-01T00:00:00Z',
      };
    });
    client._setFFIAdapter(createMockFFI({ register: registerMock }));

    await client.register({
      ownerEmail: 'owner@hai.ai',
      domain: 'agent.example',
      description: 'Security test agent',
    });
  });

  it('registerNewAgent delegates to FFI', async () => {
    const client = await makeClient();
    const registerMock = vi.fn(async (options: Record<string, unknown>) => {
      // Verify no private key in the options
      expect(JSON.stringify(options)).not.toContain('BEGIN PRIVATE KEY');
      return {
        agent_id: 'agent-999',
        jacs_id: 'security-agent',
        registration_id: 'reg-999',
        registered_at: '2026-01-01T00:00:00Z',
      };
    });
    client._setFFIAdapter(createMockFFI({ registerNewAgent: registerMock }));

    const result = await client.registerNewAgent('security-agent', {
      ownerEmail: 'owner@hai.ai',
      domain: 'agent.example',
      description: 'Security bootstrap',
      password: 'keygen-password',
      quiet: true,
    });

    expect(result.agentId).toBe('agent-999');
  });
});
