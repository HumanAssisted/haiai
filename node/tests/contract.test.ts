import { afterEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { HaiClient } from '../src/client.js';
import { generateKeypair } from '../src/crypt.js';

interface EndpointContract {
  method: string;
  path: string;
  auth_required: boolean;
}

interface ContractFixture {
  base_url: string;
  hello: EndpointContract;
  check_username: EndpointContract;
  submit_response: EndpointContract;
}

function loadContractFixture(): ContractFixture {
  const here = dirname(fileURLToPath(import.meta.url));
  const fixturePath = resolve(here, '../../fixtures/contract_endpoints.json');
  return JSON.parse(readFileSync(fixturePath, 'utf-8')) as ContractFixture;
}

function makeClient(baseUrl: string): HaiClient {
  const keypair = generateKeypair();
  return HaiClient.fromCredentials('test-agent-001', keypair.privateKeyPem, { url: baseUrl });
}

describe('mock API contract (node)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('hello uses the shared method/path/auth contract', async () => {
    const contract = loadContractFixture();
    const client = makeClient(contract.base_url);

    const fetchMock = vi.fn(async (url: string | URL, init?: RequestInit) => {
      expect(String(url)).toBe(`${contract.base_url}${contract.hello.path}`);
      expect(init?.method).toBe(contract.hello.method);
      const headers = (init?.headers ?? {}) as Record<string, string>;
      if (contract.hello.auth_required) {
        expect(headers.Authorization).toMatch(/^JACS /);
      } else {
        expect(headers.Authorization).toBeUndefined();
      }
      return new Response(JSON.stringify({
        timestamp: '2026-01-01T00:00:00Z',
        client_ip: '127.0.0.1',
        hai_public_key_fingerprint: 'fp',
        message: 'ok',
        hello_id: 'h1',
      }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });
    });
    vi.stubGlobal('fetch', fetchMock);

    await client.hello();
  });

  it('checkUsername uses the shared method/path/auth contract', async () => {
    const contract = loadContractFixture();
    const client = makeClient(contract.base_url);

    const fetchMock = vi.fn(async (url: string | URL, init?: RequestInit) => {
      const parsed = new URL(String(url));
      expect(parsed.origin + parsed.pathname).toBe(`${contract.base_url}${contract.check_username.path}`);
      expect(parsed.searchParams.get('username')).toBe('alice');
      expect(init?.method).toBe(contract.check_username.method);
      const headers = (init?.headers ?? {}) as Record<string, string>;
      if (contract.check_username.auth_required) {
        expect(headers.Authorization).toMatch(/^JACS /);
      } else {
        expect(headers.Authorization).toBeUndefined();
      }

      return new Response(JSON.stringify({
        available: true,
        username: 'alice',
      }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });
    });
    vi.stubGlobal('fetch', fetchMock);

    await client.checkUsername('alice');
  });

  it('submitResponse uses the shared method/path/auth contract', async () => {
    const contract = loadContractFixture();
    const client = makeClient(contract.base_url);
    const jobId = 'job-123';
    const expectedPath = contract.submit_response.path.replace('{job_id}', jobId);

    const fetchMock = vi.fn(async (url: string | URL, init?: RequestInit) => {
      expect(String(url)).toBe(`${contract.base_url}${expectedPath}`);
      expect(init?.method).toBe(contract.submit_response.method);
      const headers = (init?.headers ?? {}) as Record<string, string>;
      if (contract.submit_response.auth_required) {
        expect(headers.Authorization).toMatch(/^JACS /);
      } else {
        expect(headers.Authorization).toBeUndefined();
      }
      return new Response(JSON.stringify({
        success: true,
        job_id: jobId,
        message: 'ok',
      }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });
    });
    vi.stubGlobal('fetch', fetchMock);

    await client.submitResponse(jobId, 'response body');
  });
});
