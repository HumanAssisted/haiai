// =============================================================================
// Verify link generation
// =============================================================================

/** Maximum total URL length for verify links. */
export const MAX_VERIFY_URL_LEN = 2048;

/** Maximum UTF-8 byte size of the document that can fit in a verify URL. */
export const MAX_VERIFY_DOCUMENT_BYTES = 1515;

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
): string {
  const base = baseUrl.replace(/\/+$/, '');
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
