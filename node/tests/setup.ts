import { generateKeypair } from '../src/crypt.js';

/** A test keypair generated once for all tests. */
export const TEST_KEYPAIR = generateKeypair();

/** A test JACS ID. */
export const TEST_JACS_ID = 'test-agent-001';

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
