import type { HaiEvent } from './types.js';

/**
 * Parsed SSE fields accumulated between blank lines.
 */
interface SseFields {
  event: string;
  data: string;
  id: string | undefined;
}

/**
 * Parse an SSE stream (ReadableStream<Uint8Array>) into an async generator of HaiEvents.
 *
 * Implements the SSE protocol: events are delimited by blank lines,
 * fields are "event:", "data:", and "id:".
 */
export async function* parseSseStream(
  body: ReadableStream<Uint8Array>,
): AsyncGenerator<HaiEvent> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  let fields: SseFields = { event: '', data: '', id: undefined };

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split('\n');
      buffer = lines.pop() ?? '';

      for (const line of lines) {
        if (line.startsWith('event:')) {
          fields.event = line.slice(6).trim();
        } else if (line.startsWith('data:')) {
          fields.data += (fields.data ? '\n' : '') + line.slice(5).trim();
        } else if (line.startsWith('id:')) {
          fields.id = line.slice(3).trim();
        } else if (line === '' || line === '\r') {
          // Blank line = end of event
          if (fields.data) {
            const event = buildEvent(fields);
            if (event) yield event;
          }
          fields = { event: '', data: '', id: undefined };
        }
        // Lines starting with ':' are comments, ignored
      }
    }

    // Flush any remaining event
    if (fields.data) {
      const event = buildEvent(fields);
      if (event) yield event;
    }
  } finally {
    reader.releaseLock();
  }
}

function buildEvent(fields: SseFields): HaiEvent | null {
  if (!fields.event && !fields.data) return null;

  let parsed: unknown;
  try {
    parsed = JSON.parse(fields.data);
  } catch {
    parsed = fields.data;
  }

  const eventType = fields.event || (
    typeof parsed === 'object' && parsed !== null
      ? ((parsed as Record<string, unknown>).type as string) || 'message'
      : 'message'
  );

  return {
    eventType,
    data: parsed,
    id: fields.id,
    raw: fields.data,
  };
}
