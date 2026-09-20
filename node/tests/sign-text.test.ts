/**
 * Per-language sign_text / verify_text tests for the Node SDK (Issue 002).
 *
 * Mirrors `python/tests/test_sign_text.py`. Exercises the full Node SDK path
 * end-to-end via haiinpm native FFI -> binding-core -> JACS.
 *
 * Skipped when haiinpm is missing or the native binding predates TASK_008.
 */

import { afterAll, beforeAll, describe, it, expect } from 'vitest';
import { createRequire } from 'node:module';
import {
  stageSignedMediaVerifier,
  VERIFIER_AGENT_PASSWORD,
} from './signed-verifier-fixture.js';
import {
  mkdtempSync,
  realpathSync,
  writeFileSync,
  rmSync,
  existsSync,
} from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const __filenameLocal = fileURLToPath(import.meta.url);
const __dirnameLocal = dirname(__filenameLocal);

const REPO_ROOT = resolve(__dirnameLocal, '../..');

process.env.JACS_PRIVATE_KEY_PASSWORD = VERIFIER_AGENT_PASSWORD;

interface NativeHaiClientCtor {
  new (configJson: string): {
    signText?: (path: string, optsJson: string) => Promise<string>;
    verifyText?: (path: string, optsJson: string) => Promise<string>;
  };
}

interface HaiinpmModule {
  HaiClient: NativeHaiClientCtor;
}

function loadHaiinpm(): HaiinpmModule | null {
  try {
    const dynamicRequire = createRequire(__filenameLocal);
    return dynamicRequire('haiinpm') as HaiinpmModule;
  } catch {
    return null;
  }
}

function probeMediaSupport(): boolean {
  const m = loadHaiinpm();
  if (!m) return false;
  try {
    const c = new m.HaiClient('{"jacs_id":"x"}');
    return typeof c.signText === 'function' && typeof c.verifyText === 'function';
  } catch {
    return false;
  }
}

const MEDIA_SUPPORTED = probeMediaSupport();
const SKIP_REASON =
  'Installed haiinpm native binding does not expose the Layer-8 media methods. ' +
  'Rebuild via `cargo build -p haiinpm --release`.';

interface FFIClient {
  signText: (path: string, optsJson: string) => Promise<string>;
  verifyText: (path: string, optsJson: string) => Promise<string>;
}

function buildFFIClient(): { client: FFIClient; tmpDir: string } {
  const haiinpm = loadHaiinpm();
  if (!haiinpm) throw new Error('haiinpm not loadable');
  process.env.JACS_PRIVATE_KEY_PASSWORD = VERIFIER_AGENT_PASSWORD;

  const staged = stageSignedMediaVerifier(REPO_ROOT, 'haiai-sign-text-');
  const ffiConfig = JSON.stringify({
    jacs_id: staged.jacsId,
    agent_name: 'MediaFixtureVerifier',
    agent_version: '1.0.0',
    key_dir: staged.keyDir,
    jacs_config_path: staged.configPath,
    base_url: 'http://localhost:1',
  });
  const native = new haiinpm.HaiClient(ffiConfig);
  return {
    client: native as unknown as FFIClient,
    tmpDir: staged.tmpDir,
  };
}

let ffiClient: FFIClient | null = null;
let agentTmpDir: string | null = null;
let stageTmpDir: string | null = null;

const describeMaybe = MEDIA_SUPPORTED ? describe : describe.skip;

describeMaybe('Node SDK signing-side parity (sign_text)', () => {
  beforeAll(() => {
    if (!MEDIA_SUPPORTED) return;
    const built = buildFFIClient();
    ffiClient = built.client;
    agentTmpDir = built.tmpDir;
    stageTmpDir = realpathSync(mkdtempSync(join(tmpdir(), 'haiai-sign-text-stage-')));
  });

  afterAll(() => {
    if (agentTmpDir) rmSync(agentTmpDir, { recursive: true, force: true });
    if (stageTmpDir) rmSync(stageTmpDir, { recursive: true, force: true });
  });

  if (!MEDIA_SUPPORTED) {
    it.skip(SKIP_REASON, () => {});
    return;
  }

  it('sign_text round-trip: sign + verify a markdown file', async () => {
    const path = join(stageTmpDir!, 'hello.md');
    writeFileSync(path, '# Hello\n');

    const outcomeJson = JSON.parse(await ffiClient!.signText(path, '{}'));
    expect(outcomeJson.signers_added).toBe(1);

    const verifyJson = JSON.parse(await ffiClient!.verifyText(path, '{}'));
    expect(verifyJson.status).toBe('signed');
    expect(verifyJson.signatures).toHaveLength(1);
    expect((verifyJson.signatures as Array<Record<string, unknown>>)[0].status).toBe('valid');
  });

  it('sign_text default backup creates .bak file', async () => {
    const path = join(stageTmpDir!, 'with-bak.md');
    writeFileSync(path, '# original\n');

    await ffiClient!.signText(path, '{}');
    expect(existsSync(`${path}.bak`)).toBe(true);
  });

  it('sign_text with backup=false skips .bak file', async () => {
    const path = join(stageTmpDir!, 'no-bak.md');
    writeFileSync(path, '# no backup\n');

    await ffiClient!.signText(path, JSON.stringify({ backup: false }));
    expect(existsSync(`${path}.bak`)).toBe(false);
  });

  it('verify_text strict mode on missing signature signals failure', async () => {
    const path = join(stageTmpDir!, 'unsigned.md');
    writeFileSync(path, '# untouched\n');

    // Strict mode raises through the FFI as a JACS error.
    await expect(
      ffiClient!.verifyText(path, JSON.stringify({ strict: true })),
    ).rejects.toBeTruthy();
  });
});
