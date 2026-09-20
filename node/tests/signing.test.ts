import { describe, it, expect, vi } from 'vitest';
import type { JacsAgent } from '@hai.ai/jacs';
import {
  canonicalJson,
  signPayload,
  signResponse,
  unwrapSignedEvent,
  clearServerKeysCache,
  getServerKeys,
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
    const result = canonicalJson({ z: 1, a: 2, m: 3 }, TEST_AGENT);
    expect(result).toBe('{"a":2,"m":3,"z":1}');
  });

  it('sorts nested keys', () => {
    const result = canonicalJson({ b: { z: 1, a: 2 }, a: 1 }, TEST_AGENT);
    expect(result).toBe('{"a":1,"b":{"a":2,"z":1}}');
  });

  it('handles arrays without reordering', () => {
    const result = canonicalJson({ arr: [3, 1, 2] }, TEST_AGENT);
    expect(result).toBe('{"arr":[3,1,2]}');
  });

  it('handles null and primitives', () => {
    expect(canonicalJson(null, TEST_AGENT)).toBe('null');
    expect(canonicalJson(42, TEST_AGENT)).toBe('42');
    expect(canonicalJson('hello', TEST_AGENT)).toBe('"hello"');
  });

  it('throws when no JACS agent is provided', () => {
    expect(() => canonicalJson({ a: 1 })).toThrow(/loaded JACS agent/);
  });

  it('throws when agent lacks canonicalizeJsonSync', () => {
    const stubAgent = {} as unknown as JacsAgent;
    expect(() => canonicalJson({ a: 1 }, stubAgent)).toThrow(/canonicalizeJsonSync/);
  });
});

describe('signResponse', () => {
  it('cryptographically binds the v2 contract and job ID', () => {
    const jobId = '550e8400-e29b-41d4-a716-446655440000';
    const request = {
      response: { message: 'bound', metadata: null, processing_time_ms: 12 },
    };

    const result = signResponse(jobId, request, TEST_AGENT, TEST_JACS_ID, TEST_AGENT);
    const doc = JSON.parse(result.signed_document);
    expect(doc.data).toEqual({
      contract: 'hai.job-response',
      version: 2,
      job_id: jobId,
      response: request.response,
    });
  });

  it('creates a signed JACS document', () => {
    const jobId = '550e8400-e29b-41d4-a716-446655440000';
    const payload = { response: { message: 'test', metadata: null, processing_time_ms: 0 } };
    const result = signResponse(jobId, payload, TEST_AGENT, TEST_JACS_ID);

    expect(result.agent_jacs_id).toBe(TEST_JACS_ID);
    expect(typeof result.signed_document).toBe('string');

    const doc = JSON.parse(result.signed_document);
    expect(doc.version).toBe('2.0.0');
    expect(doc.document_type).toBe('job_response');
    expect(doc.data).toEqual({
      contract: 'hai.job-response',
      version: 2,
      job_id: jobId,
      response: payload.response,
    });
    // JACS 0.9.4 signResponseSync uses agent's internal ID for issuer/agentID
    expect(doc.metadata.issuer).toBeTruthy();
    expect(doc.jacsSignature.agentID).toBe(doc.metadata.issuer);
    expect(doc.metadata.hash).toBeTruthy();
  });

  it('signed response verifies correctly', () => {
    const payload = { response: { message: 'verify me' } };
    const result = signResponse('job-verify', payload, TEST_AGENT, TEST_JACS_ID);
    const doc = JSON.parse(result.signed_document);
    expect(typeof doc.jacsSignature.signature).toBe('string');
    expect(doc.jacsSignature.signature.length).toBeGreaterThan(0);
  });
});

describe('unwrapSignedEvent', () => {
  it('rejects non-JACS events instead of treating them as verified data', () => {
    const event = { type: 'heartbeat', timestamp: 123 };
    expect(() => unwrapSignedEvent(event, {}, TEST_AGENT)).toThrow(/fully bound|signed event/i);
  });

  it('rejects legacy payload-only envelopes', () => {
    const doc = {
      payload: { message: 'inner' },
      metadata: { issuer: 'agent-1', document_id: 'doc-1', created_at: 'now', hash: 'abc' },
      signature: { key_id: 'unknown-key', algorithm: 'Ed25519', signature: 'sig', signed_at: 'now' },
    };
    expect(() => unwrapSignedEvent(doc, {}, TEST_AGENT)).toThrow(/legacy|fully (bound|signed)/i);
  });

  it('throws when no JACS agent is provided', () => {
    const event = { type: 'heartbeat' };
    // @ts-expect-error — verifying runtime guard for missing agent argument
    expect(() => unwrapSignedEvent(event, {})).toThrow(/loaded JACS agent/);
  });

  it('returns data only when the native verifier reports literal verified=true', () => {
    const payload = { hello: 'world' };
    const result = signPayload(payload, TEST_AGENT, TEST_JACS_ID);
    const doc = JSON.parse(result.signed_document);
    const agentID = doc.jacsSignature.agentID;
    const agent = {
      unwrapSignedEventSync: vi.fn(() => JSON.stringify({
        data: payload,
        verified: true,
        status: 'verified',
        signerId: agentID,
        timestamp: doc.jacsSignature.date,
        algorithm: doc.jacsSignature.signingAlgorithm,
        documentId: doc.metadata.document_id,
      })),
    } as unknown as JacsAgent;

    const unwrapped = unwrapSignedEvent(doc, { [agentID]: TEST_PUBLIC_KEY_PEM }, agent);
    expect(unwrapped).toEqual(payload);
    expect((agent as unknown as { unwrapSignedEventSync: ReturnType<typeof vi.fn> }).unwrapSignedEventSync).toHaveBeenCalledOnce();
  });

  it.each([
    ['false', { data: { command: 'run' }, verified: false }],
    ['missing', { data: { command: 'run' } }],
    ['truthy string', { data: { command: 'run' }, verified: 'true' }],
    ['missing data', { verified: true }],
  ])('rejects %s native verification provenance', (_label, nativeResult) => {
    const doc = {
      version: '2.0.0',
      document_type: 'job_response',
      data: { command: 'run' },
      metadata: { issuer: 'server-1', document_id: 'doc-1', created_at: 'now', hash: 'abc' },
      jacsSignature: { agentID: 'server-1', date: 'now', signature: 'sig' },
    };
    const agent = {
      unwrapSignedEventSync: vi.fn(() => JSON.stringify(nativeResult)),
    } as unknown as JacsAgent;

    expect(() => unwrapSignedEvent(doc, { 'server-1': TEST_PUBLIC_KEY_PEM }, agent))
      .toThrow(/verification|verified|data/i);
  });

  it('rejects a JACS binding that lacks the strict native verifier', () => {
    const doc = {
      version: '2.0.0',
      document_type: 'job_response',
      data: { hello: 'world' },
      metadata: { issuer: 'server-1', document_id: 'doc-1', created_at: 'now', hash: 'abc' },
      jacsSignature: { agentID: 'server-1', date: 'now', signature: 'sig' },
    };
    const agent = {} as unknown as JacsAgent;

    expect(() => unwrapSignedEvent(doc, { 'server-1': TEST_PUBLIC_KEY_PEM }, agent))
      .toThrow(/strict|upgrade|unwrapSignedEventSync/i);
  });
});

describe('getServerKeys', () => {
  it('requires HTTPS except for explicit loopback development origins', async () => {
    clearServerKeysCache();
    const ffi = {
      fetchServerKeys: vi.fn(async () => JSON.stringify({
        keys: [{
          signer_id: 'server:v1',
          public_key: TEST_PUBLIC_KEY_PEM,
          is_active: true,
        }],
      })),
    };

    await expect(getServerKeys('http://hai.example', ffi)).rejects.toThrow(/HTTPS|loopback/i);
    expect(ffi.fetchServerKeys).not.toHaveBeenCalled();
    await expect(getServerKeys('http://127.0.0.1:8080', ffi)).resolves.toEqual({
      'server:v1': TEST_PUBLIC_KEY_PEM,
    });
  });

  it('indexes active keys by authenticated JACS signer ID, not storage key ID', async () => {
    clearServerKeysCache();
    const ffi = {
      fetchServerKeys: vi.fn(async () => JSON.stringify({
        keys: [{
          signer_id: 'server-agent-v1:version-7',
          jacs_id: 'server-agent-v1',
          key_id: 'database-row-17',
          public_key: TEST_PUBLIC_KEY_PEM,
          is_active: true,
        }],
      })),
    };

    await expect(getServerKeys('https://hai.example', ffi)).resolves.toEqual({
      'server-agent-v1:version-7': TEST_PUBLIC_KEY_PEM,
    });
  });

  it('rejects conflicting active keys for one signer ID', async () => {
    clearServerKeysCache();
    const ffi = {
      fetchServerKeys: vi.fn(async () => JSON.stringify({
        keys: [
          { signer_id: 'server-agent-v1:v1', jacs_id: 'server-agent-v1', key_id: 'k1', public_key: 'pem-one', is_active: true },
          { signer_id: 'server-agent-v1:v1', jacs_id: 'server-agent-v1', key_id: 'k2', public_key: 'pem-two', is_active: true },
        ],
      })),
    };

    await expect(getServerKeys('https://hai.example', ffi)).rejects.toThrow(/conflicting|duplicate/i);
  });

  it('rejects an endpoint with no usable active signer keys', async () => {
    clearServerKeysCache();
    const ffi = {
      fetchServerKeys: vi.fn(async () => JSON.stringify({
        keys: [{
          signer_id: 'retired-server:v1',
          jacs_id: 'retired-server',
          key_id: 'old-key',
          public_key: TEST_PUBLIC_KEY_PEM,
          is_active: false,
        }],
      })),
    };

    await expect(getServerKeys('https://hai.example', ffi)).rejects.toThrow(/active.*key/i);
  });

  it('never reuses cached keys across different HAI origins', async () => {
    clearServerKeysCache();
    const ffiA = {
      fetchServerKeys: vi.fn(async () => JSON.stringify({
        keys: [{
          signer_id: 'server-a:v1',
          jacs_id: 'server-a',
          key_id: 'a-row',
          public_key: 'pem-a',
          is_active: true,
        }],
      })),
    };
    const ffiB = {
      fetchServerKeys: vi.fn(async () => JSON.stringify({
        keys: [{
          signer_id: 'server-b:v1',
          jacs_id: 'server-b',
          key_id: 'b-row',
          public_key: 'pem-b',
          is_active: true,
        }],
      })),
    };

    await expect(getServerKeys('https://a.example/', ffiA)).resolves.toEqual({
      'server-a:v1': 'pem-a',
    });
    await expect(getServerKeys('https://b.example', ffiB)).resolves.toEqual({
      'server-b:v1': 'pem-b',
    });
    expect(ffiB.fetchServerKeys).toHaveBeenCalledOnce();
  });
});

describe('clearServerKeysCache', () => {
  it('does not throw', () => {
    expect(() => clearServerKeysCache()).not.toThrow();
  });
});
