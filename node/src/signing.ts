import { randomUUID } from 'node:crypto';
import { JacsAgent, hashString, verifyDocumentStandalone } from '@hai.ai/jacs';

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
export async function getServerKeys(baseUrl: string): Promise<Record<string, string>> {
  if (Date.now() < cacheExpiry && Object.keys(serverKeysCache).length > 0) {
    return serverKeysCache;
  }
  const resp = await fetch(`${baseUrl.replace(/\/+$/, '')}/.well-known/hai-keys.json`);
  if (!resp.ok) {
    throw new Error(`Failed to fetch server keys (${resp.status})`);
  }
  const data = (await resp.json()) as { keys?: Array<{ key_id: string; public_key: string }> };
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
 * provided. Falls back to local sorted-key JSON.stringify for environments
 * where the JACS native module is not loaded.
 */
export function canonicalJson(obj: unknown, agent?: JacsAgent): string {
  if (agent && 'canonicalizeJsonSync' in agent && typeof (agent as Record<string, unknown>).canonicalizeJsonSync === 'function') {
    try {
      const jsonStr = canonicalJsonLocal(obj);
      return (agent as Record<string, unknown> & { canonicalizeJsonSync: (s: string) => string }).canonicalizeJsonSync(jsonStr);
    } catch {
      // Fall through to local implementation
    }
  }
  return canonicalJsonLocal(obj);
}

/** Local sorted-key JSON canonicalization (fallback). */
function canonicalJsonLocal(obj: unknown): string {
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
 * supports it. Falls back to local unwrap + verification otherwise.
 *
 * Supports both the canonical format (version/document_type/data/metadata/jacsSignature)
 * and the legacy format (payload/metadata/signature). If the event is not a
 * JacsDocument, it is returned as-is.
 *
 * Throws if a known key fails verification (signature mismatch).
 */
export function unwrapSignedEvent(
  eventData: Record<string, unknown>,
  serverPublicKeys: Record<string, string>,
  agent?: JacsAgent,
): unknown {
  // Try JACS binding-core delegation first
  if (agent && 'unwrapSignedEventSync' in agent && typeof (agent as Record<string, unknown>).unwrapSignedEventSync === 'function') {
    try {
      const eventJson = JSON.stringify(eventData);
      const serverKeysJson = JSON.stringify({
        keys: Object.entries(serverPublicKeys).map(([keyId, publicKey]) => ({
          key_id: keyId,
          public_key: publicKey,
        })),
      });
      const resultJson = (agent as Record<string, unknown> & { unwrapSignedEventSync: (e: string, k: string) => string }).unwrapSignedEventSync(eventJson, serverKeysJson);
      const result = JSON.parse(resultJson) as { data: unknown; verified: boolean };
      return result.data ?? eventData;
    } catch {
      // Fall through to local implementation
    }
  }

  // Fallback: local unwrap with JACS verify for signature checks

  // Canonical JacsDocument format: {version, document_type, data, metadata, jacsSignature}
  if (eventData.jacsSignature && eventData.metadata && eventData.data !== undefined) {
    const doc = eventData as unknown as JacsDocument;
    const agentID = doc.jacsSignature.agentID;
    const publicKeyPem = serverPublicKeys[agentID];

    if (publicKeyPem) {
      const signedContent = canonicalJson(doc.data, agent);
      let valid = false;
      if (agent) {
        try {
          valid = agent.verifyStringSync(
            signedContent,
            doc.jacsSignature.signature,
            Buffer.from(publicKeyPem, 'utf-8'),
            'pem',
          );
        } catch {
          valid = false;
        }
      } else {
        // Attempt standalone verification via JACS
        try {
          const standaloneResult = verifyDocumentStandalone(JSON.stringify(eventData));
          valid = standaloneResult.valid;
        } catch {
          valid = false;
        }
      }
      if (!valid) {
        throw new Error(`JACS signature verification failed for agentID="${agentID}"`);
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
      const signedContent = canonicalJson({
        metadata: eventData.metadata,
        payload: eventData.payload,
      }, agent);
      let valid = false;
      if (agent) {
        try {
          valid = agent.verifyStringSync(
            signedContent,
            (sig.signature as string) || '',
            Buffer.from(publicKeyPem, 'utf-8'),
            'pem',
          );
        } catch {
          valid = false;
        }
      }
      if (!valid) {
        throw new Error(`JACS signature verification failed for key_id="${keyId}"`);
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
 * exposes `signResponseSync`. Falls back to local construction for agents
 * that only provide `signStringSync` (e.g. test mocks).
 */
export function signResponse(
  jobResponse: unknown,
  agent: JacsAgent,
  jacsId: string,
): { signed_document: string; agent_jacs_id: string } {
  const canonicalPayload = canonicalJson(jobResponse, agent);

  // Prefer JACS binding delegation (centralizes envelope format in jacs::protocol)
  if ('signResponseSync' in agent && typeof (agent as Record<string, unknown>).signResponseSync === 'function') {
    const resultJson = (agent as Record<string, unknown> & { signResponseSync: (p: string) => string }).signResponseSync(canonicalPayload);
    return { signed_document: resultJson, agent_jacs_id: jacsId };
  }

  // Fallback for agents without signResponseSync
  const now = new Date().toISOString();
  const documentId = randomUUID();
  const hash = hashString(canonicalPayload);
  const sortedData: unknown = JSON.parse(canonicalPayload);
  const signature = agent.signStringSync(canonicalPayload);

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
