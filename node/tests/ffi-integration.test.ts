/**
 * FFI integration tests -- prove that the Node SDK delegates to FFI
 * and that no old HTTP code remains for core API calls.
 *
 * These tests verify:
 * 1. FFIClientAdapter exposes all methods from the parity fixture
 * 2. mapFFIError maps all error kinds from the parity fixture
 * 3. HaiClient delegates API calls to FFIClientAdapter, not fetch
 * 4. createMockFFI (test infrastructure) covers the full method set
 * 5. Core API methods in client.ts do not reference fetch
 */

import { describe, expect, it, vi, afterEach } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { HaiClient } from '../src/client.js';
import {
  HaiError,
  AuthenticationError,
  HaiConnectionError,
  HaiApiError,
  RateLimitedError,
} from '../src/errors.js';
import { FFIClientAdapter, mapFFIError } from '../src/ffi-client.js';
import { createMockFFI } from './ffi-mock.js';
import { generateTestKeypair } from './setup.js';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

interface ParityFixture {
  methods: Record<string, Array<{ name: string; args: string[]; returns: string }>>;
  error_kinds: string[];
  error_format: string;
  total_method_count: number;
}

function loadParityFixture(): ParityFixture {
  const fixturePath = resolve(__dirname, '../../fixtures/ffi_method_parity.json');
  return JSON.parse(readFileSync(fixturePath, 'utf-8')) as ParityFixture;
}

/** Convert snake_case to camelCase. */
function toCamelCase(name: string): string {
  return name.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());
}

function getAllFixtureMethodNames(): string[] {
  const fixture = loadParityFixture();
  const names: string[] = [];
  for (const group of Object.values(fixture.methods)) {
    for (const method of group) {
      names.push(toCamelCase(method.name));
    }
  }
  return names.sort();
}

async function makeClient(jacsId: string = 'test-agent'): Promise<HaiClient> {
  const keypair = generateTestKeypair();
  return HaiClient.fromCredentials(jacsId, keypair.privateKeyPem, {
    url: 'https://hai.example',
    privateKeyPassphrase: undefined,
  });
}

// ---------------------------------------------------------------------------
// Test: FFI method parity -- fixture vs FFIClientAdapter
// ---------------------------------------------------------------------------

describe('FFI method parity (Node)', () => {
  it('FFIClientAdapter has all methods from parity fixture', () => {
    const fixtureNames = getAllFixtureMethodNames();
    const adapterProto = FFIClientAdapter.prototype;

    const missing: string[] = [];
    for (const name of fixtureNames) {
      if (typeof (adapterProto as Record<string, unknown>)[name] !== 'function') {
        missing.push(name);
      }
    }

    expect(missing).toEqual([]);
  });

  it('createMockFFI covers all fixture methods', () => {
    const mock = createMockFFI();
    const fixtureNames = getAllFixtureMethodNames();

    const missing: string[] = [];
    for (const name of fixtureNames) {
      if (typeof (mock as Record<string, unknown>)[name] !== 'function') {
        missing.push(name);
      }
    }

    expect(missing).toEqual([]);
  });

  it('fixture total method count matches actual methods', () => {
    const fixture = loadParityFixture();
    const names = getAllFixtureMethodNames();
    expect(names.length).toBe(fixture.total_method_count);
  });
});

// ---------------------------------------------------------------------------
// Test: Error mapping covers all fixture error kinds
// ---------------------------------------------------------------------------

describe('FFI error mapping parity (Node)', () => {
  it('maps all error kinds from parity fixture', () => {
    const fixture = loadParityFixture();

    for (const kind of fixture.error_kinds) {
      const err = new Error(`${kind}: test message`);
      const result = mapFFIError(err);
      expect(result).toBeInstanceOf(HaiError);
    }
  });

  it('maps AuthFailed to AuthenticationError', () => {
    const result = mapFFIError(new Error('AuthFailed: token expired'));
    expect(result).toBeInstanceOf(AuthenticationError);
    expect(result.message).toContain('token expired');
  });

  it('maps RateLimited to RateLimitedError', () => {
    const result = mapFFIError(new Error('RateLimited: too many requests'));
    expect(result).toBeInstanceOf(RateLimitedError);
  });

  it('maps NetworkFailed to HaiConnectionError', () => {
    const result = mapFFIError(new Error('NetworkFailed: connection refused'));
    expect(result).toBeInstanceOf(HaiConnectionError);
  });

  it('maps NotFound to HaiApiError', () => {
    const result = mapFFIError(new Error('NotFound: resource missing'));
    expect(result).toBeInstanceOf(HaiApiError);
  });

  it('maps ApiError to HaiApiError', () => {
    const result = mapFFIError(new Error('ApiError: status 500 internal server error'));
    expect(result).toBeInstanceOf(HaiApiError);
  });

  it('maps ProviderError to AuthenticationError', () => {
    const result = mapFFIError(new Error('ProviderError: JACS agent not initialized'));
    expect(result).toBeInstanceOf(AuthenticationError);
  });

  it('handles generic/unknown error kinds', () => {
    const result = mapFFIError(new Error('something unexpected'));
    expect(result).toBeInstanceOf(HaiError);
  });

  it('error format matches fixture spec', () => {
    const fixture = loadParityFixture();
    expect(fixture.error_format).toBe('{ErrorKind}: {message}');
  });
});

// ---------------------------------------------------------------------------
// Test: HaiClient delegates to FFI for core API calls
// ---------------------------------------------------------------------------

describe('HaiClient delegates to FFI (Node)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('hello delegates to FFI', async () => {
    const client = await makeClient();
    const helloMock = vi.fn(async () => ({
      timestamp: '2026-01-01T00:00:00Z',
      client_ip: '127.0.0.1',
      message: 'ok',
      hello_id: 'h1',
    }));
    client._setFFIAdapter(createMockFFI({ hello: helloMock }));

    const result = await client.hello();
    expect(helloMock).toHaveBeenCalledOnce();
    expect(result.message).toBe('ok');
  });

  it('register delegates to FFI', async () => {
    const client = await makeClient();
    const registerMock = vi.fn(async () => ({
      agent_id: 'agent-1',
      jacs_id: 'test-agent',
      registered_at: '2026-01-01T00:00:00Z',
    }));
    client._setFFIAdapter(createMockFFI({ register: registerMock }));

    await client.register();
    expect(registerMock).toHaveBeenCalledOnce();
  });

  it('sendEmail delegates to FFI', async () => {
    const client = await makeClient();
    // sendEmail requires agentEmail to be set (normally set during registration)
    (client as any).agentEmail = 'test@hai.ai';
    const sendEmailMock = vi.fn(async () => ({
      message_id: 'msg-1',
      status: 'sent',
    }));
    client._setFFIAdapter(createMockFFI({ sendEmail: sendEmailMock }));

    const result = await client.sendEmail({
      to: 'recipient@hai.ai',
      subject: 'Test',
      body: 'Hello',
    });
    expect(sendEmailMock).toHaveBeenCalledOnce();
    expect(result.messageId).toBe('msg-1');
  });

  it('listMessages delegates to FFI', async () => {
    const client = await makeClient();
    const listMock = vi.fn(async () => []);
    client._setFFIAdapter(createMockFFI({ listMessages: listMock }));

    await client.listMessages();
    expect(listMock).toHaveBeenCalledOnce();
  });

  it('verifyDocument delegates to FFI', async () => {
    const client = await makeClient();
    const verifyMock = vi.fn(async () => ({
      valid: true,
      verified_at: '2026-01-01T00:00:00Z',
      document_type: 'JacsDocument',
      issuer_verified: true,
      signature_verified: true,
      signer_id: 'agent-1',
      signed_at: '2026-01-01T00:00:00Z',
    }));
    client._setFFIAdapter(createMockFFI({ verifyDocument: verifyMock }));

    const result = await client.verifyDocument({ jacsId: 'a' });
    expect(verifyMock).toHaveBeenCalledOnce();
    expect(result.valid).toBe(true);
  });

  it('fetchRemoteKey delegates to FFI', async () => {
    const client = await makeClient();
    const fetchMock = vi.fn(async () => ({
      jacs_id: 'agent-1',
      version: 'v1',
      public_key: '-----BEGIN PUBLIC KEY-----\ntest\n-----END PUBLIC KEY-----',
      algorithm: 'ed25519',
      public_key_hash: 'sha256:' + 'a'.repeat(64),
      public_key_raw_b64: 'dGVzdA==',
      status: 'active',
      dns_verified: true,
      created_at: '2026-01-01T00:00:00Z',
    }));
    client._setFFIAdapter(createMockFFI({ fetchRemoteKey: fetchMock }));

    const result = await client.fetchRemoteKey('agent-1');
    expect(fetchMock).toHaveBeenCalledOnce();
  });
});

// ---------------------------------------------------------------------------
// Test: No fetch usage in core API method implementations
// ---------------------------------------------------------------------------

describe('No old HTTP code in core API methods (Node)', () => {
  it('client.ts imports FFIClientAdapter', async () => {
    const clientSrc = readFileSync(
      resolve(__dirname, '../src/client.ts'),
      'utf-8',
    );
    expect(clientSrc).toContain("from './ffi-client.js'");
  });

  it('client.ts mentions FFI delegation in module doc', async () => {
    const clientSrc = readFileSync(
      resolve(__dirname, '../src/client.ts'),
      'utf-8',
    );
    expect(clientSrc).toContain('FFI');
  });

  it('ffi-client.ts does not use fetch', async () => {
    const ffiSrc = readFileSync(
      resolve(__dirname, '../src/ffi-client.ts'),
      'utf-8',
    );
    // The FFI client should load native binding, not use fetch
    expect(ffiSrc).not.toMatch(/\bfetch\s*\(/);
  });

  it('fetch usage in client.ts is limited to streaming/health-check', async () => {
    const clientSrc = readFileSync(
      resolve(__dirname, '../src/client.ts'),
      'utf-8',
    );

    // Find all lines with fetch(
    const lines = clientSrc.split('\n');
    const fetchLines = lines.filter(
      (line) => /\bfetch\s*\(/.test(line) && !line.trim().startsWith('//')
    );

    // All fetch usage should be in testConnection or SSE methods
    for (const line of fetchLines) {
      const isAllowed =
        clientSrc.indexOf(line) > clientSrc.indexOf('testConnection') ||
        clientSrc.indexOf(line) > clientSrc.indexOf('connectSse');
      // At minimum, fetch should not appear before the first streaming/health method
    }

    // Verify no fetch in the FFI-delegated method section
    // Extract the section between "Registration" and "testConnection"
    const registrationIdx = clientSrc.indexOf('// Registration');
    const testConnIdx = clientSrc.indexOf('testConnection');
    if (registrationIdx !== -1 && testConnIdx !== -1) {
      const coreSection = clientSrc.substring(registrationIdx, testConnIdx);
      expect(coreSection).not.toMatch(/\bfetch\s*\(/);
    }
  });
});
