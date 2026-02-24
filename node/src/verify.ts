// =============================================================================
// Verify link generation
// =============================================================================

/** Maximum total URL length for verify links. */
export const MAX_VERIFY_URL_LEN = 2048;

/** Maximum UTF-8 byte size of the document that can fit in a verify URL. */
export const MAX_VERIFY_DOCUMENT_BYTES = 1515;

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
 * @param document - The JACS document JSON string to embed
 * @param baseUrl - Base URL for the verify page (default: https://hai.ai)
 * @returns Full verify URL
 * @throws If the resulting URL would exceed MAX_VERIFY_URL_LEN
 */
export function generateVerifyLink(
  document: string,
  baseUrl: string = 'https://hai.ai',
  hosted: boolean = false,
): string {
  const base = baseUrl.replace(/\/+$/, '');
  if (hosted) {
    const docId = extractHostedDocumentId(document);
    return `${base}/verify/${docId}`;
  }

  const encoded = Buffer.from(document, 'utf8')
    .toString('base64')
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/g, '');
  const fullUrl = `${base}/jacs/verify?s=${encoded}`;
  if (fullUrl.length > MAX_VERIFY_URL_LEN) {
    throw new Error(
      `Verify URL would exceed max length (${MAX_VERIFY_URL_LEN}). Document size must be at most ${MAX_VERIFY_DOCUMENT_BYTES} UTF-8 bytes.`,
    );
  }
  return fullUrl;
}
