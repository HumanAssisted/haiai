import { describe, it, expect } from 'vitest';
import { signString, verifyString, generateKeypair } from '../src/crypt.js';
import {
  canonicalJson,
  signResponse,
  unwrapSignedEvent,
  clearServerKeysCache,
} from '../src/signing.js';
import { TEST_KEYPAIR, TEST_JACS_ID } from './setup.js';

describe('crypt', () => {
  it('generates an Ed25519 keypair', () => {
    const kp = generateKeypair();
    expect(kp.publicKeyPem).toContain('-----BEGIN PUBLIC KEY-----');
    expect(kp.privateKeyPem).toContain('-----BEGIN PRIVATE KEY-----');
  });

  it('signs and verifies a message', () => {
    const message = 'hello world';
    const sig = signString(TEST_KEYPAIR.privateKeyPem, message);
    expect(sig).toBeTruthy();
    expect(typeof sig).toBe('string');

    const valid = verifyString(TEST_KEYPAIR.publicKeyPem, message, sig);
    expect(valid).toBe(true);
  });

  it('rejects tampered message', () => {
    const sig = signString(TEST_KEYPAIR.privateKeyPem, 'original');
    const valid = verifyString(TEST_KEYPAIR.publicKeyPem, 'tampered', sig);
    expect(valid).toBe(false);
  });

  it('rejects wrong key', () => {
    const other = generateKeypair();
    const sig = signString(TEST_KEYPAIR.privateKeyPem, 'message');
    const valid = verifyString(other.publicKeyPem, 'message', sig);
    expect(valid).toBe(false);
  });

  it('signString returns base64', () => {
    const sig = signString(TEST_KEYPAIR.privateKeyPem, 'test');
    // Ed25519 signatures are 64 bytes -> ~88 chars in base64
    expect(Buffer.from(sig, 'base64').length).toBe(64);
  });
});

describe('canonicalJson', () => {
  it('sorts keys deterministically', () => {
    const result = canonicalJson({ z: 1, a: 2, m: 3 });
    expect(result).toBe('{"a":2,"m":3,"z":1}');
  });

  it('sorts nested keys', () => {
    const result = canonicalJson({ b: { z: 1, a: 2 }, a: 1 });
    expect(result).toBe('{"a":1,"b":{"a":2,"z":1}}');
  });

  it('handles arrays without reordering', () => {
    const result = canonicalJson({ arr: [3, 1, 2] });
    expect(result).toBe('{"arr":[3,1,2]}');
  });

  it('handles null and primitives', () => {
    expect(canonicalJson(null)).toBe('null');
    expect(canonicalJson(42)).toBe('42');
    expect(canonicalJson('hello')).toBe('"hello"');
  });
});

describe('signResponse', () => {
  it('creates a signed JACS document', () => {
    const payload = { response: { message: 'test', metadata: null, processing_time_ms: 0 } };
    const result = signResponse(payload, TEST_KEYPAIR.privateKeyPem, TEST_JACS_ID);

    expect(result.agent_jacs_id).toBe(TEST_JACS_ID);
    expect(typeof result.signed_document).toBe('string');

    const doc = JSON.parse(result.signed_document);
    expect(doc.payload).toEqual(payload);
    expect(doc.metadata.issuer).toBe(TEST_JACS_ID);
    expect(doc.signature.key_id).toBe(TEST_JACS_ID);
    expect(doc.signature.algorithm).toBe('Ed25519');
    expect(doc.metadata.hash).toBeTruthy();
  });

  it('signed response verifies correctly', () => {
    const payload = { message: 'verify me' };
    const result = signResponse(payload, TEST_KEYPAIR.privateKeyPem, TEST_JACS_ID);
    const doc = JSON.parse(result.signed_document);

    // Reconstruct what was signed
    const signedContent = canonicalJson({
      metadata: doc.metadata,
      payload: doc.payload,
    });

    const valid = verifyString(TEST_KEYPAIR.publicKeyPem, signedContent, doc.signature.signature);
    expect(valid).toBe(true);
  });
});

describe('unwrapSignedEvent', () => {
  it('passes through non-JACS events unchanged', () => {
    const event = { type: 'heartbeat', timestamp: 123 };
    const result = unwrapSignedEvent(event, {});
    expect(result).toEqual(event);
  });

  it('unwraps JACS document without verification when no key available', () => {
    const doc = {
      payload: { message: 'inner' },
      metadata: { issuer: 'agent-1', document_id: 'doc-1', created_at: 'now', hash: 'abc' },
      signature: { key_id: 'unknown-key', algorithm: 'Ed25519', signature: 'sig', signed_at: 'now' },
    };
    const result = unwrapSignedEvent(doc, {});
    expect(result).toEqual({ message: 'inner' });
  });

  it('verifies and unwraps with known key', () => {
    const payload = { hello: 'world' };
    const result = signResponse(payload, TEST_KEYPAIR.privateKeyPem, TEST_JACS_ID);
    const doc = JSON.parse(result.signed_document);

    const unwrapped = unwrapSignedEvent(doc, { [TEST_JACS_ID]: TEST_KEYPAIR.publicKeyPem });
    expect(unwrapped).toEqual(payload);
  });

  it('throws on invalid signature with known key', () => {
    const payload = { hello: 'world' };
    const result = signResponse(payload, TEST_KEYPAIR.privateKeyPem, TEST_JACS_ID);
    const doc = JSON.parse(result.signed_document);
    doc.signature.signature = 'invalid-signature';

    expect(() => {
      unwrapSignedEvent(doc, { [TEST_JACS_ID]: TEST_KEYPAIR.publicKeyPem });
    }).toThrow('JACS signature verification failed');
  });
});

describe('clearServerKeysCache', () => {
  it('does not throw', () => {
    expect(() => clearServerKeysCache()).not.toThrow();
  });
});
