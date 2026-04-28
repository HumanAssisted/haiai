/**
 * Real-FFI smoke test for haiinpm.
 *
 * Loads the real haiinpm native addon and round-trips `saveMemory("smoke")`
 * against a local `node:http` server. This is the one test that would have
 * caught the regression where the FFI surface declared methods that the
 * native binding never exposed.
 *
 * Skipped cleanly when:
 * - haiinpm is not built / installable (try-import + describe.skip).
 * - The JACS toolchain isn't available to bootstrap a test agent.
 *
 * Per PRD docs/haisdk/JACS_DOCUMENT_STORE_FFI_PRD.md §5.5: real
 * `node:http.createServer` (no fetch-level mock). The traffic is Rust
 * `reqwest` running INSIDE the haiinpm native binding, which only a real
 * listening socket can intercept.
 */

import { describe, expect, it } from 'vitest';
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createRequire } from 'node:module';

const dynamicRequire = createRequire(import.meta.url);

let haiinpm: { HaiClient: new (config: string) => unknown } | null = null;
let loadError: unknown = null;
try {
  haiinpm = dynamicRequire('haiinpm') as { HaiClient: new (config: string) => unknown };
} catch (err) {
  loadError = err;
}

const describeWhenAvailable = haiinpm ? describe : describe.skip;

describeWhenAvailable('haiinpm native FFI smoke test', () => {
  it('saveMemory round-trips through the real native binding', async (ctx) => {
    if (!haiinpm) throw new Error(`haiinpm not loaded: ${String(loadError)}`);

    type CapturedRequest = {
      method: string;
      url: string;
      headers: Record<string, string | string[] | undefined>;
      body: string;
    };
    const captured: CapturedRequest[] = [];

    const server = createServer((req: IncomingMessage, res: ServerResponse) => {
      let buffer = '';
      req.setEncoding('utf-8');
      req.on('data', (chunk) => {
        buffer += chunk;
      });
      req.on('end', () => {
        captured.push({
          method: req.method ?? '',
          url: req.url ?? '',
          headers: req.headers as Record<string, string | string[] | undefined>,
          body: buffer,
        });
        if (req.url === '/api/v1/records' && req.method === 'POST') {
          const payload = JSON.stringify({
            key: 'smoke:v1',
            id: 'smoke',
            version: 'v1',
            jacsType: 'memory',
            jacsVersionDate: '2026-01-01T00:00:00Z',
          });
          res.statusCode = 201;
          res.setHeader('Content-Type', 'application/json');
          res.end(payload);
        } else {
          res.statusCode = 404;
          res.end();
        }
      });
    });

    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
    const address = server.address();
    if (!address || typeof address === 'string') throw new Error('no address');
    const baseUrl = `http://127.0.0.1:${address.port}`;

    const workdir = mkdtempSync(join(tmpdir(), 'haisdk-smoke-'));
    try {
      // Resolve the JACS agent config. Two paths:
      // 1. Pre-baked agent dir via `JACS_SMOKE_AGENT_DIR` (preferred for CI —
      //    Issue 003). CI bootstraps the agent once with `haiai init` and
      //    shares it across all three smoke tests.
      // 2. In-process bootstrap via `@hai.ai/jacs` (for local dev). Skips
      //    cleanly via `ctx.skip()` when the JACS Node bindings aren't
      //    installed (Issue 002 — a bare `return` would mark the test as
      //    PASSED with zero assertions).
      let configPath: string | null = null;

      const prebakedDir = process.env.JACS_SMOKE_AGENT_DIR;
      if (prebakedDir) {
        const prebaked = join(prebakedDir, 'jacs.config.json');
        try {
          // Statically check the file exists.
          const fs = await import('node:fs');
          if (fs.existsSync(prebaked)) {
            configPath = prebaked;
          } else {
            // eslint-disable-next-line no-console
            console.warn(
              `smoke test skipped: JACS_SMOKE_AGENT_DIR=${prebakedDir} but jacs.config.json not found`,
            );
            ctx.skip();
            return;
          }
        } catch (err) {
          // eslint-disable-next-line no-console
          console.warn(`smoke test skipped: cannot read pre-baked agent (${String(err)})`);
          ctx.skip();
          return;
        }
      } else {
        try {
          const jacs = dynamicRequire('@hai.ai/jacs') as {
            JacsAgent: new () => { createAgentSync: (params: string) => string };
          };
          const agent = new jacs.JacsAgent();
          const params = JSON.stringify({
            name: 'smoke-agent',
            password: 'smoke-password',
            dataDirectory: workdir,
            keyDirectory: workdir,
            configPath: join(workdir, 'jacs.config.json'),
          });
          const resultJson = agent.createAgentSync(params);
          const result = JSON.parse(resultJson) as { config_path?: string };
          configPath = result.config_path ?? join(workdir, 'jacs.config.json');
        } catch (err) {
          // Skip rather than fail when JACS isn't installed in this environment.
          // The smoke test is meant to be opt-in; the expensive part (agent
          // creation) is not in scope when the toolchain is missing.
          //
          // Use vitest's `ctx.skip()` so the test is reported as SKIPPED, not
          // PASSED — a bare `return` here previously caused vitest to record a
          // passing test with zero assertions (Issue 002).
          // eslint-disable-next-line no-console
          console.warn(`smoke test skipped: cannot bootstrap JACS agent (${String(err)})`);
          ctx.skip();
          return;
        }
      }

      const ffiConfig = JSON.stringify({
        base_url: baseUrl,
        jacs_config_path: configPath,
        client_type: 'node',
        timeout_secs: 5,
        max_retries: 0,
      });

      const HaiClientCtor = haiinpm.HaiClient;
      const client = new HaiClientCtor(ffiConfig) as {
        saveMemory: (content: string | null) => Promise<string>;
      };
      const key = await client.saveMemory('smoke-content');
      expect(key).toBe('smoke:v1');

      expect(captured).toHaveLength(1);
      expect(captured[0].url).toBe('/api/v1/records');
      expect(captured[0].method).toBe('POST');
      expect(String(captured[0].headers['content-type'])).toContain('application/json');
      expect(captured[0].body).toContain('"jacsType":"memory"');
    } finally {
      server.close();
      rmSync(workdir, { recursive: true, force: true });
    }
  }, 30_000);
});
