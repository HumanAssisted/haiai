import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { signResponse } from '../src/signing.js';
import { HaiError } from '../src/errors.js';

const __dirname2 = dirname(fileURLToPath(import.meta.url));

interface ErrorContract {
  description: string;
  error_codes: Record<string, {
    message_pattern: string;
    action_hint_pattern: string;
  }>;
  http_error_mapping: Record<string, string>;
}

function loadContract(): ErrorContract {
  const fixturePath = resolve(__dirname2, '../../fixtures/error_contract.json');
  return JSON.parse(readFileSync(fixturePath, 'utf-8')) as ErrorContract;
}

describe('error_contract', () => {
  const contract = loadContract();

  it('fixture loads successfully', () => {
    expect(contract.error_codes).toBeTruthy();
    expect(Object.keys(contract.error_codes).length).toBeGreaterThan(0);
  });

  it('JACS_NOT_LOADED error matches message pattern', () => {
    const spec = contract.error_codes['JACS_NOT_LOADED'];
    const fakeSigner = {} as never;

    try {
      signResponse({ test: true }, fakeSigner, 'test-id');
      expect.fail('Expected HaiError to be thrown');
    } catch (err) {
      expect(err).toBeInstanceOf(HaiError);
      const haiErr = err as HaiError;
      expect(haiErr.errorCode).toBe('JACS_NOT_LOADED');
      expect(haiErr.message).toMatch(new RegExp(spec.message_pattern, 'i'));
      expect(haiErr.action).toMatch(new RegExp(spec.action_hint_pattern, 'i'));
    }
  });

  it('all defined error codes have message and action patterns', () => {
    for (const [code, spec] of Object.entries(contract.error_codes)) {
      expect(spec.message_pattern, `${code} missing message_pattern`).toBeTruthy();
      expect(spec.action_hint_pattern, `${code} missing action_hint_pattern`).toBeTruthy();
    }
  });
});
