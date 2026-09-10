import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it, vi } from 'vitest';
import type { JacsAgent } from '@hai.ai/jacs';
import { HaiError } from '../src/errors.js';
import { signResponse, unwrapSignedEvent } from '../src/signing.js';
import { TEST_AGENT, TEST_JACS_ID, TEST_PUBLIC_KEY_PEM } from './setup.js';

interface ProtocolCase {
  name: string;
  input: Record<string, unknown>;
  job_id?: string;
  expected_data?: Record<string, unknown>;
  expected?: Record<string, unknown>;
  expected_outcome?: 'verified' | 'reject';
  expected_error_code?: string;
  native_result?: Record<string, unknown>;
}

interface ProtocolFixture {
  sign_response: {
    native_signer_required: boolean;
    missing_native_error_code: string;
    invalid_native_contract_error_code: string;
    cases: Array<ProtocolCase & {
      job_id: string;
      expected_data: Record<string, unknown>;
      expected_structure: {
        version: string;
        document_type: string;
        metadata_fields: string[];
        signature_fields: string[];
        signature_content_version: string;
      };
    }>;
  };
  unwrap_signed_event: {
    strict_by_default: boolean;
    cases: ProtocolCase[];
  };
}

function loadFixture(): ProtocolFixture {
  const here = dirname(fileURLToPath(import.meta.url));
  const path = resolve(here, '../../fixtures/protocol_parity.json');
  return JSON.parse(readFileSync(path, 'utf8')) as ProtocolFixture;
}

describe('shared signed protocol parity fixture', () => {
  const fixture = loadFixture();

  it.each(fixture.sign_response.cases)(
    'binds the v2 response contract: $name',
    (testCase) => {
      const result = signResponse(
        testCase.job_id,
        testCase.input,
        TEST_AGENT,
        TEST_JACS_ID,
        TEST_AGENT,
      );
      const document = JSON.parse(result.signed_document) as Record<string, any>;

      expect(document.version).toBe(testCase.expected_structure.version);
      expect(document.document_type).toBe(testCase.expected_structure.document_type);
      expect(document.data).toEqual(testCase.expected_data);
      for (const field of testCase.expected_structure.metadata_fields) {
        expect(document.metadata).toHaveProperty(field);
      }
      for (const field of testCase.expected_structure.signature_fields) {
        expect(document.jacsSignature).toHaveProperty(field);
      }
      expect(document.jacsSignature.signatureContentVersion)
        .toBe(testCase.expected_structure.signature_content_version);
    },
  );

  it('rejects a signer that can only create the legacy local v1 envelope', () => {
    expect(fixture.sign_response.native_signer_required).toBe(true);
    const legacySigner = {
      signStringSync: vi.fn(() => 'legacy-signature'),
      canonicalizeJsonSync: vi.fn((value: string) => value),
    } as unknown as JacsAgent;

    try {
      signResponse(
        'job-legacy',
        { response: { message: 'must not sign locally' } },
        legacySigner,
        'legacy-agent',
        legacySigner,
      );
      throw new Error('expected old JACS rejection');
    } catch (error) {
      expect(error).toBeInstanceOf(HaiError);
      expect((error as HaiError).errorCode)
        .toBe(fixture.sign_response.missing_native_error_code);
    }
    expect(legacySigner.signStringSync).not.toHaveBeenCalled();
  });

  it('rejects a native signer that returns the legacy v1 contract', () => {
    const oldNativeSigner = {
      signStringSync: vi.fn(() => 'unused'),
      signResponseSync: vi.fn(() => JSON.stringify({
        version: '1.0.0',
        document_type: 'job_response',
        data: { response: { message: 'legacy' } },
        metadata: {
          issuer: 'legacy-agent',
          document_id: 'legacy-doc',
          created_at: '2024-01-01T00:00:00Z',
          hash: 'legacy-hash',
        },
        jacsSignature: {
          agentID: 'legacy-agent',
          date: '2024-01-01T00:00:00Z',
          signature: 'legacy-signature',
        },
      })),
    } as unknown as JacsAgent;

    try {
      signResponse(
        'job-legacy',
        { response: { message: 'must not accept v1' } },
        oldNativeSigner,
        'legacy-agent',
      );
      throw new Error('expected invalid native contract rejection');
    } catch (error) {
      expect(error).toBeInstanceOf(HaiError);
      expect((error as HaiError).errorCode)
        .toBe(fixture.sign_response.invalid_native_contract_error_code);
    }
  });

  it('declares strict verification as the default', () => {
    expect(fixture.unwrap_signed_event.strict_by_default).toBe(true);
  });

  it.each(fixture.unwrap_signed_event.cases)(
    'enforces signed event outcome: $name',
    (testCase) => {
      if (testCase.expected_outcome === 'reject' && testCase.native_result === undefined) {
        try {
          unwrapSignedEvent(testCase.input, {}, TEST_AGENT);
          throw new Error('expected strict event rejection');
        } catch (error) {
          expect(error).toBeInstanceOf(HaiError);
          expect((error as HaiError).errorCode).toBe(testCase.expected_error_code);
        }
        return;
      }

      const nativeResult = testCase.native_result ?? {
        data: testCase.expected,
        verified: true,
      };
      const verifier = {
        unwrapSignedEventSync: vi.fn(() => JSON.stringify(nativeResult)),
      } as unknown as JacsAgent;

      if (testCase.expected_outcome === 'verified') {
        expect(unwrapSignedEvent(
          testCase.input,
          { 'server:v1': TEST_PUBLIC_KEY_PEM },
          verifier,
        )).toEqual(testCase.expected);
        return;
      }

      try {
        unwrapSignedEvent(
          testCase.input,
          { 'server:v1': TEST_PUBLIC_KEY_PEM },
          verifier,
        );
        throw new Error('expected strict event rejection');
      } catch (error) {
        expect(error).toBeInstanceOf(HaiError);
        expect((error as HaiError).errorCode).toBe(testCase.expected_error_code);
      }
    },
  );
});
