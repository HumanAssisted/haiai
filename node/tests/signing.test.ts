import { describe, it, expect } from 'vitest';
import { JacsAgent, createAgentSync } from '@hai.ai/jacs';
import {
  canonicalJson,
  signResponse,
  unwrapSignedEvent,
  clearServerKeysCache,
} from '../src/signing.js';
import { TEST_AGENT, TEST_JACS_ID, TEST_PUBLIC_KEY_PEM } from './setup.js';
import { mkdtempSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

describe('JACS agent signing', () => {
  it('signs and verifies a message via JACS', () => {
    const message = 'hello world';
    const sig = TEST_AGENT.signStringSync(message);
    expect(sig).toBeTruthy();
    expect(typeof sig).toBe('string');

    const valid = TEST_AGENT.verifyStringSync(
      message,
      sig,
      Buffer.from(TEST_PUBLIC_KEY_PEM, 'utf-8'),
      'pem',
    );
    expect(valid).toBe(true);
  });

  it('rejects tampered message', () => {
    const sig = TEST_AGENT.signStringSync('original');
    const valid = TEST_AGENT.verifyStringSync(
      'tampered',
      sig,
      Buffer.from(TEST_PUBLIC_KEY_PEM, 'utf-8'),
      'pem',
    );
    expect(valid).toBe(false);
  });

  it('rejects wrong key', () => {
    // Create a second agent with different keys
    const tempDir = mkdtempSync(join(tmpdir(), 'haisdk-test-other-'));
    createAgentSync(
      'other-agent',
      'test-password-456',
      'ring-Ed25519',
      join(tempDir, 'data'),
      join(tempDir, 'keys'),
      join(tempDir, 'jacs.config.json'),
      null, null, null, null,
    );
    const otherAgent = new JacsAgent();
    otherAgent.loadSync(join(tempDir, 'jacs.config.json'));
    const otherPubKey = readFileSync(join(tempDir, 'keys', 'jacs.public.pem'), 'utf-8');

    const sig = TEST_AGENT.signStringSync('message');
    const valid = TEST_AGENT.verifyStringSync(
      'message',
      sig,
      Buffer.from(otherPubKey, 'utf-8'),
      'pem',
    );
    expect(valid).toBe(false);
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
    const result = signResponse(payload, TEST_AGENT, TEST_JACS_ID);

    expect(result.agent_jacs_id).toBe(TEST_JACS_ID);
    expect(typeof result.signed_document).toBe('string');

    const doc = JSON.parse(result.signed_document);
    expect(doc.version).toBe('1.0.0');
    expect(doc.document_type).toBe('job_response');
    expect(doc.data).toEqual(JSON.parse(canonicalJson(payload)));
    expect(doc.metadata.issuer).toBe(TEST_JACS_ID);
    expect(doc.jacsSignature.agentID).toBe(TEST_JACS_ID);
    expect(doc.metadata.hash).toBeTruthy();
  });

  it('signed response verifies correctly', () => {
    const payload = { message: 'verify me' };
    const result = signResponse(payload, TEST_AGENT, TEST_JACS_ID);
    const doc = JSON.parse(result.signed_document);

    // The signed content is canonical JSON of the data payload
    const signedContent = canonicalJson(doc.data);

    const valid = TEST_AGENT.verifyStringSync(
      signedContent,
      doc.jacsSignature.signature,
      Buffer.from(TEST_PUBLIC_KEY_PEM, 'utf-8'),
      'pem',
    );
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
    const result = signResponse(payload, TEST_AGENT, TEST_JACS_ID);
    const doc = JSON.parse(result.signed_document);

    const unwrapped = unwrapSignedEvent(doc, { [TEST_JACS_ID]: TEST_PUBLIC_KEY_PEM }, TEST_AGENT);
    expect(unwrapped).toEqual(payload);
  });

  it('throws on invalid signature with known key', () => {
    const payload = { hello: 'world' };
    const result = signResponse(payload, TEST_AGENT, TEST_JACS_ID);
    const doc = JSON.parse(result.signed_document);
    doc.jacsSignature.signature = 'invalid-signature';

    expect(() => {
      unwrapSignedEvent(doc, { [TEST_JACS_ID]: TEST_PUBLIC_KEY_PEM }, TEST_AGENT);
    }).toThrow('JACS signature verification failed');
  });
});

describe('clearServerKeysCache', () => {
  it('does not throw', () => {
    expect(() => clearServerKeysCache()).not.toThrow();
  });
});
