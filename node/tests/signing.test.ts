import { describe, it, expect, vi } from 'vitest';
import type { JacsAgent } from '@hai.ai/jacs';
import {
  canonicalJson,
  signResponse,
  unwrapSignedEvent,
  clearServerKeysCache,
} from '../src/signing.js';
import { TEST_AGENT, TEST_JACS_ID, TEST_PUBLIC_KEY_PEM } from './setup.js';

describe('JACS agent signing', () => {
  it('signs a message via JACS', () => {
    const message = 'hello world';
    const sig = TEST_AGENT.signStringSync(message);
    expect(sig).toBeTruthy();
    expect(typeof sig).toBe('string');
  });

  it('produces different signatures for different messages', () => {
    const sig1 = TEST_AGENT.signStringSync('message-one');
    const sig2 = TEST_AGENT.signStringSync('message-two');
    expect(sig1).not.toBe(sig2);
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
    // JACS 0.9.4 signResponseSync uses agent's internal ID for issuer/agentID
    expect(doc.metadata.issuer).toBeTruthy();
    expect(doc.jacsSignature.agentID).toBe(doc.metadata.issuer);
    expect(doc.metadata.hash).toBeTruthy();
  });

  it('signed response verifies correctly', () => {
    const payload = { message: 'verify me' };
    const result = signResponse(payload, TEST_AGENT, TEST_JACS_ID);
    const doc = JSON.parse(result.signed_document);
    expect(typeof doc.jacsSignature.signature).toBe('string');
    expect(doc.jacsSignature.signature.length).toBeGreaterThan(0);
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
    // Use the actual agentID from the signed doc (JACS 0.9.4 uses internal UUID)
    const agentID = doc.jacsSignature.agentID;
    const agent = {
      verifyStringSync: vi.fn(() => true),
    } as unknown as JacsAgent;

    const unwrapped = unwrapSignedEvent(doc, { [agentID]: TEST_PUBLIC_KEY_PEM }, agent);
    expect(unwrapped).toEqual(payload);
    expect(agent.verifyStringSync).toHaveBeenCalledOnce();
  });

  it('throws on invalid signature with known key', () => {
    const payload = { hello: 'world' };
    const result = signResponse(payload, TEST_AGENT, TEST_JACS_ID);
    const doc = JSON.parse(result.signed_document);
    const agentID = doc.jacsSignature.agentID;
    doc.jacsSignature.signature = 'invalid-signature';
    const agent = {
      verifyStringSync: vi.fn(() => false),
    } as unknown as JacsAgent;

    expect(() => {
      unwrapSignedEvent(doc, { [agentID]: TEST_PUBLIC_KEY_PEM }, agent);
    }).toThrow('JACS signature verification failed');
  });

  it('propagates verification errors instead of swallowing them', () => {
    const payload = { hello: 'world' };
    const result = signResponse(payload, TEST_AGENT, TEST_JACS_ID);
    const doc = JSON.parse(result.signed_document);
    const agentID = doc.jacsSignature.agentID;
    const agent = {
      verifyStringSync: vi.fn(() => { throw new Error('invalid PEM format'); }),
    } as unknown as JacsAgent;

    expect(() => {
      unwrapSignedEvent(doc, { [agentID]: TEST_PUBLIC_KEY_PEM }, agent);
    }).toThrow('invalid PEM format');
  });
});

describe('clearServerKeysCache', () => {
  it('does not throw', () => {
    expect(() => clearServerKeysCache()).not.toThrow();
  });
});
