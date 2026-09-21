import { afterEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { HaiClient } from '../src/client.js';
import { FFIClientAdapter } from '../src/ffi-client.js';
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
    const verifyEmailRawMock = vi.fn(async () => mockJson);
    client._setFFIAdapter(createMockFFI({ verifyEmailRaw: verifyEmailRawMock }));

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
// Raw-email FFI contract: signing returns base64, verification returns JSON.
// ---------------------------------------------------------------------------

const rawEmailCases = [
  {
    name: 'UTF-8 string',
    rawEmail: 'From: alice@example.test\r\nSubject: café\r\n\r\nHello ☃\r\n',
  },
  {
    name: 'binary Buffer',
    rawEmail: Buffer.concat([
      Buffer.from('From: alice@example.test\r\nContent-Transfer-Encoding: binary\r\n\r\n'),
      Buffer.from([0x00, 0xc3, 0xa9, 0xff, 0x0d, 0x0a]),
    ]),
  },
];

describe('email conformance: signEmail raw-email FFI contract', () => {

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it.each(rawEmailCases)('signs $name through signEmailRaw without sending it', async ({ rawEmail }) => {
    const client = await makeClient('https://mock.hai.ai');
    const signedBytes = Buffer.concat([
      Buffer.from('Content-Type: multipart/mixed; boundary=signed\r\n\r\n'),
      Buffer.from([0x00, 0xff, 0x0d, 0x0a]),
    ]);
    const signEmailRawMock = vi.fn(async () => signedBytes.toString('base64'));
    const sendSignedEmailMock = vi.fn(async () => {
      throw new Error('signEmail must not send an email');
    });
    client._setFFIAdapter(createMockFFI({
      signEmailRaw: signEmailRawMock,
      sendSignedEmail: sendSignedEmailMock,
    }));

    const result = await client.signEmail(rawEmail);
    expect(signEmailRawMock).toHaveBeenCalledTimes(1);
    expect(signEmailRawMock).toHaveBeenCalledWith(Buffer.from(rawEmail).toString('base64'));
    expect(sendSignedEmailMock).not.toHaveBeenCalled();
    expect(result).toBeInstanceOf(Buffer);
    expect(result.equals(signedBytes)).toBe(true);
  });

  it('preserves the native signing result as a base64 string rather than parsing JSON', async () => {
    const inputB64 = Buffer.from('Subject: sign\r\n\r\nbody\r\n').toString('base64');
    const signedB64 = Buffer.from('Subject: signed\r\n\r\nbody\r\n').toString('base64');
    const signEmailRaw = vi.fn(async () => signedB64);
    // Exercise the real adapter conversion with a native boundary double.
    const adapter = Object.assign(Object.create(FFIClientAdapter.prototype), {
      native: { signEmailRaw },
    }) as FFIClientAdapter;

    await expect(adapter.signEmailRaw(inputB64)).resolves.toBe(signedB64);
    expect(signEmailRaw).toHaveBeenCalledTimes(1);
    expect(signEmailRaw).toHaveBeenCalledWith(inputB64);
  });

  it('maps native raw-email signing errors without returning empty bytes', async () => {
    const adapter = Object.assign(Object.create(FFIClientAdapter.prototype), {
      native: { signEmailRaw: vi.fn(async () => { throw new Error('NotFound: email not active'); }) },
    }) as FFIClientAdapter;

    await expect(adapter.signEmailRaw('')).rejects.toBeInstanceOf(EmailNotActiveError);
  });
});

// ---------------------------------------------------------------------------
// API contract conformance: VerifyEmail
// ---------------------------------------------------------------------------

describe('email conformance: verifyEmail raw-email FFI contract', () => {
  const fixture = loadConformanceFixture();

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it.each(rawEmailCases)('verifies exact $name bytes through verifyEmailRaw', async ({ rawEmail }) => {
    const client = await makeClient('https://mock.hai.ai');
    const verifyEmailRawMock = vi.fn(async () => fixture.mock_verify_response.json);
    const verifyDocumentMock = vi.fn(async () => {
      throw new Error('verifyEmail must not use JSON-document verification');
    });
    client._setFFIAdapter(createMockFFI({
      verifyEmailRaw: verifyEmailRawMock,
      verifyDocument: verifyDocumentMock,
    }));

    const result = await client.verifyEmail(rawEmail);
    expect(verifyEmailRawMock).toHaveBeenCalledTimes(1);
    expect(verifyEmailRawMock).toHaveBeenCalledWith(Buffer.from(rawEmail).toString('base64'));
    expect(verifyDocumentMock).not.toHaveBeenCalled();
    expect(result.valid).toBe(true);
  });

  it('decodes the native JSON verification response', async () => {
    const inputB64 = Buffer.from('Subject: signed\r\n\r\nbody\r\n').toString('base64');
    const verifyEmailRaw = vi.fn(async () => JSON.stringify(fixture.mock_verify_response.json));
    const adapter = Object.assign(Object.create(FFIClientAdapter.prototype), {
      native: { verifyEmailRaw },
    }) as FFIClientAdapter;

    await expect(adapter.verifyEmailRaw(inputB64)).resolves.toEqual(fixture.mock_verify_response.json);
    expect(verifyEmailRaw).toHaveBeenCalledTimes(1);
    expect(verifyEmailRaw).toHaveBeenCalledWith(inputB64);
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
