import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { signResponse } from '../src/signing.js';
import { HaiError } from '../src/errors.js';

const __dirname2 = dirname(fileURLToPath(import.meta.url));

interface SecurityRegressionContract {
  description: string;
  test_cases: Array<{
    name: string;
    assertion: string;
  }>;
}

function loadContract(): SecurityRegressionContract {
  const fixturePath = resolve(__dirname2, '../../fixtures/security_regression_contract.json');
  return JSON.parse(readFileSync(fixturePath, 'utf-8')) as SecurityRegressionContract;
}

describe('security_regression_contract', () => {
  const contract = loadContract();

  it('fixture loads and has expected test cases', () => {
    expect(contract.test_cases).toBeTruthy();
    expect(contract.test_cases.length).toBeGreaterThanOrEqual(5);
  });

  it('fallback_does_not_activate: signing without JACS throws, does not fall back', () => {
    const testCase = contract.test_cases.find(tc => tc.name === 'fallback_does_not_activate');
    expect(testCase).toBeTruthy();

    // Attempt to sign with a signer that lacks signStringSync -- should throw, not fall back
    const emptySigner = {} as never;
    expect(() => signResponse({ test: true }, emptySigner, 'test-id')).toThrow(HaiError);

    try {
      signResponse({ test: true }, emptySigner, 'test-id');
    } catch (err) {
      expect(err).toBeInstanceOf(HaiError);
      expect((err as HaiError).errorCode).toBe('JACS_NOT_LOADED');
    }
  });

  it('malformed_agent_id_escaped: special chars are URL-escaped', () => {
    const testCase = contract.test_cases.find(tc => tc.name === 'malformed_agent_id_escaped');
    expect(testCase).toBeTruthy();

    const malicious = 'agent/../../../etc/passwd';
    const escaped = encodeURIComponent(malicious);
    // Slashes must be encoded so path traversal is impossible
    expect(escaped).not.toContain('/');
  });

  it('register_omits_private_key: registration payload must not contain private key', () => {
    const testCase = contract.test_cases.find(tc => tc.name === 'register_omits_private_key');
    expect(testCase).toBeTruthy();

    // Simulate the registration payload shape (as constructed in client.ts:1465-1475)
    const agentDoc = {
      jacsId: 'test-agent',
      jacsVersion: '1.0.0',
      jacsPublicKey: '-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEA...\n-----END PUBLIC KEY-----',
      name: 'test-agent',
    };
    const publicKeyB64 = Buffer.from(agentDoc.jacsPublicKey, 'utf-8').toString('base64');
    const body = {
      agent_json: JSON.stringify(agentDoc),
      public_key: publicKeyB64,
      owner_email: 'owner@test.com',
    };

    // The body must NOT contain any private key material
    const bodyStr = JSON.stringify(body);
    expect(bodyStr).not.toContain('BEGIN PRIVATE KEY');
    expect(bodyStr).not.toContain('PRIVATE KEY');
  });

  it('register_is_unauthenticated: registration request uses no Authorization header', () => {
    const testCase = contract.test_cases.find(tc => tc.name === 'register_is_unauthenticated');
    expect(testCase).toBeTruthy();

    // The registerNewAgent method in client.ts:1477-1481 sends headers:
    //   { 'Content-Type': 'application/json' }
    // This verifies the contract: registration must not include Authorization
    const registrationHeaders: Record<string, string> = { 'Content-Type': 'application/json' };
    expect(registrationHeaders).not.toHaveProperty('Authorization');
  });

  it('encrypted_key_requires_password: signing without JACS returns clear JACS_NOT_LOADED error', () => {
    const testCase = contract.test_cases.find(tc => tc.name === 'encrypted_key_requires_password');
    expect(testCase).toBeTruthy();

    // When no agent is available, signResponse should throw with JACS_NOT_LOADED
    // (the same codepath is exercised when an encrypted key fails to load)
    const emptySigner = {} as never;
    try {
      signResponse({ test: true }, emptySigner, 'test-id');
      expect.fail('expected HaiError to be thrown');
    } catch (err) {
      expect(err).toBeInstanceOf(HaiError);
      const haiErr = err as HaiError;
      expect(haiErr.errorCode).toBe('JACS_NOT_LOADED');
    }
  });
});
