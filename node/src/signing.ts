import { createHash, randomUUID } from 'node:crypto';
import { signString, verifyString } from './crypt.js';

/** JACS document envelope wrapping a payload with metadata and signature. */
export interface JacsDocument {
  payload: unknown;
  metadata: {
    issuer: string;
    document_id: string;
    created_at: string;
    hash: string;
  };
  signature: {
    key_id: string;
    algorithm: string;
    signature: string;
    signed_at: string;
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
 * Unwrap a JACS-signed event, verifying the signature if server keys are provided.
 *
 * If the event is not a JacsDocument (no metadata+signature fields), it is
 * returned as-is. If it is a JacsDocument and a matching server public key
 * is available, the signature is verified before returning the payload.
 *
 * Throws if a known key fails verification (signature mismatch).
 */
export function unwrapSignedEvent(
  eventData: Record<string, unknown>,
  serverPublicKeys: Record<string, string>,
): unknown {
  // Not a JacsDocument -- return unchanged
  if (!eventData.metadata || !eventData.signature) {
    return eventData;
  }

  const doc = eventData as unknown as JacsDocument;
  const keyId = doc.signature.key_id;
  const publicKeyPem = serverPublicKeys[keyId];

  if (publicKeyPem) {
    // The signed content is canonical JSON of {metadata, payload} (without signature)
    const signedContent = canonicalJson({
      metadata: doc.metadata,
      payload: doc.payload,
    });
    const valid = verifyString(publicKeyPem, signedContent, doc.signature.signature);
    if (!valid) {
      throw new Error(`JACS signature verification failed for key_id="${keyId}"`);
    }
  }

  return doc.payload;
}

/**
 * Sign a job response as a JACS document.
 *
 * Creates a JacsDocument with the response as payload, signs it with
 * the agent's Ed25519 private key, and returns the envelope.
 */
export function signResponse(
  jobResponse: unknown,
  privateKeyPem: string,
  jacsId: string,
): { signed_document: string; agent_jacs_id: string } {
  const now = new Date().toISOString();
  const documentId = randomUUID();

  // Build the content to be signed (metadata + payload, no signature yet)
  const contentToSign = {
    metadata: {
      issuer: jacsId,
      document_id: documentId,
      created_at: now,
      hash: '',
    },
    payload: jobResponse,
  };

  // Compute hash of the payload
  const payloadCanonical = canonicalJson(jobResponse);
  const hash = createHash('sha256').update(payloadCanonical).digest('hex');
  contentToSign.metadata.hash = hash;

  // Canonical JSON of {metadata, payload} for signing
  const canonical = canonicalJson(contentToSign);
  const signature = signString(privateKeyPem, canonical);

  const jacsDoc: JacsDocument = {
    payload: jobResponse,
    metadata: contentToSign.metadata,
    signature: {
      key_id: jacsId,
      algorithm: 'Ed25519',
      signature,
      signed_at: now,
    },
  };

  return {
    signed_document: JSON.stringify(jacsDoc),
    agent_jacs_id: jacsId,
  };
}
