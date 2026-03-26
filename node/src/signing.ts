import { randomUUID } from 'node:crypto';
import { JacsAgent, hashString, verifyDocumentStandalone } from '@hai.ai/jacs';
import { HaiError } from './errors.js';

type ResponseSigner = Pick<JacsAgent, 'signStringSync'> & Partial<Pick<JacsAgent, 'signResponseSync'>>;

/** JACS document envelope wrapping data with metadata and jacsSignature. */
export interface JacsDocument {
  version: string;
  document_type: string;
  data: unknown;
  metadata: {
    issuer: string;
    document_id: string;
    created_at: string;
    hash: string;
  };
  jacsSignature: {
    agentID: string;
    date: string;
    signature: string;
  };
}

// Cache for server public keys
let serverKeysCache: Record<string, string> = {};
let cacheExpiry = 0;

/**
 * Fetch the server's public signing keys from the well-known endpoint.
 * Results are cached for 1 hour.
 */
export async function getServerKeys(baseUrl: string, ffi?: { fetchServerKeys(): Promise<string> }): Promise<Record<string, string>> {
  if (Date.now() < cacheExpiry && Object.keys(serverKeysCache).length > 0) {
    return serverKeysCache;
  }

  if (!ffi) {
    throw new Error('FFI client required for getServerKeys (no native HTTP fallback)');
  }

  const raw = await ffi.fetchServerKeys();
  const data = JSON.parse(raw) as { keys?: Array<{ key_id: string; public_key: string }> };

  serverKeysCache = {};
  for (const key of data.keys ?? []) {
    serverKeysCache[key.key_id] = key.public_key;
  }
  cacheExpiry = Date.now() + 3_600_000; // 1 hour
  return serverKeysCache;
}

/** Reset the server keys cache (useful for testing). */
export function clearServerKeysCache(): void {
  serverKeysCache = {};
  cacheExpiry = 0;
}

/**
 * Produce canonical JSON per RFC 8785 (JCS).
 *
 * Delegates to JACS binding-core `canonicalizeJsonSync` when an agent is
 * provided and supports it.  Falls back to deterministic sorted-key
 * JSON.stringify when the agent does not expose the method or is absent.
 *
 * Sorted-key JSON is consistent with the JACS internal canonical form
 * used for signing and is safe as a standalone deterministic serialiser.
 */
export function canonicalJson(obj: unknown, agent?: JacsAgent): string {
  if (agent && 'canonicalizeJsonSync' in agent && typeof (agent as unknown as Record<string, unknown>).canonicalizeJsonSync === 'function') {
    const jsonStr = sortedKeyJson(obj);
    return (agent as unknown as Record<string, unknown> & { canonicalizeJsonSync: (s: string) => string }).canonicalizeJsonSync(jsonStr);
  }

  return sortedKeyJson(obj);
}

/** Sorted-key JSON serialization (deterministic). */
function sortedKeyJson(obj: unknown): string {
  return JSON.stringify(obj, (_key, value: unknown) => {
    if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
      const sorted: Record<string, unknown> = {};
      for (const k of Object.keys(value as Record<string, unknown>).sort()) {
        sorted[k] = (value as Record<string, unknown>)[k];
      }
      return sorted;
    }
    return value;
  });
}

/**
 * Unwrap a JACS-signed event, verifying the signature via JACS if server keys
 * are provided.
 *
 * Delegates to JACS binding-core `unwrapSignedEventSync` when the agent
 * supports it.  Falls back to local unwrap + verification otherwise.
 *
 * Supports both the canonical format (version/document_type/data/metadata/jacsSignature)
 * and the legacy format (payload/metadata/signature). If the event is not a
 * JacsDocument, it is returned as-is.
 *
 * @throws {HaiError} If JACS delegation fails (JACS_OP_FAILED).
 * @throws {HaiError} If a known key fails verification (VERIFICATION_FAILED).
 */
export function unwrapSignedEvent(
  eventData: Record<string, unknown>,
  serverPublicKeys: Record<string, string>,
  agent?: JacsAgent,
): unknown {
  // Try JACS binding-core delegation first
  if (agent && 'unwrapSignedEventSync' in agent && typeof (agent as unknown as Record<string, unknown>).unwrapSignedEventSync === 'function') {
    const eventJson = JSON.stringify(eventData);
    const serverKeysJson = JSON.stringify({
      keys: Object.entries(serverPublicKeys).map(([keyId, publicKey]) => ({
        key_id: keyId,
        public_key: publicKey,
      })),
    });
    try {
      const resultJson = (agent as unknown as Record<string, unknown> & { unwrapSignedEventSync: (e: string, k: string) => string }).unwrapSignedEventSync(eventJson, serverKeysJson);
      const result = JSON.parse(resultJson) as { data: unknown; verified: boolean };
      return result.data ?? eventData;
    } catch (err) {
      throw new HaiError(
        `unwrapSignedEvent failed: ${err instanceof Error ? err.message : String(err)}`,
        undefined,
        undefined,
        'JACS_OP_FAILED',
        'Check JACS installation: npm install @hai.ai/jacs',
      );
    }
  }

  // Local unwrap with JACS verify for signature checks

  // Canonical JacsDocument format: {version, document_type, data, metadata, jacsSignature}
  if (eventData.jacsSignature && eventData.metadata && eventData.data !== undefined) {
    const doc = eventData as unknown as JacsDocument;
    const agentID = doc.jacsSignature.agentID;
    const publicKeyPem = serverPublicKeys[agentID];

    if (publicKeyPem) {
      const signedContent = agent ? canonicalJson(doc.data, agent) : sortedKeyJson(doc.data);
      let valid = false;
      if (agent) {
        valid = agent.verifyStringSync(
          signedContent,
          doc.jacsSignature.signature,
          Buffer.from(publicKeyPem, 'utf-8'),
          'pem',
        );
      } else {
        const standaloneResult = verifyDocumentStandalone(JSON.stringify(eventData));
        valid = standaloneResult.valid;
      }
      if (!valid) {
        throw new HaiError(
          `Signature verification failed for agentID="${agentID}"`,
          undefined,
          undefined,
          'VERIFICATION_FAILED',
          'Verify the public key and algorithm match the signer',
        );
      }
    }

    return doc.data;
  }

  // Legacy format: {payload, metadata, signature}
  if (eventData.metadata && eventData.signature) {
    const payload = eventData.payload;
    const sig = eventData.signature as Record<string, unknown>;
    const keyId = (sig.key_id as string) || '';
    const publicKeyPem = serverPublicKeys[keyId];

    if (publicKeyPem) {
      const signedContent = agent ? canonicalJson({
        metadata: eventData.metadata,
        payload: eventData.payload,
      }, agent) : sortedKeyJson({
        metadata: eventData.metadata,
        payload: eventData.payload,
      });
      let valid = false;
      if (agent) {
        valid = agent.verifyStringSync(
          signedContent,
          (sig.signature as string) || '',
          Buffer.from(publicKeyPem, 'utf-8'),
          'pem',
        );
      } else {
        // Use standalone verification as fallback (matching canonical path)
        try {
          const standaloneResult = verifyDocumentStandalone(JSON.stringify(eventData));
          valid = standaloneResult.valid;
        } catch (verifyErr) {
          // verifyDocumentStandalone may not be available or may fail for this format.
          // Log but don't throw -- the valid=false path below handles it.
          if (typeof console !== 'undefined') console.warn('standalone verify failed:', verifyErr);
        }
      }
      if (!valid) {
        throw new HaiError(
          `Signature verification failed for key_id="${keyId}"`,
          undefined,
          undefined,
          'VERIFICATION_FAILED',
          'Verify the public key and algorithm match the signer',
        );
      }
    }

    return payload;
  }

  // Not a JacsDocument -- return unchanged
  return eventData;
}

/**
 * Sign a job response as a JACS document via JACS core.
 *
 * Delegates envelope construction to JACS binding-core when the agent
 * exposes `signResponseSync`. Otherwise constructs the envelope locally
 * and delegates the signature to JACS `signStringSync`.
 *
 * @throws {HaiError} If the signer does not support signing (JACS_NOT_LOADED).
 */
export function signResponse(
  jobResponse: unknown,
  signer: ResponseSigner,
  jacsId: string,
  canonicalizer?: JacsAgent,
): { signed_document: string; agent_jacs_id: string } {
  if (!('signStringSync' in signer) || typeof signer.signStringSync !== 'function') {
    throw new HaiError(
      'signResponse requires a JACS agent with signStringSync support',
      undefined,
      undefined,
      'JACS_NOT_LOADED',
      "Run 'haiai init' or set JACS_CONFIG_PATH environment variable",
    );
  }

  // Prefer JACS binding delegation (JACS canonicalizes internally via RFC 8785)
  if ('signResponseSync' in signer && typeof (signer as unknown as Record<string, unknown>).signResponseSync === 'function') {
    const rawJson = JSON.stringify(jobResponse);
    const resultJson = (signer as unknown as Record<string, unknown> & { signResponseSync: (p: string) => string }).signResponseSync(rawJson);
    return { signed_document: resultJson, agent_jacs_id: jacsId };
  }

  // Local envelope construction with JACS signStringSync delegation
  const canonicalPayload = canonicalizer ? canonicalJson(jobResponse, canonicalizer) : sortedKeyJson(jobResponse);
  const now = new Date().toISOString();
  const documentId = randomUUID();
  const hash = hashString(canonicalPayload);
  const sortedData: unknown = JSON.parse(canonicalPayload);
  const signature = signer.signStringSync(canonicalPayload);

  const jacsDoc: JacsDocument = {
    version: '1.0.0',
    document_type: 'job_response',
    data: sortedData,
    metadata: {
      issuer: jacsId,
      document_id: documentId,
      created_at: now,
      hash,
    },
    jacsSignature: {
      agentID: jacsId,
      date: now,
      signature,
    },
  };

  return {
    signed_document: JSON.stringify(jacsDoc),
    agent_jacs_id: jacsId,
  };
}
