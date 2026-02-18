import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { HaiClient } from '../src/client.js';
import { generateKeypair } from '../src/crypt.js';
import { HaiError, AuthenticationError, HaiConnectionError } from '../src/errors.js';
import { TEST_KEYPAIR, TEST_JACS_ID, TEST_BASE_URL, createMockFetch, createSseBody } from './setup.js';

describe('HaiClient', () => {
  let originalFetch: typeof globalThis.fetch;

  beforeEach(() => {
    originalFetch = globalThis.fetch;
  });

  afterEach(() => {
    globalThis.fetch = originalFetch;
  });

  function createClient(url?: string): HaiClient {
    return HaiClient.fromCredentials(TEST_JACS_ID, TEST_KEYPAIR.privateKeyPem, {
      url: url ?? TEST_BASE_URL,
      timeout: 5000,
      maxRetries: 1,
    });
  }

  describe('fromCredentials', () => {
    it('creates a client with JACS ID and key', () => {
      const client = createClient();
      expect(client.jacsId).toBe(TEST_JACS_ID);
      expect(client.agentName).toBe(TEST_JACS_ID);
      expect(client.isConnected).toBe(false);
    });

    it('uses custom base URL', () => {
      const client = HaiClient.fromCredentials(TEST_JACS_ID, TEST_KEYPAIR.privateKeyPem, {
        url: 'https://custom.hai.ai/',
      });
      expect(client.jacsId).toBe(TEST_JACS_ID);
    });

    it('defaults to https://hai.ai when no url provided', () => {
      const client = HaiClient.fromCredentials(TEST_JACS_ID, TEST_KEYPAIR.privateKeyPem);
      expect(client.jacsId).toBe(TEST_JACS_ID);
    });
  });

  describe('buildAuthHeader', () => {
    it('returns JACS formatted header', () => {
      const client = createClient();
      const header = client.buildAuthHeader();
      expect(header).toMatch(/^JACS test-agent-001:\d+:/);
    });

    it('contains valid base64 signature', () => {
      const client = createClient();
      const header = client.buildAuthHeader();
      const parts = header.replace('JACS ', '').split(':');
      expect(parts).toHaveLength(3);
      expect(parts[0]).toBe(TEST_JACS_ID);
      // parts[1] is timestamp (numeric)
      expect(Number(parts[1])).toBeGreaterThan(0);
      // parts[2] is base64 signature
      expect(Buffer.from(parts[2], 'base64').length).toBe(64);
    });
  });

  describe('signMessage', () => {
    it('signs a message', () => {
      const client = createClient();
      const sig = client.signMessage('hello');
      expect(typeof sig).toBe('string');
      expect(Buffer.from(sig, 'base64').length).toBe(64);
    });
  });

  describe('hello', () => {
    it('sends JACS auth and returns HelloWorldResult', async () => {
      const mock = createMockFetch({
        status: 200,
        body: {
          timestamp: '2024-01-01T00:00:00Z',
          client_ip: '1.2.3.4',
          hai_public_key_fingerprint: 'fp123',
          message: 'Hello, agent!',
        },
      });
      globalThis.fetch = mock.fetch;

      const client = createClient();
      const result = await client.hello();

      expect(result.success).toBe(true);
      expect(result.timestamp).toBe('2024-01-01T00:00:00Z');
      expect(result.clientIp).toBe('1.2.3.4');
      expect(result.message).toBe('Hello, agent!');
      expect(result.haiPublicKeyFingerprint).toBe('fp123');

      // Verify auth header was sent
      const authHeader = (mock.calls[0].init.headers as Record<string, string>)['Authorization'];
      expect(authHeader).toMatch(/^JACS test-agent-001:\d+:/);
    });

    it('passes include_test flag', async () => {
      const mock = createMockFetch({ status: 200, body: { message: 'ok' } });
      globalThis.fetch = mock.fetch;

      const client = createClient();
      await client.hello(true);

      const body = JSON.parse(mock.calls[0].init.body as string);
      expect(body.include_test).toBe(true);
    });

    it('throws AuthenticationError on 401', async () => {
      const mock = createMockFetch({ status: 401, body: { error: 'bad sig' } });
      globalThis.fetch = mock.fetch;

      const client = createClient();
      await expect(client.hello()).rejects.toThrow(AuthenticationError);
    });

    it('throws HaiError on 429', async () => {
      const mock = createMockFetch({ status: 429, body: {} });
      globalThis.fetch = mock.fetch;

      const client = createClient();
      await expect(client.hello()).rejects.toThrow('Rate limited');
    });
  });

  describe('register', () => {
    it('sends registration with JACS auth', async () => {
      const mock = createMockFetch({
        status: 201,
        body: {
          agent_id: 'agent-xyz',
          hai_signature: 'sig-abc',
          registration_id: 'reg-1',
          registered_at: '2024-01-01T00:00:00Z',
        },
      });
      globalThis.fetch = mock.fetch;

      const client = createClient();
      const result = await client.register();

      expect(result.success).toBe(true);
      expect(result.agentId).toBe('agent-xyz');
      expect(result.registrationId).toBe('reg-1');

      // Verify no API key / Bearer auth
      const authHeader = (mock.calls[0].init.headers as Record<string, string>)['Authorization'];
      expect(authHeader).toMatch(/^JACS /);
      expect(authHeader).not.toContain('Bearer');
    });
  });

  describe('status', () => {
    it('returns agent status', async () => {
      const mock = createMockFetch({
        status: 200,
        body: {
          registered: true,
          agent_id: TEST_JACS_ID,
          registration_id: 'reg-1',
          registered_at: '2024-01-01T00:00:00Z',
          hai_signatures: ['ed25519'],
          benchmark_count: 3,
        },
      });
      globalThis.fetch = mock.fetch;

      const client = createClient();
      const result = await client.status();

      expect(result.registered).toBe(true);
      expect(result.agentId).toBe(TEST_JACS_ID);
      expect(result.registrationId).toBe('reg-1');
      expect(result.haiSignatures).toEqual(['ed25519']);
      expect(result.benchmarkCount).toBe(3);
    });

    it('handles legacy active field as registered', async () => {
      const mock = createMockFetch({
        status: 200,
        body: { active: true, agent_id: TEST_JACS_ID },
      });
      globalThis.fetch = mock.fetch;

      const client = createClient();
      const result = await client.status();
      expect(result.registered).toBe(true);
    });

    it('defaults to empty values', async () => {
      const mock = createMockFetch({ status: 200, body: {} });
      globalThis.fetch = mock.fetch;

      const client = createClient();
      const result = await client.status();
      expect(result.registered).toBe(false);
      expect(result.registrationId).toBe('');
      expect(result.haiSignatures).toEqual([]);
      expect(result.benchmarkCount).toBe(0);
    });
  });

  describe('freeChaoticRun', () => {
    it('sends free chaotic benchmark request', async () => {
      const mock = createMockFetch({
        status: 200,
        body: {
          run_id: 'run-1',
          transcript: [
            { role: 'party_a', content: 'I need help', timestamp: 'now', annotations: [] },
          ],
          upsell_message: 'Try baseline!',
        },
      });
      globalThis.fetch = mock.fetch;

      const client = createClient();
      const result = await client.freeChaoticRun();

      expect(result.success).toBe(true);
      expect(result.runId).toBe('run-1');
      expect(result.transcript).toHaveLength(1);
      expect(result.transcript[0].role).toBe('party_a');
      expect(result.upsellMessage).toBe('Try baseline!');

      const body = JSON.parse(mock.calls[0].init.body as string);
      expect(body.tier).toBe('free_chaotic');
    });
  });

  describe('submitResponse', () => {
    it('sends signed response for a job', async () => {
      const mock = createMockFetch({
        status: 200,
        body: { success: true, job_id: 'job-1', message: 'Accepted' },
      });
      globalThis.fetch = mock.fetch;

      const client = createClient();
      const result = await client.submitResponse('job-1', 'My mediation response', {
        processingTimeMs: 1500,
        metadata: { model: 'gpt-4' },
      });

      expect(result.success).toBe(true);
      expect(result.jobId).toBe('job-1');
      expect(result.message).toBe('Accepted');

      // Should use JACS auth not Bearer
      const authHeader = (mock.calls[0].init.headers as Record<string, string>)['Authorization'];
      expect(authHeader).toMatch(/^JACS /);

      // Body should contain signed_document
      const body = JSON.parse(mock.calls[0].init.body as string);
      expect(body.signed_document).toBeDefined();
      expect(body.agent_jacs_id).toBe(TEST_JACS_ID);
    });
  });

  describe('verifyHaiMessage', () => {
    it('returns false for empty inputs', () => {
      const client = createClient();
      expect(client.verifyHaiMessage('', 'sig', 'key')).toBe(false);
      expect(client.verifyHaiMessage('msg', '', 'key')).toBe(false);
      expect(client.verifyHaiMessage('msg', 'sig', '')).toBe(false);
    });
  });

  describe('exportKeys', () => {
    it('returns matching public and private keys', () => {
      const client = createClient();
      const keys = client.exportKeys();
      expect(keys.privateKeyPem).toBe(TEST_KEYPAIR.privateKeyPem);
      expect(keys.publicKeyPem).toContain('-----BEGIN PUBLIC KEY-----');
    });
  });

  describe('disconnect', () => {
    it('does not throw when not connected', () => {
      const client = createClient();
      expect(() => client.disconnect()).not.toThrow();
      expect(client.isConnected).toBe(false);
    });
  });

  describe('getAgentAttestation', () => {
    it('fetches attestation for another agent', async () => {
      const mock = createMockFetch({
        status: 200,
        body: { agent_id: 'other-agent', registered: true, hai_signatures: ['ed25519'] },
      });
      globalThis.fetch = mock.fetch;

      const client = createClient();
      const result = await client.getAgentAttestation('other-agent');

      expect(result.agent_id).toBe('other-agent');
      expect(mock.calls[0].url).toContain('/api/v1/agents/other-agent/status');
    });

    it('uses JACS auth', async () => {
      const mock = createMockFetch({ status: 200, body: {} });
      globalThis.fetch = mock.fetch;

      const client = createClient();
      await client.getAgentAttestation('other');

      const authHeader = (mock.calls[0].init.headers as Record<string, string>)['Authorization'];
      expect(authHeader).toMatch(/^JACS /);
    });
  });

  describe('signBenchmarkResult', () => {
    it('returns a signed JACS document', () => {
      const client = createClient();
      const result = client.signBenchmarkResult({ score: 85, suite: 'mediation_basic' });

      expect(result.signed_document).toBeDefined();
      expect(typeof result.signed_document).toBe('string');
      expect(result.agent_jacs_id).toBe(TEST_JACS_ID);

      // Verify the signed document is valid JSON
      const doc = JSON.parse(result.signed_document);
      expect(doc.data).toEqual(JSON.parse(JSON.stringify({ score: 85, suite: 'mediation_basic' })));
      expect(doc.version).toBe('1.0.0');
      expect(doc.jacsSignature.agentID).toBe(TEST_JACS_ID);
      expect(doc.metadata.issuer).toBe(TEST_JACS_ID);
    });
  });

  describe('benchmark', () => {
    it('runs a legacy suite-based benchmark', async () => {
      const mock = createMockFetch({
        status: 200,
        body: { score: 72.5, suite: 'mediation_basic', passed: 8, total: 10 },
      });
      globalThis.fetch = mock.fetch;

      const client = createClient();
      const result = await client.benchmark('mediation_basic');

      expect(result.score).toBe(72.5);
      expect(result.suite).toBe('mediation_basic');

      const body = JSON.parse(mock.calls[0].init.body as string);
      expect(body.suite).toBe('mediation_basic');
    });

    it('defaults to mediation_basic suite', async () => {
      const mock = createMockFetch({ status: 200, body: {} });
      globalThis.fetch = mock.fetch;

      const client = createClient();
      await client.benchmark();

      const body = JSON.parse(mock.calls[0].init.body as string);
      expect(body.suite).toBe('mediation_basic');
    });
  });

  describe('error classes', () => {
    it('HaiError has name and statusCode', () => {
      const err = new HaiError('test', 500, { detail: 'info' });
      expect(err.name).toBe('HaiError');
      expect(err.statusCode).toBe(500);
      expect(err.responseData).toEqual({ detail: 'info' });
      expect(err.message).toBe('test');
      expect(err instanceof Error).toBe(true);
    });

    it('AuthenticationError extends HaiError', () => {
      const err = new AuthenticationError('bad auth', 401);
      expect(err.name).toBe('AuthenticationError');
      expect(err instanceof HaiError).toBe(true);
      expect(err instanceof Error).toBe(true);
    });

    it('HaiConnectionError extends HaiError', () => {
      const err = new HaiConnectionError('timeout');
      expect(err.name).toBe('HaiConnectionError');
      expect(err instanceof HaiError).toBe(true);
    });
  });
});
