import { describe, it, expect, vi } from 'vitest';
import type { WsLike } from '../src/ws.js';
import { wsRecv, wsEventStream } from '../src/ws.js';
import { HaiConnectionError, WebSocketError } from '../src/errors.js';

/** Create a mock WebSocket that supports on/once/close */
function createMockWs(): WsLike & {
  _trigger(event: string, ...args: unknown[]): void;
  _listeners: Map<string, Array<(...args: unknown[]) => void>>;
} {
  const listeners = new Map<string, Array<(...args: unknown[]) => void>>();

  const ws: WsLike & {
    _trigger(event: string, ...args: unknown[]): void;
    _listeners: Map<string, Array<(...args: unknown[]) => void>>;
  } = {
    readyState: 1, // OPEN
    _listeners: listeners,
    send: vi.fn(),
    close: vi.fn(() => {
      (ws as { readyState: number }).readyState = 3; // CLOSED
    }),
    on(event: string, listener: (...args: unknown[]) => void) {
      if (!listeners.has(event)) listeners.set(event, []);
      listeners.get(event)!.push(listener);
    },
    once(event: string, listener: (...args: unknown[]) => void) {
      if (!listeners.has(event)) listeners.set(event, []);
      const wrapped = (...args: unknown[]) => {
        const idx = listeners.get(event)!.indexOf(wrapped);
        if (idx >= 0) listeners.get(event)!.splice(idx, 1);
        listener(...args);
      };
      listeners.get(event)!.push(wrapped);
    },
    _trigger(event: string, ...args: unknown[]) {
      const handlers = listeners.get(event) ?? [];
      for (const h of [...handlers]) {
        h(...args);
      }
    },
  };

  return ws;
}

/** Helper to wait for microtasks to flush */
const tick = () => new Promise(r => setTimeout(r, 0));

describe('ws', () => {
  describe('wsRecv', () => {
    it('receives and parses JSON message', async () => {
      const ws = createMockWs();
      const promise = wsRecv(ws);

      ws._trigger('message', JSON.stringify({ type: 'connected', status: 'ok' }));

      const result = await promise;
      expect(result).toEqual({ type: 'connected', status: 'ok' });
    });

    it('returns raw string for non-JSON message', async () => {
      const ws = createMockWs();
      const promise = wsRecv(ws);

      ws._trigger('message', 'not-json');

      const result = await promise;
      expect(result).toBe('not-json');
    });

    it('handles Buffer input', async () => {
      const ws = createMockWs();
      const promise = wsRecv(ws);

      ws._trigger('message', Buffer.from('{"key":"value"}'));

      const result = await promise;
      expect(result).toEqual({ key: 'value' });
    });

    it('rejects on close', async () => {
      const ws = createMockWs();
      const promise = wsRecv(ws);

      ws._trigger('close');

      await expect(promise).rejects.toThrow(HaiConnectionError);
    });

    it('rejects on error with WebSocketError', async () => {
      const ws = createMockWs();
      const promise = wsRecv(ws);

      ws._trigger('error', new Error('connection lost'));

      await expect(promise).rejects.toThrow(WebSocketError);
    });
  });

  describe('wsEventStream', () => {
    it('yields events from messages', async () => {
      const ws = createMockWs();
      const stream = wsEventStream(ws);

      // Send messages, then close after a separate tick so generator can yield
      setTimeout(() => {
        ws._trigger('message', JSON.stringify({ type: 'heartbeat', timestamp: 123 }));
        ws._trigger('message', JSON.stringify({ type: 'benchmark_job', run_id: 'r1' }));
        // Close after a separate tick to allow generator to process messages
        setTimeout(() => ws._trigger('close'), 5);
      }, 10);

      const events = [];
      for await (const event of stream) {
        events.push(event);
      }

      expect(events).toHaveLength(2);
      expect(events[0].eventType).toBe('heartbeat');
      expect(events[1].eventType).toBe('benchmark_job');
    });

    it('extracts event_type from data', async () => {
      const ws = createMockWs();
      const stream = wsEventStream(ws);

      setTimeout(() => {
        ws._trigger('message', JSON.stringify({ event_type: 'custom_event', data: 'test' }));
        setTimeout(() => ws._trigger('close'), 5);
      }, 10);

      const events = [];
      for await (const event of stream) {
        events.push(event);
      }

      expect(events[0].eventType).toBe('custom_event');
    });

    it('extracts event id', async () => {
      const ws = createMockWs();
      const stream = wsEventStream(ws);

      setTimeout(() => {
        ws._trigger('message', JSON.stringify({ type: 'test', id: 'evt-42' }));
        setTimeout(() => ws._trigger('close'), 5);
      }, 10);

      const events = [];
      for await (const event of stream) {
        events.push(event);
      }

      expect(events[0].id).toBe('evt-42');
    });

    it('handles non-JSON messages as raw text', async () => {
      const ws = createMockWs();
      const stream = wsEventStream(ws);

      setTimeout(() => {
        ws._trigger('message', 'plain text message');
        setTimeout(() => ws._trigger('close'), 5);
      }, 10);

      const events = [];
      for await (const event of stream) {
        events.push(event);
      }

      expect(events[0].eventType).toBe('message');
      expect(events[0].data).toBe('plain text message');
    });

    it('stores raw text', async () => {
      const ws = createMockWs();
      const stream = wsEventStream(ws);

      const rawJson = JSON.stringify({ type: 'test', data: { x: 1 } });
      setTimeout(() => {
        ws._trigger('message', rawJson);
        setTimeout(() => ws._trigger('close'), 5);
      }, 10);

      const events = [];
      for await (const event of stream) {
        events.push(event);
      }

      expect(events[0].raw).toBe(rawJson);
    });

    it('throws on WebSocket error', async () => {
      const ws = createMockWs();
      const stream = wsEventStream(ws);

      setTimeout(() => {
        ws._trigger('error', new Error('connection reset'));
      }, 10);

      const events = [];
      try {
        for await (const event of stream) {
          events.push(event);
        }
        expect.fail('should have thrown');
      } catch (e) {
        expect(e).toBeInstanceOf(Error);
      }
    });
  });
});
