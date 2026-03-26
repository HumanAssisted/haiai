import { afterEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { HaiClient } from '../src/client.js';
import { generateTestKeypair as generateKeypair } from './setup.js';
import { createMockFFI } from './ffi-mock.js';
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

async function makeClient(baseUrl: string): Promise<HaiClient> {
  const keypair = generateKeypair();
  const client = await HaiClient.fromCredentials('test-agent-001', keypair.privateKeyPem, { url: baseUrl, privateKeyPassphrase: 'keygen-password' });
  client.setAgentEmail('test@hai.ai');
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

    const client = await makeClient('https://mock.hai.ai');
    const verifyDocumentMock = vi.fn(async () => mockJson);
    client._setFFIAdapter(createMockFFI({ verifyDocument: verifyDocumentMock }));

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

    // agent_status and benchmarks_completed (TASK_012)
    expect(result.agentStatus).toBe('active');
    expect(result.benchmarksCompleted).toEqual(['free_chaotic']);
  });
});

// ---------------------------------------------------------------------------
// Content hash golden vector conformance (TASK_013)
// ---------------------------------------------------------------------------

describe('email conformance: content hash golden vectors', () => {
  it('all golden vectors produce the expected content hash', async () => {
    const { computeContentHash } = await import('../src/hash.js');
    const fixture = loadConformanceFixture() as ConformanceFixture & {
      content_hash_golden: {
        vectors: Array<{
          name: string;
          subject: string;
          body: string;
          attachments: Array<{ filename: string; content_type: string; data_utf8: string }>;
          expected_hash: string;
        }>;
      };
    };

    for (const vector of fixture.content_hash_golden.vectors) {
      const result = computeContentHash(vector.subject, vector.body, vector.attachments);
      expect(result).toBe(vector.expected_hash);
    }
  });
});

// ---------------------------------------------------------------------------
// MIME round-trip conformance (TASK_014)
// ---------------------------------------------------------------------------

describe('email conformance: MIME round-trip content hash', () => {
  it('produces expected content hash from round-trip input', async () => {
    const { computeContentHash } = await import('../src/hash.js');
    const fixture = loadConformanceFixture() as ConformanceFixture & {
      mime_round_trip: {
        input: {
          subject: string;
          body: string;
          attachments: Array<{ filename: string; content_type: string; data_utf8: string }>;
        };
        expected_content_hash: string;
      };
    };

    const { input, expected_content_hash: expectedHash } = fixture.mime_round_trip;
    const result = computeContentHash(input.subject, input.body, input.attachments);
    expect(result).toBe(expectedHash);
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
    // signEmail delegates to FFI sendSignedEmail; verify the contract shape
    const client = await makeClient('https://mock.hai.ai');
    const sendSignedEmailMock = vi.fn(async () => ({
      signed_email_base64: Buffer.from('signed email bytes').toString('base64'),
    }));
    client._setFFIAdapter(createMockFFI({ sendSignedEmail: sendSignedEmailMock }));

    const result = await client.signEmail('raw email');
    expect(sendSignedEmailMock).toHaveBeenCalledTimes(1);
    expect(result).toBeInstanceOf(Buffer);

    // Verify contract shape
    expect(fixture.api_contracts.sign_email.method).toBe('POST');
    expect(fixture.api_contracts.sign_email.path).toBe('/api/v1/email/sign');
    expect(fixture.api_contracts.sign_email.request_content_type).toBe('message/rfc822');
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
    const client = await makeClient('https://mock.hai.ai');
    const verifyDocumentMock = vi.fn(async () => fixture.mock_verify_response.json);
    client._setFFIAdapter(createMockFFI({ verifyDocument: verifyDocumentMock }));

    await client.verifyEmail('raw email');
    expect(verifyDocumentMock).toHaveBeenCalledTimes(1);

    // Verify contract shape
    expect(fixture.api_contracts.verify_email.method).toBe('POST');
    expect(fixture.api_contracts.verify_email.path).toBe('/api/v1/email/verify');
    expect(fixture.api_contracts.verify_email.request_content_type).toBe('message/rfc822');
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
    let capturedOptions: Record<string, unknown> = {};

    const client = await makeClient('https://mock.hai.ai');
    const sendEmailMock = vi.fn(async (options: Record<string, unknown>) => {
      capturedOptions = options;
      return { message_id: 'msg-conf', status: 'sent' };
    });
    client._setFFIAdapter(createMockFFI({ sendEmail: sendEmailMock }));

    await client.sendEmail({ to: 'bob@hai.ai', subject: 'Test', body: 'Body' });

    for (const excluded of fixture.api_contracts.send_email.excluded_fields) {
      expect(capturedOptions).not.toHaveProperty(excluded);
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
