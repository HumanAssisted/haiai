// =============================================================================
// Verify link generation
// =============================================================================

import type { JacsAgent } from '@hai.ai/jacs';
import { DEFAULT_BASE_URL } from './client.js';
import { HaiError } from './errors.js';

/** Maximum total URL length for verify links. */
export const MAX_VERIFY_URL_LEN = 2048;

/** Maximum UTF-8 byte size of the document that can fit in a verify URL. */
export const MAX_VERIFY_DOCUMENT_BYTES = 1515;

/**
 * URL-safe base64 encoding for verification payloads.
 *
 * Delegates to JACS binding-core `encodeVerifyPayloadSync` when an agent
 * is provided and supports it. Falls back to local base64url encoding
 * otherwise (base64url is deterministic encoding, not cryptography).
 */
function encodeVerifyPayload(document: string, agent?: JacsAgent): string {
  if (agent && 'encodeVerifyPayloadSync' in agent && typeof (agent as unknown as Record<string, unknown>).encodeVerifyPayloadSync === 'function') {
    return (agent as unknown as Record<string, unknown> & { encodeVerifyPayloadSync: (d: string) => string }).encodeVerifyPayloadSync(document);
  }

  // Local base64url encoding (no padding) -- consistent with JACS Rust
  return Buffer.from(document, 'utf8')
    .toString('base64')
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/g, '');
}

function extractHostedDocumentId(document: string): string {
  let parsed: Record<string, unknown>;
  try {
    parsed = JSON.parse(document) as Record<string, unknown>;
  } catch {
    throw new Error(
      "Cannot generate hosted verify link: no document ID found in document. " +
      "Document must contain 'jacsDocumentId', 'document_id', or 'id' field.",
    );
  }

  const docId = (parsed.jacsDocumentId ?? parsed.document_id ?? parsed.id ?? '') as string;
  if (!docId) {
    throw new Error(
      "Cannot generate hosted verify link: no document ID found in document. " +
      "Document must contain 'jacsDocumentId', 'document_id', or 'id' field.",
    );
  }
  return docId;
}

/**
 * Generate a verification link that embeds a JACS document as a URL-safe
 * base64 query parameter.
 *
 * Delegates base64url encoding to JACS binding-core when an agent is
 * provided. Falls back to local encoding otherwise.
 *
 * TODO: This link cannot be embedded in the email it verifies — the signed body would need to
 * contain its own base64 encoding (chicken-and-egg), and hosting the content behind a token
 * creates a public access path to private messages. Per-message verification is therefore
 * recipient-initiated: paste the raw email at /verify.
 *
 * @param document - The JACS document JSON string to embed
 * @param baseUrl - Base URL for the verify page (default: https://hai.ai)
 * @param hosted - If true, generate a hosted verify link using the document ID
 * @param agent - Optional JACS agent for delegated encoding
 * @returns Full verify URL
 * @throws If the resulting URL would exceed MAX_VERIFY_URL_LEN
 */
export function generateVerifyLink(
  document: string,
  baseUrl: string = DEFAULT_BASE_URL,
  hosted: boolean = false,
  agent?: JacsAgent,
): string {
  const base = baseUrl.replace(/\/+$/, '');
  if (hosted) {
    const docId = extractHostedDocumentId(document);
    return `${base}/verify/${docId}`;
  }

  const encoded = encodeVerifyPayload(document, agent);
  const fullUrl = `${base}/jacs/verify?s=${encoded}`;
  if (fullUrl.length > MAX_VERIFY_URL_LEN) {
    throw new Error(
      `Verify URL would exceed max length (${MAX_VERIFY_URL_LEN}). Document size must be at most ${MAX_VERIFY_DOCUMENT_BYTES} UTF-8 bytes.`,
    );
  }
  return fullUrl;
}
