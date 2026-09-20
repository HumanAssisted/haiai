import type { JacsAgent } from '@hai.ai/jacs';
import { randomUUID } from 'node:crypto';
import { HaiError } from './errors.js';

type ResponseSigner = Pick<JacsAgent, 'signStringSync'> & Partial<Pick<JacsAgent, 'signResponseSync'>>;

/**
 * A single-use replay nonce for the `JACS` Authorization header.
 *
 * Not a signing primitive — the signature itself is always produced by JACS.
 * This lives here because `scripts/ci/check_no_local_crypto.sh` allows
 * `node:crypto` only in the signing/hash modules, so `client.ts` must not
 * reach for `randomUUID` itself.
 */
export function randomNonce(): string {
  return randomUUID().replace(/-/g, '');
}

/** Domain identifier cryptographically bound into signed HAI job responses. */
export const SIGNED_JOB_RESPONSE_CONTRACT = 'hai.job-response' as const;

/** Current signed HAI job-response payload contract version. */
export const SIGNED_JOB_RESPONSE_VERSION = 2 as const;

/** Payload placed inside the JACS-signed `data` field for a job response. */
export interface SignedJobResponsePayloadV2 {
  contract: typeof SIGNED_JOB_RESPONSE_CONTRACT;
  version: typeof SIGNED_JOB_RESPONSE_VERSION;
  job_id: string;
  response: unknown;
}

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
    signingAlgorithm: string;
    publicKeyHash: string;
    signatureContentVersion: 'jacs-response-v2';
    signature: string;
  };
}

// Cache for server public keys
let serverKeysCache: Record<string, string> = {};
let cacheExpiry = 0;
let serverKeysCacheOrigin = '';

function trustedServerKeyOrigin(baseUrl: string): string {
  let parsed: URL;
  try {
    parsed = new URL(baseUrl);
  } catch (error) {
    throw new HaiError(
      `Invalid HAI server origin: ${error instanceof Error ? error.message : String(error)}`,
      undefined,
      undefined,
      'SERVER_KEY_ORIGIN_INVALID',
      'Configure an absolute HTTPS HAI server URL',
    );
  }
  const hostname = parsed.hostname.replace(/^\[|\]$/g, '').toLowerCase();
  const loopback =
    hostname === 'localhost' ||
    hostname === '::1' ||
    /^127\.(?:\d{1,3}\.){2}\d{1,3}$/.test(hostname);
  const secure = parsed.protocol === 'https:';
  const loopbackHttp = parsed.protocol === 'http:' && loopback;
  if (
    (!secure && !loopbackHttp) ||
    parsed.username !== '' ||
    parsed.password !== '' ||
    parsed.hash !== ''
  ) {
    throw new HaiError(
      'HAI server signing keys require HTTPS (HTTP is allowed only on loopback)',
      undefined,
      undefined,
      'SERVER_KEY_ORIGIN_INVALID',
      'Configure an HTTPS HAI server URL without userinfo or fragments',
    );
  }
  return parsed.origin;
}

/**
 * Fetch the server's public signing keys from the well-known endpoint.
 * Results are cached for 1 hour.
 */
export async function getServerKeys(baseUrl: string, ffi?: { fetchServerKeys(): Promise<string> }): Promise<Record<string, string>> {
  const origin = trustedServerKeyOrigin(baseUrl);
  if (
    serverKeysCacheOrigin === origin &&
    Date.now() < cacheExpiry &&
    Object.keys(serverKeysCache).length > 0
  ) {
    return serverKeysCache;
  }

  if (!ffi) {
    throw new Error('FFI client required for getServerKeys (no native HTTP fallback)');
  }

  const raw = await ffi.fetchServerKeys();
  const data = JSON.parse(raw) as {
    keys?: Array<{
      signer_id?: unknown;
      jacs_id?: unknown;
      key_id?: unknown;
      public_key?: unknown;
      is_active?: unknown;
    }>;
  };

  serverKeysCache = {};
  for (const key of data.keys ?? []) {
    if (key.is_active !== true) continue;
    const signerId = typeof key.signer_id === 'string' && key.signer_id.trim() !== ''
      ? key.signer_id.trim()
      : typeof key.jacs_id === 'string' &&
          key.jacs_id.trim() !== '' &&
          typeof key.key_id === 'string' &&
          key.key_id.startsWith(`${key.jacs_id.trim()}:`)
        ? key.key_id
        : '';
    if (signerId === '') {
      throw new HaiError(
        'HAI server key response contains an active key without an exact JACS signer ID',
        undefined,
        undefined,
        'SERVER_KEY_INVALID',
        'The server must publish signer_id for every active signing key',
      );
    }
    if (typeof key.public_key !== 'string' || key.public_key.trim() === '') {
      throw new HaiError(
        `HAI server key response contains no public key for signer "${signerId}"`,
        undefined,
        undefined,
        'SERVER_KEY_INVALID',
        'The server must publish a non-empty PEM public key',
      );
    }
    const prior = serverKeysCache[signerId];
    if (prior !== undefined && prior !== key.public_key) {
      throw new HaiError(
        `HAI server key response contains conflicting active keys for signer "${signerId}"`,
        undefined,
        undefined,
        'SERVER_KEY_CONFLICT',
        'Retry after the server key registry is consistent',
      );
    }
    serverKeysCache[signerId] = key.public_key;
  }
  if (Object.keys(serverKeysCache).length === 0) {
    throw new HaiError(
      'HAI server key response contains no usable active signing key',
      undefined,
      undefined,
      'SERVER_KEY_UNAVAILABLE',
      'The server must publish its active JACS signing key before streaming events',
    );
  }
  serverKeysCacheOrigin = origin;
  cacheExpiry = Date.now() + 3_600_000; // 1 hour
  return serverKeysCache;
}

/** Reset the server keys cache (useful for testing). */
export function clearServerKeysCache(): void {
  serverKeysCache = {};
  cacheExpiry = 0;
  serverKeysCacheOrigin = '';
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
 * Delegates to JACS binding-core `unwrapSignedEventSync`. There is no
 * payload-only fallback: the native verifier authenticates the complete v2
 * envelope, enforces freshness, and atomically consumes its replay ID before
 * releasing data.
 *
 * The `agent` parameter is REQUIRED — RFC 8785 canonicalization is delegated
 * to JACS with no local fallback.
 *
 * Only the fully-bound canonical format
 * (version/document_type/data/metadata/jacsSignature) is accepted. Plain and
 * legacy payload-only events are rejected.
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

  const hasOwnData = Object.prototype.hasOwnProperty.call(eventData, 'data');
  const looksBoundV2 =
    typeof eventData.version === 'string' &&
    typeof eventData.document_type === 'string' &&
    hasOwnData &&
    eventData.metadata !== null && typeof eventData.metadata === 'object' &&
    eventData.jacsSignature !== null && typeof eventData.jacsSignature === 'object';
  if (!looksBoundV2) {
    const legacy = eventData.payload !== undefined || eventData.signature !== undefined;
    throw new HaiError(
      legacy
        ? 'Legacy payload-only signed events are not accepted by strict verification'
        : 'Event is not a fully bound v2 JACS signed event',
      undefined,
      undefined,
      'VERIFICATION_FAILED',
      'Require the HAI server to emit a fully signed v2 response envelope',
    );
  }

  const native = (agent as unknown as Record<string, unknown>).unwrapSignedEventSync;
  if (typeof native !== 'function') {
    throw new HaiError(
      'Strict event verification requires JACS unwrapSignedEventSync',
      undefined,
      undefined,
      'JACS_TOO_OLD',
      'Upgrade @hai.ai/jacs to 0.11.4 or newer',
    );
  }

  let resultJson: string;
  try {
    resultJson = (native as (eventJson: string, keysJson: string) => string).call(
      agent,
      JSON.stringify(eventData),
      JSON.stringify(serverPublicKeys),
    );
  } catch (err) {
    throw new HaiError(
      `Signed event verification failed: ${err instanceof Error ? err.message : String(err)}`,
      undefined,
      undefined,
      'VERIFICATION_FAILED',
      'Reject the event and verify the server key registry is current',
    );
  }

  let result: Record<string, unknown>;
  try {
    const parsed: unknown = JSON.parse(resultJson);
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
      throw new Error('native result must be an object');
    }
    result = parsed as Record<string, unknown>;
  } catch (err) {
    throw new HaiError(
      `Strict JACS verifier returned malformed JSON: ${err instanceof Error ? err.message : String(err)}`,
      undefined,
      undefined,
      'JACS_CONTRACT_INVALID',
      'Upgrade @hai.ai/jacs and report the malformed native result',
    );
  }

  if (result.verified !== true || !Object.prototype.hasOwnProperty.call(result, 'data')) {
    throw new HaiError(
      'Strict JACS verifier did not return verified data',
      undefined,
      undefined,
      'JACS_CONTRACT_INVALID',
      'Reject the event and upgrade @hai.ai/jacs',
    );
  }
  return result.data;
}

/**
 * Sign arbitrary data as a JACS document via JACS core.
 *
 * Delegates the complete v2 envelope to JACS binding-core. HAIAI must not
 * construct a local v1 envelope: that format signs only the data field and is
 * rejected by HAI's strict complete-envelope verifier.
 *
 * @throws {HaiError} If the signer does not support signing (JACS_NOT_LOADED).
 * @throws {HaiError} If `signer.signResponseSync` is unavailable
 *   (JACS_TOO_OLD), or returns a non-v2 envelope (JACS_CONTRACT_INVALID).
 * @internal Used for non-job documents that retain the generic signing path.
 */
export function signPayload(
  payload: unknown,
  signer: ResponseSigner,
  jacsId: string,
  _canonicalizer?: JacsAgent,
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

  const nativeSignResponse = (signer as unknown as Record<string, unknown>).signResponseSync;
  if (typeof nativeSignResponse !== 'function') {
    throw new HaiError(
      'Strict response signing requires JACS signResponseSync; legacy local v1 envelopes are not accepted',
      undefined,
      undefined,
      'JACS_TOO_OLD',
      'Upgrade @hai.ai/jacs to 0.11.4 or newer',
    );
  }

  const signedDocument = (nativeSignResponse as (payloadJson: string) => string).call(
    signer,
    JSON.stringify(payload),
  );
  assertV2SignedResponseDocument(signedDocument);

  return {
    signed_document: signedDocument,
    agent_jacs_id: jacsId,
  };
}

function assertV2SignedResponseDocument(signedDocument: string): void {
  let document: unknown;
  try {
    document = JSON.parse(signedDocument);
  } catch (error) {
    throw new HaiError(
      `JACS signResponseSync returned malformed JSON: ${error instanceof Error ? error.message : String(error)}`,
      undefined,
      undefined,
      'JACS_CONTRACT_INVALID',
      'Upgrade @hai.ai/jacs to 0.11.4 or newer',
    );
  }

  const value = document as Record<string, unknown> | null;
  const metadata = value?.metadata as Record<string, unknown> | null;
  const signature = value?.jacsSignature as Record<string, unknown> | null;
  const nonEmptyString = (candidate: unknown): candidate is string =>
    typeof candidate === 'string' && candidate.length > 0;
  const valid =
    value !== null &&
    typeof value === 'object' &&
    value.version === '2.0.0' &&
    value.document_type === 'job_response' &&
    Object.prototype.hasOwnProperty.call(value, 'data') &&
    metadata !== null &&
    typeof metadata === 'object' &&
    nonEmptyString(metadata.issuer) &&
    nonEmptyString(metadata.document_id) &&
    nonEmptyString(metadata.created_at) &&
    nonEmptyString(metadata.hash) &&
    signature !== null &&
    typeof signature === 'object' &&
    nonEmptyString(signature.agentID) &&
    nonEmptyString(signature.date) &&
    nonEmptyString(signature.signingAlgorithm) &&
    nonEmptyString(signature.publicKeyHash) &&
    signature.signatureContentVersion === 'jacs-response-v2' &&
    nonEmptyString(signature.signature);

  if (!valid) {
    throw new HaiError(
      'JACS signResponseSync did not return a fully bound v2 response envelope',
      undefined,
      undefined,
      'JACS_CONTRACT_INVALID',
      'Upgrade @hai.ai/jacs to 0.11.4 or newer',
    );
  }
}

/**
 * Sign a version-2 HAI job response, including its job ID in the signed bytes.
 *
 * The server requires the signed `job_id` to equal the HTTP path or WebSocket
 * event job ID. This prevents a valid response from being replayed against a
 * different job owned by the same agent.
 */
export function signResponse(
  jobId: string,
  jobResponse: unknown,
  signer: ResponseSigner,
  jacsId: string,
  canonicalizer?: JacsAgent,
): { signed_document: string; agent_jacs_id: string } {
  if (typeof jobId !== 'string' || jobId.trim().length === 0) {
    throw new HaiError(
      'signResponse requires a non-empty job ID',
      undefined,
      undefined,
      'INVALID_ARGUMENT',
      'Pass the job ID from the HAI job event',
    );
  }
  if (
    typeof jobResponse !== 'object'
    || jobResponse === null
    || Array.isArray(jobResponse)
    || !Object.prototype.hasOwnProperty.call(jobResponse, 'response')
  ) {
    throw new HaiError(
      'signResponse requires a job response object with a response field',
      undefined,
      undefined,
      'INVALID_ARGUMENT',
      'Pass { response: { message, metadata, processing_time_ms } }',
    );
  }

  const signedPayload: SignedJobResponsePayloadV2 = {
    contract: SIGNED_JOB_RESPONSE_CONTRACT,
    version: SIGNED_JOB_RESPONSE_VERSION,
    job_id: jobId,
    response: (jobResponse as { response: unknown }).response,
  };
  return signPayload(signedPayload, signer, jacsId, canonicalizer);
}
