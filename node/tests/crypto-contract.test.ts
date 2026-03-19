import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { canonicalJson, signResponse } from '../src/signing.js';
import { HaiError } from '../src/errors.js';
import { TEST_AGENT, TEST_JACS_ID } from './setup.js';

const __dirname2 = dirname(fileURLToPath(import.meta.url));

interface CryptoDelegationContract {
  description: string;
  canonicalization: {
    test_vectors: Array<{ input: unknown; expected: string }>;
    jacs_required: boolean;
    error_when_no_jacs: string;
  };
  signing: {
    operations: string[];
    jacs_required: boolean;
    error_when_no_jacs: string;
  };
  verification: {
    operations: string[];
    jacs_required: boolean;
    error_when_no_jacs: string;
  };
}

function loadContract(): CryptoDelegationContract {
  const fixturePath = resolve(__dirname2, '../../fixtures/crypto_delegation_contract.json');
  return JSON.parse(readFileSync(fixturePath, 'utf-8')) as CryptoDelegationContract;
}

describe('crypto_delegation_contract', () => {
  const contract = loadContract();

  describe('canonicalization test vectors', () => {
    for (const vec of contract.canonicalization.test_vectors) {
      it(`canonicalizes ${JSON.stringify(vec.input)} correctly`, () => {
        const result = canonicalJson(vec.input);
        expect(result).toBe(vec.expected);
      });
    }
  });

  describe('signing requires JACS', () => {
    it('fixture asserts jacs_required for signing', () => {
      expect(contract.signing.jacs_required).toBe(true);
    });

    it('signResponse throws HaiError with JACS_NOT_LOADED when signer lacks signStringSync', () => {
      const fakeSigner = {} as never;
      expect(() => signResponse({ test: true }, fakeSigner, 'test-id')).toThrow(HaiError);
      try {
        signResponse({ test: true }, fakeSigner, 'test-id');
      } catch (err) {
        expect(err).toBeInstanceOf(HaiError);
        expect((err as HaiError).errorCode).toBe(contract.signing.error_when_no_jacs);
      }
    });
  });

  describe('signing works with JACS agent', () => {
    it('signResponse succeeds with a valid JACS agent', () => {
      const result = signResponse({ test: 'value' }, TEST_AGENT, TEST_JACS_ID);
      expect(result.signed_document).toBeTruthy();
      expect(typeof result.signed_document).toBe('string');
      const doc = JSON.parse(result.signed_document);
      expect(doc.jacsSignature).toBeTruthy();
      expect(doc.jacsSignature.signature).toBeTruthy();
    });
  });
});
