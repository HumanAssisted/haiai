import type { HaiEvent } from './types.js';
import { HaiConnectionError, WebSocketError } from './errors.js';

/**
 * Minimal interface for a WebSocket connection.
 * Compatible with both the `ws` package and the built-in Node 21+ WebSocket.
 */
export interface WsLike {
  send(data: string): void;
  close(): void;
  readonly readyState: number;
  on?(event: string, listener: (...args: unknown[]) => void): void;
  once?(event: string, listener: (...args: unknown[]) => void): void;
  addEventListener?(event: string, listener: (...args: unknown[]) => void, options?: { once?: boolean }): void;
  removeEventListener?(event: string, listener: (...args: unknown[]) => void): void;
}

const WS_OPEN = 1;

/**
 * Open a WebSocket connection using the `ws` package.
 * Falls back to built-in WebSocket (Node 21+) if ws is not installed.
 */
export function openWebSocket(
  url: string,
  headers: Record<string, string>,
  timeout: number,
): Promise<WsLike> {
  return (async () => {
    // Try ws package first
    try {
      const wsModule = await import('ws');
      const WS = ((wsModule as unknown as { default?: unknown }).default ?? wsModule) as {
        new (wsUrl: string, opts: Record<string, unknown>): WsLike;
      };

      return await new Promise<WsLike>((resolve, reject) => {
        const ws = new WS(url, { headers, handshakeTimeout: timeout });
        ws.on!('open', () => resolve(ws));
        ws.on!('error', (err: unknown) => {
          const msg = err instanceof Error ? err.message : String(err);
          reject(new HaiConnectionError(`WebSocket error: ${msg}`));
        });
      });
    } catch {
      // ws not installed, fall through
    }

    // Built-in WebSocket (Node 21+)
    try {
      return await new Promise<WsLike>((resolve, reject) => {
        const ws = new WebSocket(url) as unknown as WsLike;
        ws.addEventListener!('open', () => resolve(ws));
        ws.addEventListener!('error', (e: unknown) => {
          reject(new HaiConnectionError(`WebSocket error: ${e}`));
        });
      });
    } catch {
      throw new WebSocketError(
        'WebSocket support requires the "ws" package or Node 21+. Install with: npm install ws',
      );
    }
  })();
}

/** Receive a single message from a WebSocket, parsing JSON if possible. */
export function wsRecv(ws: WsLike): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const handler = (data: unknown) => {
      const str = typeof data === 'string'
        ? data
        : data instanceof Buffer
          ? data.toString('utf-8')
          : String(data);
      try {
        resolve(JSON.parse(str));
      } catch {
        resolve(str);
      }
    };

    // ws package uses .on()/.once()
    if (typeof ws.once === 'function') {
      ws.once('message', handler);
      ws.once('close', () => reject(new HaiConnectionError('WebSocket closed')));
      ws.once('error', (err: unknown) => {
        const msg = err instanceof Error ? err.message : String(err);
        reject(new WebSocketError(msg));
      });
      return;
    }

    // Built-in WebSocket uses addEventListener
    if (typeof ws.addEventListener === 'function') {
      const onMessage = (e: unknown) => {
        ws.removeEventListener!('message', onMessage);
        const event = e as { data: unknown };
        handler(event.data);
      };
      ws.addEventListener('message', onMessage);
      ws.addEventListener('close', () => reject(new HaiConnectionError('WebSocket closed')), { once: true });
      ws.addEventListener('error', () => reject(new WebSocketError('WebSocket error')), { once: true });
    }
  });
}

/**
 * Wrap a WebSocket into an async generator of HaiEvents.
 *
 * Messages are parsed as JSON when possible. The event type is extracted
 * from the `type` or `event_type` field in the JSON payload.
 */
export async function* wsEventStream(ws: WsLike): AsyncGenerator<HaiEvent> {
  const messageQueue: HaiEvent[] = [];
  let resolveWait: (() => void) | null = null;
  let closed = false;
  let error: Error | null = null;

  const onMessage = (raw: unknown) => {
    const text = typeof raw === 'string'
      ? raw
      : raw instanceof Buffer
        ? raw.toString('utf-8')
        : String(raw);
    try {
      const parsed = JSON.parse(text) as Record<string, unknown>;
      const eventType = (parsed.type as string) || (parsed.event_type as string) || 'message';
      const eventId = (parsed.id as string) || (parsed.event_id as string) || undefined;

      messageQueue.push({
        eventType,
        data: parsed,
        id: eventId,
        raw: text,
      });
    } catch {
      messageQueue.push({
        eventType: 'message',
        data: text,
        raw: text,
      });
    }

    if (resolveWait) {
      const r = resolveWait;
      resolveWait = null;
      r();
    }
  };

  const onClose = () => {
    closed = true;
    if (resolveWait) {
      const r = resolveWait;
      resolveWait = null;
      r();
    }
  };

  const onError = (err: unknown) => {
    error = err instanceof Error ? err : new Error(String(err));
    closed = true;
    if (resolveWait) {
      const r = resolveWait;
      resolveWait = null;
      r();
    }
  };

  // Attach listeners (ws package)
  if (typeof ws.on === 'function') {
    ws.on('message', onMessage);
    ws.on('close', onClose);
    ws.on('error', onError);
  } else if (typeof ws.addEventListener === 'function') {
    ws.addEventListener('message', (e: unknown) => onMessage((e as { data: unknown }).data));
    ws.addEventListener('close', onClose);
    ws.addEventListener('error', onError);
  }

  try {
    while (!closed) {
      while (messageQueue.length > 0) {
        yield messageQueue.shift()!;
      }
      if (closed) break;
      await new Promise<void>((r) => {
        resolveWait = r;
      });
    }
  } finally {
    if (ws.readyState === WS_OPEN) {
      ws.close();
    }
  }

  if (error) throw error;
}
