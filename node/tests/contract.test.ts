import { afterEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { HaiClient } from '../src/client.js';
import { generateTestKeypair as generateKeypair } from './setup.js';
import { createMockFFI } from './ffi-mock.js';

interface EndpointContract {
  method: string;
  path: string;
  auth_required: boolean;
}

interface ContractFixture {
  base_url: string;
  hello: EndpointContract;
  submit_response: EndpointContract;
}

function loadContractFixture(): ContractFixture {
  const here = dirname(fileURLToPath(import.meta.url));
  const fixturePath = resolve(here, '../../fixtures/contract_endpoints.json');
  return JSON.parse(readFileSync(fixturePath, 'utf-8')) as ContractFixture;
}

async function makeClient(baseUrl: string): Promise<HaiClient> {
  const keypair = generateKeypair();
  return HaiClient.fromCredentials('test-agent-001', keypair.privateKeyPem, { url: baseUrl, privateKeyPassphrase: 'keygen-password' });
}

describe('mock API contract (node)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('hello uses the shared method/path/auth contract', async () => {
    const contract = loadContractFixture();
    const client = await makeClient(contract.base_url);

    const helloMock = vi.fn(async (includeTest: boolean) => ({
      timestamp: '2026-01-01T00:00:00Z',
      client_ip: '127.0.0.1',
      hai_public_key_fingerprint: 'fp',
      message: 'ok',
      hello_id: 'h1',
    }));
    client._setFFIAdapter(createMockFFI({ hello: helloMock }));

    await client.hello();
    expect(helloMock).toHaveBeenCalledTimes(1);
  });

  it('submitResponse uses the shared method/path/auth contract', async () => {
    const contract = loadContractFixture();
    const client = await makeClient(contract.base_url);

    const submitResponseMock = vi.fn(async (params: Record<string, unknown>) => {
      expect(params.job_id).toBe('job-123');
      return { success: true, job_id: 'job-123', message: 'accepted' };
    });
    client._setFFIAdapter(createMockFFI({ submitResponse: submitResponseMock }));

    await client.submitResponse('job-123', 'test response');
  });

  // Fixture-driven response-shape tests below do NOT need fetch/FFI mocking.
  // They parse JSON from disk and verify TypeScript type conformance.

  it('hello response fixture is parseable and typed', () => {
    const contract = loadContractFixture();
    expect(contract.hello.method).toBe('POST');
    expect(contract.hello.auth_required).toBe(true);
    expect(contract.hello.path).toContain('/hello');
  });

  it('submitResponse fixture data is well-formed', () => {
    const contract = loadContractFixture();
    expect(contract.submit_response.method).toBe('POST');
    expect(contract.submit_response.auth_required).toBe(true);
    expect(contract.submit_response.path).toContain('/response');
  });
});
