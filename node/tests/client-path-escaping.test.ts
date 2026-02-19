import { afterEach, describe, expect, it, vi } from 'vitest';
import { HaiClient } from '../src/client.js';
import { generateKeypair } from '../src/crypt.js';

function makeClient(jacsId: string = 'agent/with/slash'): HaiClient {
  const keypair = generateKeypair();
  return HaiClient.fromCredentials(jacsId, keypair.privateKeyPem, { url: 'https://hai.example' });
}

function stubJsonFetch(expectedUrl: string, payload: Record<string, unknown> = {}): void {
  const fetchMock = vi.fn(async (url: string | URL) => {
    expect(String(url)).toBe(expectedUrl);
    return new Response(JSON.stringify(payload), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    });
  });
  vi.stubGlobal('fetch', fetchMock);
}

describe('client path escaping', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('escapes claimUsername agentId path segments', async () => {
    const client = makeClient();
    stubJsonFetch(
      'https://hai.example/api/v1/agents/agent%2F..%2Fescape/username',
      { username: 'agent', email: 'agent@hai.ai', agent_id: 'agent/../escape' },
    );

    await client.claimUsername('agent/../escape', 'agent');
  });

  it('escapes submitResponse jobId path segments', async () => {
    const client = makeClient();
    stubJsonFetch(
      'https://hai.example/api/v1/agents/jobs/job%2Fwith%2Fslash/response',
      { success: true, job_id: 'job/with/slash', message: 'ok' },
    );

    await client.submitResponse('job/with/slash', 'response body');
  });

  it('escapes markRead jacsId and messageId path segments', async () => {
    const client = makeClient('agent/with/slash');
    stubJsonFetch(
      'https://hai.example/api/agents/agent%2Fwith%2Fslash/email/messages/msg%2Fwith%2Fslash/read',
      {},
    );

    await client.markRead('msg/with/slash');
  });

  it('escapes fetchRemoteKey jacsId and version path segments', async () => {
    const client = makeClient();
    stubJsonFetch(
      'https://hai.example/jacs/v1/agents/agent%2Fwith%2Fslash/keys/2026%2F01',
      {
        jacs_id: 'agent/with/slash',
        version: '2026/01',
        public_key: 'pem',
      },
    );

    await client.fetchRemoteKey('agent/with/slash', '2026/01');
  });

  it('escapes getAgentAttestation agentId path segments', async () => {
    const client = makeClient();
    stubJsonFetch(
      'https://hai.example/api/v1/agents/other%2Fagent/verify',
      { jacs_id: 'other/agent', registered: false, registrations: [] },
    );

    await client.getAgentAttestation('other/agent');
  });
});
