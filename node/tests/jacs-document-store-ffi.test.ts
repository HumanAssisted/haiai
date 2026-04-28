/**
 * Issue 025 — Node FFI tests for the 7 D5/D9 JACS Document Store methods.
 *
 * Exercises every D5 (saveMemory / saveSoul / getMemory / getSoul) and D9
 * (storeTextFile / storeImageFile / getRecordBytes) wrapper through the
 * FFIClientAdapter using the createMockFFI test infrastructure. The fixture
 * file `fixtures/ffi_method_parity.json` declares these methods in the
 * `jacs_document_store` group; this test file pins their wire shape
 * (argument names, return types, error mapping) at the adapter boundary.
 *
 * Mock-only: these tests do NOT load the haiinpm native library. The full
 * HTTP round-trip is exercised by `haisdk/rust/haiai/tests/jacs_remote_integration.rs`
 * (`--ignored` against a live hosted stack).
 */

import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { createMockFFI } from './ffi-mock.js';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function loadParityFixture(): {
  methods: Record<string, Array<{ name: string; args: string[]; returns: string }>>;
} {
  const fixturePath = resolve(__dirname, '../../fixtures/ffi_method_parity.json');
  return JSON.parse(readFileSync(fixturePath, 'utf-8'));
}

// ---------------------------------------------------------------------------
// D5 — MEMORY / SOUL wrappers
// ---------------------------------------------------------------------------

describe('D5 MEMORY / SOUL wrappers (Issue 025)', () => {
  it('saveMemory passes content through to native', async () => {
    const ffi = createMockFFI({
      saveMemory: async (content) => {
        expect(content).toBe('# MEMORY.md\n\nproject: foo');
        return 'mem-id:v1';
      },
    });
    const key = await ffi.saveMemory('# MEMORY.md\n\nproject: foo');
    expect(key).toBe('mem-id:v1');
  });

  it('saveMemory accepts null for default-file mode', async () => {
    let captured: string | null | undefined = 'unset';
    const ffi = createMockFFI({
      saveMemory: async (content) => {
        captured = content;
        return 'mem-id:v2';
      },
    });
    const key = await ffi.saveMemory(null);
    expect(key).toBe('mem-id:v2');
    expect(captured).toBeNull();
  });

  it('saveSoul passes content through to native', async () => {
    const ffi = createMockFFI({
      saveSoul: async (content) => {
        expect(content).toBe('# SOUL.md\n\nvoice: terse');
        return 'soul-id:v1';
      },
    });
    const key = await ffi.saveSoul('# SOUL.md\n\nvoice: terse');
    expect(key).toBe('soul-id:v1');
  });

  it('getMemory returns envelope JSON when present', async () => {
    const envelope = JSON.stringify({
      jacsId: 'mem-1',
      jacsType: 'memory',
      body: 'x',
    });
    const ffi = createMockFFI({
      getMemory: async () => envelope,
    });
    const out = await ffi.getMemory();
    expect(out).toBe(envelope);
  });

  it('getMemory returns null when no record exists', async () => {
    const ffi = createMockFFI({
      getMemory: async () => null,
    });
    const out = await ffi.getMemory();
    expect(out).toBeNull();
  });

  it('getSoul returns envelope JSON when present', async () => {
    const envelope = JSON.stringify({ jacsId: 'soul-1', jacsType: 'soul' });
    const ffi = createMockFFI({
      getSoul: async () => envelope,
    });
    const out = await ffi.getSoul();
    expect(out).toBe(envelope);
  });
});

// ---------------------------------------------------------------------------
// D9 — typed-content helpers
// ---------------------------------------------------------------------------

describe('D9 typed-content helpers (Issue 025)', () => {
  it('storeTextFile passes path through to native', async () => {
    const ffi = createMockFFI({
      storeTextFile: async (path) => {
        expect(path).toBe('/tmp/signed.md');
        return 'txt-id:v1';
      },
    });
    const key = await ffi.storeTextFile('/tmp/signed.md');
    expect(key).toBe('txt-id:v1');
  });

  it('storeImageFile passes path through to native', async () => {
    const ffi = createMockFFI({
      storeImageFile: async (path) => {
        expect(path).toBe('/tmp/signed.png');
        return 'png-id:v1';
      },
    });
    const key = await ffi.storeImageFile('/tmp/signed.png');
    expect(key).toBe('png-id:v1');
  });

  it('getRecordBytes returns Uint8Array', async () => {
    const pngMagic = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
    const ffi = createMockFFI({
      getRecordBytes: async (key) => {
        expect(key).toBe('png-id:v1');
        return pngMagic;
      },
    });
    const out = await ffi.getRecordBytes('png-id:v1');
    expect(out).toBeInstanceOf(Uint8Array);
    expect(out).toEqual(pngMagic);
  });
});

// ---------------------------------------------------------------------------
// Generic JACS Document Store CRUD — also part of the 20-method scope
// ---------------------------------------------------------------------------

describe('Generic JACS Document Store CRUD (Issue 025)', () => {
  it('signAndStore passes data JSON through and returns Record', async () => {
    const ffi = createMockFFI({
      signAndStore: async (dataJson) => {
        expect(dataJson).toBe('{"hello":"world"}');
        return { key: 'id1:v1', json: '{}' };
      },
    });
    const out = await ffi.signAndStore('{"hello":"world"}');
    expect(out).toEqual({ key: 'id1:v1', json: '{}' });
  });

  it('searchDocuments forwards limit + offset', async () => {
    const ffi = createMockFFI({
      searchDocuments: async (query, limit, offset) => {
        expect(query).toBe('marker-xyz');
        expect(limit).toBe(10);
        expect(offset).toBe(0);
        return { results: [], total_count: 0 };
      },
    });
    const out = await ffi.searchDocuments('marker-xyz', 10, 0);
    expect(out).toEqual({ results: [], total_count: 0 });
  });

  it('queryByType forwards three args', async () => {
    const ffi = createMockFFI({
      queryByType: async (docType, limit, offset) => {
        expect(docType).toBe('memory');
        expect(limit).toBe(25);
        expect(offset).toBe(0);
        return { items: [] };
      },
    });
    const out = await ffi.queryByType('memory', 25, 0);
    expect(out).toEqual({ items: [] });
  });

  it('storageCapabilities takes no args', async () => {
    const ffi = createMockFFI({
      storageCapabilities: async () => ({ fulltext: true, vector: false }),
    });
    const out = await ffi.storageCapabilities();
    expect(out).toEqual({ fulltext: true, vector: false });
  });

  it('removeDocument returns void', async () => {
    let called = false;
    const ffi = createMockFFI({
      removeDocument: async (key) => {
        expect(key).toBe('id1:v1');
        called = true;
      },
    });
    const out = await ffi.removeDocument('id1:v1');
    expect(out).toBeUndefined();
    expect(called).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// FFI surface area — every D5/D9 method appears in the parity fixture.
// ---------------------------------------------------------------------------

describe('D5/D9 methods are in parity fixture (Issue 025)', () => {
  const D5_METHODS = ['save_memory', 'save_soul', 'get_memory', 'get_soul'];
  const D9_METHODS = ['store_text_file', 'store_image_file', 'get_record_bytes'];

  function fixtureMethodNames(): Set<string> {
    const fixture = loadParityFixture();
    const names = new Set<string>();
    for (const group of Object.values(fixture.methods)) {
      for (const m of group) {
        names.add(m.name);
      }
    }
    return names;
  }

  it('all D5 methods appear in parity', () => {
    const parity = fixtureMethodNames();
    for (const name of D5_METHODS) {
      expect(parity.has(name), `missing ${name}`).toBe(true);
    }
  });

  it('all D9 methods appear in parity', () => {
    const parity = fixtureMethodNames();
    for (const name of D9_METHODS) {
      expect(parity.has(name), `missing ${name}`).toBe(true);
    }
  });
});
