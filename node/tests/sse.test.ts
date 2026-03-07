import { describe, it, expect } from 'vitest';
import { parseSseStream } from '../src/sse.js';
import { createSseBody } from './setup.js';

describe('parseSseStream', () => {
  it('parses a single JSON event', async () => {
    const body = createSseBody([
      { event: 'heartbeat', data: '{"timestamp":1234}' },
    ]);

    const events = [];
    for await (const event of parseSseStream(body)) {
      events.push(event);
    }

    expect(events).toHaveLength(1);
    expect(events[0].eventType).toBe('heartbeat');
    expect(events[0].data).toEqual({ timestamp: 1234 });
    expect(events[0].raw).toBe('{"timestamp":1234}');
  });

  it('parses multiple events', async () => {
    const body = createSseBody([
      { event: 'connected', data: '{"agent_id":"a1"}' },
      { event: 'benchmark_job', data: '{"run_id":"r1","scenario":"test"}' },
      { event: 'heartbeat', data: '{"timestamp":999}' },
    ]);

    const events = [];
    for await (const event of parseSseStream(body)) {
      events.push(event);
    }

    expect(events).toHaveLength(3);
    expect(events[0].eventType).toBe('connected');
    expect(events[1].eventType).toBe('benchmark_job');
    expect(events[2].eventType).toBe('heartbeat');
  });

  it('handles non-JSON data gracefully', async () => {
    const body = createSseBody([
      { event: 'info', data: 'plain text message' },
    ]);

    const events = [];
    for await (const event of parseSseStream(body)) {
      events.push(event);
    }

    expect(events).toHaveLength(1);
    expect(events[0].data).toBe('plain text message');
    expect(events[0].raw).toBe('plain text message');
  });

  it('preserves event IDs', async () => {
    const body = createSseBody([
      { event: 'test', data: '{"val":1}', id: 'evt-42' },
    ]);

    const events = [];
    for await (const event of parseSseStream(body)) {
      events.push(event);
    }

    expect(events[0].id).toBe('evt-42');
  });

  it('defaults eventType from JSON type field when no event: line', async () => {
    const body = createSseBody([
      { data: '{"type":"benchmark_job","run_id":"r1"}' },
    ]);

    const events = [];
    for await (const event of parseSseStream(body)) {
      events.push(event);
    }

    expect(events[0].eventType).toBe('benchmark_job');
  });

  it('handles empty stream', async () => {
    const body = new ReadableStream({
      start(controller) {
        controller.close();
      },
    });

    const events = [];
    for await (const event of parseSseStream(body)) {
      events.push(event);
    }

    expect(events).toHaveLength(0);
  });

  it('handles chunked delivery', async () => {
    const encoder = new TextEncoder();
    let chunkIndex = 0;
    const chunks = [
      'event: test\n',
      'data: {"hello"',
      ':"world"}\n\n',
    ];

    const body = new ReadableStream<Uint8Array>({
      pull(controller) {
        if (chunkIndex < chunks.length) {
          controller.enqueue(encoder.encode(chunks[chunkIndex]));
          chunkIndex++;
        } else {
          controller.close();
        }
      },
    });

    const events = [];
    for await (const event of parseSseStream(body)) {
      events.push(event);
    }

    expect(events).toHaveLength(1);
    expect(events[0].eventType).toBe('test');
    expect(events[0].data).toEqual({ hello: 'world' });
  });
});
