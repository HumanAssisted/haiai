import { generateKeypair } from '../src/crypt.js';

/** A test keypair generated once for all tests. */
export const TEST_KEYPAIR = generateKeypair();

/** A test JACS ID. */
export const TEST_JACS_ID = 'test-agent-001';

/** A test base URL. */
export const TEST_BASE_URL = 'https://hai.test';

/**
 * Create a mock fetch that returns a canned response.
 * The mock captures the last request for assertions.
 */
export function createMockFetch(response: {
  status?: number;
  body?: unknown;
  headers?: Record<string, string>;
}) {
  const calls: { url: string; init: RequestInit }[] = [];

  const mockFn = async (url: string | URL | Request, init?: RequestInit): Promise<Response> => {
    calls.push({ url: String(url), init: init ?? {} });

    const status = response.status ?? 200;
    const body = response.body !== undefined ? JSON.stringify(response.body) : '';

    return new Response(body, {
      status,
      headers: {
        'Content-Type': 'application/json',
        ...(response.headers ?? {}),
      },
    });
  };

  return { fetch: mockFn as typeof globalThis.fetch, calls };
}

/**
 * Create a mock SSE response body from a list of events.
 */
export function createSseBody(events: Array<{ event?: string; data: string; id?: string }>): ReadableStream<Uint8Array> {
  const encoder = new TextEncoder();
  let sent = false;

  return new ReadableStream({
    pull(controller) {
      if (sent) {
        controller.close();
        return;
      }
      sent = true;

      let text = '';
      for (const evt of events) {
        if (evt.event) text += `event: ${evt.event}\n`;
        if (evt.id) text += `id: ${evt.id}\n`;
        text += `data: ${evt.data}\n\n`;
      }
      controller.enqueue(encoder.encode(text));
    },
  });
}
