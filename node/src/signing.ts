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
 * Produce canonical JSON for signing: sorted keys, compact separators.
 * Matches the Rust `serde_json::to_string()` with BTreeMap behavior.
 */
export function canonicalJson(obj: unknown): string {
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
  // Canonical JacsDocument format: {version, document_type, data, metadata, jacsSignature}
  if (eventData.jacsSignature && eventData.metadata && eventData.data !== undefined) {
    const doc = eventData as unknown as JacsDocument;
    const agentID = doc.jacsSignature.agentID;
    const publicKeyPem = serverPublicKeys[agentID];

    if (publicKeyPem) {
      const signedContent = canonicalJson(doc.data);
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
      });
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
 * Creates a JacsDocument matching the Python SDK format:
 *   {version, document_type, data, metadata, jacsSignature}
 *
 * The signature is computed over the canonical JSON of the job response
 * payload (matching Python which signs `canonicalize_json(job_response_payload)`).
 */
export function signResponse(
  jobResponse: unknown,
  agent: JacsAgent,
  jacsId: string,
): { signed_document: string; agent_jacs_id: string } {
  const now = new Date().toISOString();
  const documentId = randomUUID();

  // Canonical JSON of the payload for signing and hashing
  const canonicalPayload = canonicalJson(jobResponse);
  const hash = hashString(canonicalPayload);

  // Store data in canonical (sorted-key) form for cross-language compat
  const sortedData: unknown = JSON.parse(canonicalPayload);

  // Sign the canonical payload data via JACS
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
