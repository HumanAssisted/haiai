import { randomUUID } from 'node:crypto';
import { JacsAgent, hashString } from '@hai.ai/jacs';
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
 * Produce canonical JSON per RFC 8785 (JCS) via the JACS binding.
 *
 * Delegates to JACS binding-core `canonicalizeJsonSync`. There is no
 * local fallback: sorted-key `JSON.stringify` is NOT byte-equivalent to
 * RFC 8785 (numeric formatting, Unicode escape rules, and float
 * canonicalization all differ), so signatures produced over a fallback
 * string would not verify against JACS-canonicalized input on the verifier
 * side. The agent argument is REQUIRED — pass a loaded `JacsAgent` from
 * `@hai.ai/jacs`.
 *
 * @throws Error if no agent is provided or the agent does not expose
 *   `canonicalizeJsonSync` (upgrade @hai.ai/jacs).
 */
export function canonicalJson(obj: unknown, agent: JacsAgent): string {
  if (!agent) {
    throw new HaiError(
      'canonicalJson requires a loaded JACS agent (RFC 8785 canonicalization is delegated to JACS — no local fallback)',
      undefined,
      undefined,
      'JACS_NOT_LOADED',
      "Run 'haiai init' or set JACS_CONFIG_PATH environment variable",
    );
  }
  const a = agent as unknown as Record<string, unknown>;
  if (typeof a.canonicalizeJsonSync !== 'function') {
    throw new HaiError(
      'Loaded JACS agent does not expose canonicalizeJsonSync — upgrade @hai.ai/jacs to a version that includes it',
      undefined,
      undefined,
      'JACS_TOO_OLD',
      'Upgrade @hai.ai/jacs to a version that exposes canonicalizeJsonSync',
    );
  }
  // Pass a stable JSON serialization to JACS; JACS produces the canonical
  // RFC 8785 bytes. Plain JSON.stringify is sufficient as input — JACS
  // re-canonicalizes regardless of input ordering.
  return (a as { canonicalizeJsonSync: (s: string) => string }).canonicalizeJsonSync(
    JSON.stringify(obj),
  );
}

/**
 * Unwrap a JACS-signed event, verifying the signature via JACS if server keys
 * are provided.
 *
 * Delegates to JACS binding-core `unwrapSignedEventSync` when the agent
 * supports it. Otherwise falls back to local unwrap + agent-side verification
 * via `verifyStringSync` (still routed through JACS canonical bytes).
 *
 * The `agent` parameter is REQUIRED — RFC 8785 canonicalization is delegated
 * to JACS with no local fallback.
 *
 * Supports both the canonical format (version/document_type/data/metadata/jacsSignature)
 * and the legacy format (payload/metadata/signature). If the event is not a
 * JacsDocument, it is returned as-is.
 *
 * @throws {HaiError} If JACS delegation fails (JACS_OP_FAILED).
 * @throws {HaiError} If a known key fails verification (VERIFICATION_FAILED).
 * @throws {HaiError} If `agent` is not provided (JACS_NOT_LOADED).
 */
export function unwrapSignedEvent(
  eventData: Record<string, unknown>,
  serverPublicKeys: Record<string, string>,
  agent: JacsAgent,
): unknown {
  if (!agent) {
    throw new HaiError(
      'unwrapSignedEvent requires a loaded JACS agent (RFC 8785 canonicalization is delegated to JACS — no local fallback)',
      undefined,
      undefined,
      'JACS_NOT_LOADED',
      "Run 'haiai init' or set JACS_CONFIG_PATH environment variable",
    );
  }

  // Try JACS binding-core delegation first — but only when the event itself
  // looks like a JACS-signed document. JACS's unwrap_signed_event expects to
  // receive a signed envelope; passing a plain heartbeat / non-JACS event
  // would be an error. For non-JACS events we fall through to the local
  // detection branch below which returns the event unchanged.
  const looksJacs =
    (eventData.jacsSignature && eventData.metadata && eventData.data !== undefined) ||
    (eventData.metadata && eventData.signature);
  if (looksJacs && 'unwrapSignedEventSync' in agent && typeof (agent as unknown as Record<string, unknown>).unwrapSignedEventSync === 'function') {
    const eventJson = JSON.stringify(eventData);
    // JACS binding-core expects HashMap<String, String> (flat object map of
    // key_id -> public_key_pem), not an array.
    const serverKeysJson = JSON.stringify(serverPublicKeys);
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

  // Local unwrap with JACS verify for signature checks (still uses
  // canonicalJson which delegates RFC 8785 to JACS)

  // Canonical JacsDocument format: {version, document_type, data, metadata, jacsSignature}
  if (eventData.jacsSignature && eventData.metadata && eventData.data !== undefined) {
    const doc = eventData as unknown as JacsDocument;
    const agentID = doc.jacsSignature.agentID;
    const publicKeyPem = serverPublicKeys[agentID];

    if (publicKeyPem) {
      const signedContent = canonicalJson(doc.data, agent);
      const valid = agent.verifyStringSync(
        signedContent,
        doc.jacsSignature.signature,
        Buffer.from(publicKeyPem, 'utf-8'),
        'pem',
      );
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
      const signedContent = canonicalJson(
        {
          metadata: eventData.metadata,
          payload: eventData.payload,
        },
        agent,
      );
      const valid = agent.verifyStringSync(
        signedContent,
        (sig.signature as string) || '',
        Buffer.from(publicKeyPem, 'utf-8'),
        'pem',
      );
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
 * Delegates envelope construction to JACS binding-core when the signer
 * exposes `signResponseSync`. Otherwise constructs the envelope locally
 * with `canonicalJson` (RFC 8785 via JACS) and delegates the signature to
 * JACS `signStringSync`.
 *
 * The local-envelope path REQUIRES `canonicalizer` — RFC 8785 canonicalization
 * is delegated to JACS with no local fallback. If `signer` is itself a full
 * `JacsAgent` (i.e. exposes `canonicalizeJsonSync`), pass it as both
 * arguments.
 *
 * @throws {HaiError} If the signer does not support signing (JACS_NOT_LOADED).
 * @throws {HaiError} If neither `signer.signResponseSync` nor `canonicalizer`
 *   is available for the local-envelope path (JACS_NOT_LOADED).
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

  // Local envelope construction with JACS signStringSync + JACS canonical JSON
  // delegation. canonicalizer is REQUIRED — there is no JS-side RFC 8785
  // fallback. If the signer itself is a JacsAgent, callers can pass it as
  // both signer and canonicalizer.
  const canonicalAgent =
    canonicalizer ??
    (typeof (signer as unknown as Record<string, unknown>).canonicalizeJsonSync === 'function'
      ? (signer as unknown as JacsAgent)
      : undefined);
  if (!canonicalAgent) {
    throw new HaiError(
      'signResponse local-envelope path requires a JacsAgent canonicalizer (RFC 8785 canonicalization is delegated to JACS — no local fallback)',
      undefined,
      undefined,
      'JACS_NOT_LOADED',
      'Pass a loaded JacsAgent as the `canonicalizer` argument, or use a signer that exposes signResponseSync',
    );
  }
  const canonicalPayload = canonicalJson(jobResponse, canonicalAgent);
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
