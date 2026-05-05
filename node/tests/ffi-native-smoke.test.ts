/**
 * Real-FFI smoke tests for haiinpm.
 *
 * Two tests, one per backend, both loading the real haiinpm native addon
 * and exercising `saveMemory("...")` end-to-end:
 *
 * 1. **Remote** (`saveMemory round-trips through the real native binding`)
 *    — hosted production path. Sets `JACS_DEFAULT_STORAGE=remote` so the
 *    FFI builds a `RemoteJacsProvider`, signs locally, POSTs to a
 *    `node:http.createServer` mock, and reads the server-issued key from
 *    the response. Verifies the mock saw a `POST /api/v1/records` with
 *    signed markdown bytes (plaintext plus the JACS signature footer).
 *
 * 2. **Local** (`saveMemory persists locally without HTTP traffic`) — dev
 *    default path (`haiai init` writes `default_storage: "fs"`). Bootstraps
 *    a fresh agent, sets `JACS_DEFAULT_STORAGE=fs`, signs locally, writes to
 *    disk, and returns a client-side `{jacsId}:{jacsVersion}` key. Verifies
 *    the doc round-trips via `getRecordBytes(key)`.
 *
 * Together these cover the only two backends production and dev users
 * actually exercise.
 *
 * Skipped cleanly when:
 * - haiinpm is not built / installable (try-import + describe.skip).
 * - The JACS toolchain isn't available to bootstrap a test agent.
 *
 * Per PRD docs/haiai/JACS_DOCUMENT_STORE_FFI_PRD.md §5.5: real
 * `node:http.createServer` (no fetch-level mock). The traffic is Rust
 * `reqwest` running INSIDE the haiinpm native binding, which only a real
 * listening socket can intercept.
 */

import { afterEach, beforeEach, describe, expect, it, type TaskContext } from 'vitest';
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { spawnSync } from 'node:child_process';
import { accessSync, constants, existsSync, mkdtempSync, realpathSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
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

// `LocalJacsProvider::store_signed_text` returns the key as
// `{jacsId}:{jacsVersion}` where both halves are JACS UUIDs. This regex
// matches that exact shape so the local-path test asserts on the key
// *structure* (not a specific value, which would change every run).
const LOCAL_KEY_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

/**
 * Bootstrap or locate the JACS agent config. Two paths:
 * 1. Pre-baked agent dir via `JACS_SMOKE_AGENT_DIR` (preferred for CI —
 *    `haiai init` writes the agent once and all three smoke tests share it).
 * 2. In-process bootstrap via `@hai.ai/jacs` (local dev). Returns null
 *    when the JACS Node bindings aren't installed; caller should
 *    `ctx.skip()` to mark the test as SKIPPED (not PASSED with zero
 *    assertions, which a bare `return` produces in vitest).
 */
function resolveJacsAgentConfig(workdir: string): string | null {
  workdir = realpathSync(workdir);

  const prebakedDir = process.env.JACS_SMOKE_AGENT_DIR;
  if (prebakedDir) {
    if (!existsSync(prebakedDir)) return null;
    const prebaked = join(realpathSync(prebakedDir), 'jacs.config.json');
    if (existsSync(prebaked)) return prebaked;
    return null;
  }

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
    return result.config_path ?? join(workdir, 'jacs.config.json');
  } catch {
    return null;
  }
}

function canExecute(path: string): boolean {
  try {
    accessSync(path, constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

function locateHaiaiCli(): string | null {
  const explicit = process.env.HAIAI_CLI;
  if (explicit && canExecute(explicit)) return resolve(explicit);

  let dir = dirname(fileURLToPath(import.meta.url));
  for (;;) {
    const candidate = join(
      dir,
      'rust',
      'target',
      'release',
      process.platform === 'win32' ? 'haiai.exe' : 'haiai',
    );
    if (canExecute(candidate)) return candidate;
    if (existsSync(join(dir, '.git'))) break;
    const parent = dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }

  const result = spawnSync('haiai', ['--help'], { stdio: 'ignore' });
  return result.status === 0 ? 'haiai' : null;
}

function resolveFreshJacsAgentConfig(
  workdir: string,
): { configPath?: string; skipReason?: string } {
  workdir = realpathSync(workdir);
  const cli = locateHaiaiCli();
  if (!cli) {
    return { skipReason: 'haiai CLI binary not found; cannot bootstrap a fresh JACS agent' };
  }

  const password =
    process.env._HAISDK_SMOKE_PASSWORD ?? process.env.JACS_PRIVATE_KEY_PASSWORD ?? 'smoke-password';
  process.env.JACS_PRIVATE_KEY_PASSWORD = password;

  const configPath = join(workdir, 'jacs.config.json');
  const result = spawnSync(
    cli,
    [
      'init',
      '--quiet',
      '--name',
      'local-smoke-agent',
      '--register',
      'false',
      '--data-dir',
      join(workdir, 'data'),
      '--key-dir',
      join(workdir, 'keys'),
      '--config-path',
      configPath,
    ],
    {
      env: { ...process.env, JACS_PRIVATE_KEY_PASSWORD: password },
      encoding: 'utf8',
    },
  );

  if (result.status !== 0) {
    return {
      skipReason:
        `haiai init failed (status=${String(result.status)}): ` +
        `stdout=${result.stdout} stderr=${result.stderr}`,
    };
  }
  if (!existsSync(configPath)) {
    return { skipReason: `haiai init succeeded but ${configPath} was not written` };
  }

  return { configPath };
}

describeWhenAvailable('haiinpm native FFI smoke test', () => {
  // Snapshot/restore JACS_DEFAULT_STORAGE around each test so per-test
  // overrides don't leak into sibling tests or the parent process. We use
  // explicit setup rather than vitest's `vi.stubEnv` because process.env
  // is what the Rust side reads — stubEnv operates only on import.meta.env.
  let savedDefaultStorage: string | undefined;
  beforeEach(() => {
    savedDefaultStorage = process.env.JACS_DEFAULT_STORAGE;
  });
  afterEach(() => {
    if (savedDefaultStorage === undefined) {
      delete process.env.JACS_DEFAULT_STORAGE;
    } else {
      process.env.JACS_DEFAULT_STORAGE = savedDefaultStorage;
    }
  });

  it(
    'saveMemory round-trips through the real native binding (remote backend)',
    async (ctx: TaskContext) => {
      if (!haiinpm) throw new Error(`haiinpm not loaded: ${String(loadError)}`);

      // Force remote routing for THIS test. Without this, the FFI's
      // `build_document_provider` falls through to `default_storage: "fs"`
      // (set by `haiai init`), routes to LocalJacsProvider, and never
      // makes the HTTP call this test was written to verify.
      process.env.JACS_DEFAULT_STORAGE = 'remote';

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

          // The FFI's `save_memory(singleton: true)` first issues a GET
          // to /api/v1/records to check for an existing singleton. Reply
          // with an empty `items` list so the caller takes the
          // "no existing → create" branch and proceeds to POST.
          if (req.method === 'GET' && req.url?.startsWith('/api/v1/records')) {
            res.statusCode = 200;
            res.setHeader('Content-Type', 'application/json');
            res.end(JSON.stringify({ items: [], next_cursor: null }));
            return;
          }
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

      const workdir = realpathSync(mkdtempSync(join(tmpdir(), 'haiai-smoke-remote-')));
      try {
        const configPath = resolveJacsAgentConfig(workdir);
        if (!configPath) {
          // eslint-disable-next-line no-console
          console.warn('remote smoke test skipped: cannot resolve JACS agent config');
          ctx.skip();
          return;
        }

        const ffiConfig = JSON.stringify({
          base_url: baseUrl,
          jacs_config_path: configPath,
          jacs_storage_backend: 'remote',
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

        // The FFI does at least: 1 GET (find_document singleton check)
        // + 1 POST (sign+store). Assert the POST is what we expect; the
        // GET count can vary with future routing tweaks.
        const posts = captured.filter((r) => r.method === 'POST');
        expect(posts).toHaveLength(1);
        expect(posts[0].url).toBe('/api/v1/records');
        expect(String(posts[0].headers['content-type'])).toContain('text/markdown');
        expect(posts[0].body).toContain('smoke-content');
        expect(posts[0].body).toContain('-----BEGIN JACS SIGNATURE-----');
      } finally {
        server.close();
        rmSync(workdir, { recursive: true, force: true });
      }
    },
    30_000,
  );

  it(
    'saveMemory persists locally without HTTP traffic (fs backend)',
    async (ctx: TaskContext) => {
      if (!haiinpm) throw new Error(`haiinpm not loaded: ${String(loadError)}`);

      // Force fs routing. The pre-baked smoke agent already defaults to
      // fs, but pinning the env var here makes the test hermetic against
      // future bootstrap changes or a leaking parent-shell env var.
      process.env.JACS_DEFAULT_STORAGE = 'fs';

      const workdir = realpathSync(mkdtempSync(join(tmpdir(), 'haiai-smoke-local-')));
      try {
        const fresh = resolveFreshJacsAgentConfig(workdir);
        if (!fresh.configPath) {
          // eslint-disable-next-line no-console
          console.warn(`local smoke test skipped: ${fresh.skipReason}`);
          ctx.skip();
          return;
        }

        // No mock HTTP server: the local path must not make any network
        // calls, and binding the FFI to an unreachable URL surfaces that
        // invariant if the routing decision ever regresses.
        const ffiConfig = JSON.stringify({
          base_url: 'http://127.0.0.1:1', // unreachable on purpose
          jacs_config_path: fresh.configPath,
          jacs_storage_backend: 'fs',
          client_type: 'node',
          timeout_secs: 5,
          max_retries: 0,
        });

        const HaiClientCtor = haiinpm.HaiClient;
        const client = new HaiClientCtor(ffiConfig) as {
          saveMemory: (content: string | null) => Promise<string>;
          getRecordBytes: (key: string) => Promise<Buffer>;
        };
        const key = await client.saveMemory('local-smoke-content');

        expect(key).toMatch(LOCAL_KEY_PATTERN);

        // Round-trip: fetch the just-stored document by key. The FFI returns
        // the raw bytes of the signed text artifact, which must contain the
        // original plaintext we saved.
        const recordBytes: Buffer = await client.getRecordBytes(key);
        expect(Buffer.isBuffer(recordBytes)).toBe(true);
        expect(recordBytes.toString('utf8')).toContain('local-smoke-content');
      } finally {
        rmSync(workdir, { recursive: true, force: true });
      }
    },
    30_000,
  );
});
