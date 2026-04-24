/**
 * Tests for `getRawEmail` on the Node SDK.
 *
 * Asserts the load-bearing R2 byte-fidelity contract: bytes passing through
 * the FFI boundary are byte-identical to what JACS signed. No trim, no
 * line-ending normalization.
 */

import { afterEach, describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { resolve } from 'node:path';

import { HaiClient } from '../src/client.js';
import { HaiError } from '../src/errors.js';
import { generateTestKeypair as generateKeypair } from './setup.js';
import { createMockFFI } from './ffi-mock.js';

async function makeClient(jacsId = 'test-agent-001'): Promise<HaiClient> {
  const keypair = generateKeypair();
  const client = await HaiClient.fromCredentials(jacsId, keypair.privateKeyPem, {
    url: 'https://hai.example',
    privateKeyPassphrase: 'keygen-password',
  });
  client.setAgentEmail(`${jacsId}@hai.ai`);
  return client;
}

describe('getRawEmail', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('decodes base64 and returns byte-identical Buffer', async () => {
    const client = await makeClient();
    const raw = Buffer.from(
      [
        ...Buffer.from('From: a\r\n'),
        ...Buffer.from('To: b\r\n\r\n'),
        0x00,
        ...Buffer.from('body '),
        0xc3, 0xa9, // é
        0xff,
        ...Buffer.from('\r\n'),
      ],
    );
    const b64 = raw.toString('base64');

    const getRawEmailMock = vi.fn(async (messageId: string) => {
      expect(messageId).toBe('m.1');
      return {
        message_id: 'm.1',
        rfc_message_id: '<a@b>',
        available: true,
        raw_email_b64: b64,
        size_bytes: raw.length,
        omitted_reason: null,
      };
    });
    client._setFFIAdapter(createMockFFI({ getRawEmail: getRawEmailMock }));

    const result = await client.getRawEmail('m.1');
    expect(result.available).toBe(true);
    expect(result.messageId).toBe('m.1');
    expect(result.rfcMessageId).toBe('<a@b>');
    expect(result.sizeBytes).toBe(raw.length);
    expect(result.omittedReason).toBeNull();
    // R2: byte-identity
    expect(result.rawEmail).not.toBeNull();
    expect(Buffer.compare(result.rawEmail!, raw)).toBe(0);
  });

  it('available:false not_stored maps to null bytes', async () => {
    const client = await makeClient();
    const mock = vi.fn(async () => ({
      message_id: 'legacy',
      available: false,
      raw_email_b64: null,
      size_bytes: null,
      omitted_reason: 'not_stored',
    }));
    client._setFFIAdapter(createMockFFI({ getRawEmail: mock }));

    const result = await client.getRawEmail('legacy');
    expect(result.available).toBe(false);
    expect(result.rawEmail).toBeNull();
    expect(result.sizeBytes).toBeNull();
    expect(result.omittedReason).toBe('not_stored');
  });

  it('available:false oversize maps to null bytes', async () => {
    const client = await makeClient();
    const mock = vi.fn(async () => ({
      message_id: 'big',
      available: false,
      raw_email_b64: null,
      size_bytes: null,
      omitted_reason: 'oversize',
    }));
    client._setFFIAdapter(createMockFFI({ getRawEmail: mock }));

    const result = await client.getRawEmail('big');
    expect(result.available).toBe(false);
    expect(result.rawEmail).toBeNull();
    expect(result.omittedReason).toBe('oversize');
  });

  it('empty messageId throws HaiError', async () => {
    const client = await makeClient();
    await expect(client.getRawEmail('')).rejects.toThrow(HaiError);
  });

  it('byte-fidelity with CRLF + NUL + non-ASCII (R2)', async () => {
    const client = await makeClient();
    const raw = Buffer.concat([
      Buffer.from('\r\n'),
      Buffer.from([0x00]),
      Buffer.from('mid'),
      Buffer.from([0xc3, 0xa9, 0xff]),
      Buffer.from('\r\n'),
    ]);
    const mock = vi.fn(async () => ({
      message_id: 'byte-id',
      available: true,
      raw_email_b64: raw.toString('base64'),
      size_bytes: raw.length,
      omitted_reason: null,
    }));
    client._setFFIAdapter(createMockFFI({ getRawEmail: mock }));

    const result = await client.getRawEmail('byte-id');
    expect(result.rawEmail).not.toBeNull();
    expect(Buffer.compare(result.rawEmail!, raw)).toBe(0);
  });
});

describe('raw_email_roundtrip conformance fixture', () => {
  it('fetched bytes match declared SHA-256 AND verify succeeds (PRD §5.4)', async () => {
    const fixturePath = resolve(__dirname, '..', '..', 'fixtures', 'email_conformance.json');
    const fixture = JSON.parse(readFileSync(fixturePath, 'utf-8'));
    const scenario = fixture.raw_email_roundtrip;
    expect(scenario).toBeDefined();

    const expectedBytes = Buffer.from(scenario.input_raw_b64, 'base64');
    const sha = createHash('sha256').update(expectedBytes).digest('hex');
    expect(sha).toBe(scenario.input_sha256);

    const client = await makeClient();
    const getRawEmailMock = vi.fn(async () => ({
      message_id: 'conf-001',
      available: scenario.expected_available,
      raw_email_b64: scenario.expected_raw_b64,
      size_bytes: scenario.expected_size_bytes,
      omitted_reason: scenario.expected_omitted_reason,
    }));

    // Capture the exact string Node passes to verifyDocument so we can assert
    // byte-identity through the verify call chain.
    let capturedVerifyInput: string | null = null;
    const verifyDocMock = vi.fn(async (doc: string) => {
      capturedVerifyInput = doc;
      return {
        valid: scenario.expected_verify_valid as boolean,
        jacs_id: scenario.verify_registry.jacs_id as string,
        algorithm: scenario.verify_registry.algorithm as string,
        reputation_tier: scenario.verify_registry.reputation_tier as string,
        dns_verified: null,
        field_results: [],
        chain: [],
        error: null,
        agent_status: scenario.verify_registry.agent_status as string,
        benchmarks_completed: [],
      } as Record<string, unknown>;
    });

    client._setFFIAdapter(
      createMockFFI({ getRawEmail: getRawEmailMock, verifyDocument: verifyDocMock }),
    );

    const result = await client.getRawEmail('conf-001');
    // Assertion 1 (PRD §5.4): byte-identity.
    expect(result.rawEmail).not.toBeNull();
    expect(Buffer.compare(result.rawEmail!, expectedBytes)).toBe(0);
    expect(result.sizeBytes).toBe(scenario.expected_size_bytes);

    // Assertion 2 (PRD §5.4): verify_email(fetched_bytes).valid === true.
    const verifyResult = await client.verifyEmail(result.rawEmail!);
    expect(verifyResult.valid).toBe(true);
    expect(verifyResult.jacsId).toBe(scenario.verify_registry.jacs_id);

    // The real JACS crypto verify runs in the Rust conformance test
    // (rust/haiai/tests/email_conformance.rs). Here we assert that Node's
    // wrapper forwards the bytes to FFI. Anchor on the JACS attachment
    // filename as an ASCII sanity check — if the wrapper dropped the MIME
    // altogether we would miss it immediately.
    expect(capturedVerifyInput).not.toBeNull();
    expect(capturedVerifyInput!).toContain('jacs-signature.json');
  });
});
