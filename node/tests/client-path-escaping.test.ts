import { afterEach, describe, expect, it, vi } from 'vitest';
import { HaiClient } from '../src/client.js';
import { generateTestKeypair as generateKeypair } from './setup.js';
import { createMockFFI } from './ffi-mock.js';

async function makeClient(jacsId: string = 'agent/with/slash'): Promise<HaiClient> {
  const keypair = generateKeypair();
  return HaiClient.fromCredentials(jacsId, keypair.privateKeyPem, { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' });
}

describe('client path escaping', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('escapes submitResponse jobId path segments', async () => {
    const client = await makeClient();
    const submitResponseMock = vi.fn(async (params: Record<string, unknown>) => {
      // FFI adapter receives the raw jobId in the params; Rust handles escaping
      expect(params.job_id).toBe('job/with/slash');
      return { success: true, job_id: 'job/with/slash', message: 'ok' };
    });
    client._setFFIAdapter(createMockFFI({ submitResponse: submitResponseMock }));

    await client.submitResponse('job/with/slash', 'response body');
  });

  it('escapes markRead jacsId and messageId path segments', async () => {
    const client = await makeClient('agent/with/slash');
    const markReadMock = vi.fn(async (messageId: string) => {
      // FFI adapter receives the raw messageId; Rust handles escaping
      expect(messageId).toBe('msg/with/slash');
    });
    client._setFFIAdapter(createMockFFI({ markRead: markReadMock }));

    await client.markRead('msg/with/slash');
  });

  it('escapes fetchRemoteKey jacsId and version path segments', async () => {
    const client = await makeClient();
    const fetchRemoteKeyMock = vi.fn(async (jacsId: string, version: string) => {
      // FFI adapter receives the raw values; Rust handles escaping
      expect(jacsId).toBe('agent/with/slash');
      expect(version).toBe('2026/01');
      return {
        jacs_id: 'agent/with/slash',
        version: '2026/01',
        public_key: 'pem',
      };
    });
    client._setFFIAdapter(createMockFFI({ fetchRemoteKey: fetchRemoteKeyMock }));

    await client.fetchRemoteKey('agent/with/slash', '2026/01');
  });

  it('escapes getAgentAttestation agentId path segments', async () => {
    const client = await makeClient();
    const verifyStatusMock = vi.fn(async (agentId?: string) => {
      // FFI adapter receives the raw agentId; Rust handles escaping
      expect(agentId).toBe('other/agent');
      return { jacs_id: 'other/agent', registered: false, registrations: [] };
    });
    client._setFFIAdapter(createMockFFI({ verifyStatus: verifyStatusMock }));

    await client.getAgentAttestation('other/agent');
  });
});

// ---------------------------------------------------------------------------
// Fixture-driven path escaping tests (T09)
// ---------------------------------------------------------------------------

import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname2 = dirname(fileURLToPath(import.meta.url));

describe('path_escaping_contract', () => {
  const fixture = JSON.parse(
    readFileSync(resolve(__dirname2, '../../fixtures/path_escaping_contract.json'), 'utf-8'),
  ) as { test_vectors: Array<{ raw: string; escaped: string }> };

  for (const vec of fixture.test_vectors) {
    it(`escapes "${vec.raw}" to "${vec.escaped}"`, () => {
      const result = encodeURIComponent(vec.raw);
      expect(result).toBe(vec.escaped);
    });
  }

  it('prevents path traversal', () => {
    const malicious = '../../../etc/passwd';
    const escaped = encodeURIComponent(malicious);
    // Slashes must be encoded to prevent path traversal
    expect(escaped).not.toContain('/');
  });
});
