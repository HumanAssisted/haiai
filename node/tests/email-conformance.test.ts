import { afterEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { HaiClient } from '../src/client.js';
import { generateKeypair } from '../src/crypt.js';
import {
  EmailNotActiveError,
  RecipientNotFoundError,
  RateLimitedError,
} from '../src/errors.js';
import type {
  EmailVerificationResultV2,
  FieldStatus,
} from '../src/types.js';

// ---------------------------------------------------------------------------
// Fixture loading
// ---------------------------------------------------------------------------

interface ConformanceFixture {
  verification_result_v2_schema: {
    required_fields: Record<string, string>;
    field_status_values: string[];
  };
  api_contracts: {
    sign_email: { method: string; path: string; request_content_type: string };
    verify_email: { method: string; path: string; request_content_type: string };
    send_email: { excluded_fields: string[] };
  };
  mock_verify_response: { json: Record<string, unknown> };
}

function loadConformanceFixture(): ConformanceFixture {
  const here = dirname(fileURLToPath(import.meta.url));
  const fixturePath = resolve(here, '../../fixtures/email_conformance.json');
  return JSON.parse(readFileSync(fixturePath, 'utf-8')) as ConformanceFixture;
}

function makeClient(baseUrl: string): HaiClient {
  const keypair = generateKeypair();
  const client = HaiClient.fromCredentials('test-agent-001', keypair.privateKeyPem, { url: baseUrl });
  client.agentEmail = 'test@hai.ai';
  return client;
}

// ---------------------------------------------------------------------------
// EmailVerificationResultV2 structural conformance
// ---------------------------------------------------------------------------

describe('email conformance: mock verify response deserialization', () => {
  const fixture = loadConformanceFixture();

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('deserializes mock response into EmailVerificationResultV2 via verifyEmail', async () => {
    const mockJson = fixture.mock_verify_response.json;

    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: () => Promise.resolve(mockJson),
    }));

    const client = makeClient('https://mock.hai.ai');
    const result: EmailVerificationResultV2 = await client.verifyEmail('raw email content');

    expect(result.valid).toBe(true);
    expect(result.jacsId).toBe('conformance-test-agent-001');
    expect(result.algorithm).toBe('ed25519');
    expect(result.reputationTier).toBe('established');
    expect(result.dnsVerified).toBe(true);
    expect(result.error).toBeNull();

    // field_results
    expect(result.fieldResults).toHaveLength(4);
    expect(result.fieldResults[0].field).toBe('subject');
    expect(result.fieldResults[0].status).toBe('pass');
    expect(result.fieldResults[3].field).toBe('date');
    expect(result.fieldResults[3].status).toBe('modified');

    // chain
    expect(result.chain).toHaveLength(1);
    expect(result.chain[0].signer).toBe('agent@hai.ai');
    expect(result.chain[0].jacsId).toBe('conformance-test-agent-001');
    expect(result.chain[0].valid).toBe(true);
    expect(result.chain[0].forwarded).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// FieldStatus enum conformance
// ---------------------------------------------------------------------------

describe('email conformance: FieldStatus values', () => {
  const fixture = loadConformanceFixture();

  it('all fixture field_status_values are valid FieldStatus literals', () => {
    const validStatuses: FieldStatus[] = ['pass', 'modified', 'fail', 'unverifiable'];
    for (const val of fixture.verification_result_v2_schema.field_status_values) {
      expect(validStatuses).toContain(val);
    }
    expect(fixture.verification_result_v2_schema.field_status_values).toHaveLength(validStatuses.length);
  });
});

// ---------------------------------------------------------------------------
// API contract conformance: SignEmail
// ---------------------------------------------------------------------------

describe('email conformance: signEmail API contract', () => {
  const fixture = loadConformanceFixture();

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('sends POST to correct path with message/rfc822 content-type', async () => {
    let gotMethod = '';
    let gotPath = '';
    let gotContentType = '';

    vi.stubGlobal('fetch', vi.fn().mockImplementation((url: string, init?: RequestInit) => {
      const u = new URL(url);
      gotMethod = init?.method ?? 'GET';
      gotPath = u.pathname;
      gotContentType = (init?.headers as Record<string, string>)?.['Content-Type'] ?? '';
      return Promise.resolve({
        ok: true,
        status: 200,
        arrayBuffer: () => Promise.resolve(new ArrayBuffer(0)),
      });
    }));

    const client = makeClient('https://mock.hai.ai');
    await client.signEmail('raw email');

    expect(gotMethod).toBe(fixture.api_contracts.sign_email.method);
    expect(gotPath).toBe(fixture.api_contracts.sign_email.path);
    expect(gotContentType).toBe(fixture.api_contracts.sign_email.request_content_type);
  });
});

// ---------------------------------------------------------------------------
// API contract conformance: VerifyEmail
// ---------------------------------------------------------------------------

describe('email conformance: verifyEmail API contract', () => {
  const fixture = loadConformanceFixture();

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('sends POST to correct path with message/rfc822 content-type', async () => {
    let gotMethod = '';
    let gotPath = '';
    let gotContentType = '';

    vi.stubGlobal('fetch', vi.fn().mockImplementation((url: string, init?: RequestInit) => {
      const u = new URL(url);
      gotMethod = init?.method ?? 'GET';
      gotPath = u.pathname;
      gotContentType = (init?.headers as Record<string, string>)?.['Content-Type'] ?? '';
      return Promise.resolve({
        ok: true,
        status: 200,
        json: () => Promise.resolve(fixture.mock_verify_response.json),
      });
    }));

    const client = makeClient('https://mock.hai.ai');
    await client.verifyEmail('raw email');

    expect(gotMethod).toBe(fixture.api_contracts.verify_email.method);
    expect(gotPath).toBe(fixture.api_contracts.verify_email.path);
    expect(gotContentType).toBe(fixture.api_contracts.verify_email.request_content_type);
  });
});

// ---------------------------------------------------------------------------
// API contract conformance: SendEmail excluded fields
// ---------------------------------------------------------------------------

describe('email conformance: sendEmail excluded fields', () => {
  const fixture = loadConformanceFixture();

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('does not send client-side signing fields', async () => {
    let gotBody: Record<string, unknown> = {};

    vi.stubGlobal('fetch', vi.fn().mockImplementation((_url: string, init?: RequestInit) => {
      if (init?.body) {
        gotBody = JSON.parse(init.body as string) as Record<string, unknown>;
      }
      return Promise.resolve({
        ok: true,
        status: 200,
        json: () => Promise.resolve({ message_id: 'msg-conf', status: 'sent' }),
      });
    }));

    const client = makeClient('https://mock.hai.ai');
    await client.sendEmail({ to: 'bob@hai.ai', subject: 'Test', body: 'Body' });

    for (const excluded of fixture.api_contracts.send_email.excluded_fields) {
      expect(gotBody).not.toHaveProperty(excluded);
    }
  });
});

// ---------------------------------------------------------------------------
// Error type conformance
// ---------------------------------------------------------------------------

describe('email conformance: error types', () => {
  it('all email error sentinel classes exist', () => {
    expect(EmailNotActiveError).toBeDefined();
    expect(RecipientNotFoundError).toBeDefined();
    expect(RateLimitedError).toBeDefined();

    // Verify they are constructable
    const e1 = new EmailNotActiveError('test');
    const e2 = new RecipientNotFoundError('test');
    const e3 = new RateLimitedError('test');
    expect(e1.message).toBe('test');
    expect(e2.message).toBe('test');
    expect(e3.message).toBe('test');
  });
});
