/**
 * Per-language signing-side tests for the Node SDK (Issue 002).
 *
 * Mirrors `python/tests/test_sign_image.py` and exercises the Node SDK
 * `HaiClient.signImage` / `verifyImage` / `extractMediaSignature` paths
 * end-to-end (HaiClient -> ffi-client -> haiinpm native -> binding-core
 * -> JACS).
 *
 * Skipped when haiinpm is missing or the native binding predates TASK_008.
 *
 * Runs in vitest's `forks` pool (configured in `vitest.config.ts` via
 * `poolMatchGlobs`) so process.env mutations propagate to the JACS native
 * side.
 */

import { afterAll, beforeAll, describe, it, expect, vi } from 'vitest';
import { createRequire } from 'node:module';
import { HaiClient } from '../src/client.js';
import { generateTestKeypair } from './setup.js';
import { createMockFFI } from './ffi-mock.js';
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  statSync,
  copyFileSync,
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
const JACS_AGENT_DIR = join(REPO_ROOT, 'fixtures', 'jacs-agent');
const SOURCE_PNG_PATH = join(REPO_ROOT, 'fixtures', 'media', '_source', 'source.png');
const FIXTURE_AGENT_PASSWORD = 'secretpassord';

process.env.JACS_PRIVATE_KEY_PASSWORD = FIXTURE_AGENT_PASSWORD;

interface NativeHaiClientCtor {
  new (configJson: string): {
    signImage?: (inPath: string, outPath: string, optsJson: string) => Promise<string>;
    verifyImage?: (path: string, optsJson: string) => Promise<string>;
    extractMediaSignature?: (path: string, optsJson: string) => Promise<string>;
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
    return (
      typeof c.signImage === 'function' &&
      typeof c.verifyImage === 'function' &&
      typeof c.extractMediaSignature === 'function'
    );
  } catch {
    return false;
  }
}

const MEDIA_SUPPORTED = probeMediaSupport();
const SKIP_REASON =
  'Installed haiinpm native binding does not expose the Layer-8 media methods. ' +
  'Rebuild via `cargo build -p haiinpm --release`.';

function copyWithColons(src: string, dst: string): void {
  mkdirSync(dst, { recursive: true });
  for (const name of readdirSync(src)) {
    const newName = name.replace(/_/g, ':');
    const srcPath = join(src, name);
    const dstPath = join(dst, newName);
    if (statSync(srcPath).isDirectory()) {
      copyWithColons(srcPath, dstPath);
    } else {
      copyFileSync(srcPath, dstPath);
    }
  }
}

interface StagedAgent {
  configPath: string;
  tmpDir: string;
}

function stageFixtureAgent(): StagedAgent {
  process.env.JACS_PRIVATE_KEY_PASSWORD = FIXTURE_AGENT_PASSWORD;

  const tmpDir = mkdtempSync(join(tmpdir(), 'haiai-sign-image-'));
  const cfg = JSON.parse(
    readFileSync(join(JACS_AGENT_DIR, 'jacs.config.json'), 'utf-8'),
  ) as Record<string, unknown>;

  const srcKeys = join(JACS_AGENT_DIR, cfg.jacs_key_directory as string);
  const tmpKeys = join(tmpDir, 'keys');
  mkdirSync(tmpKeys, { recursive: true });
  for (const name of readdirSync(srcKeys)) {
    copyFileSync(join(srcKeys, name), join(tmpKeys, name));
  }

  const srcData = join(JACS_AGENT_DIR, cfg.jacs_data_directory as string);
  const tmpData = join(tmpDir, 'data');
  copyWithColons(srcData, tmpData);

  cfg.jacs_data_directory = tmpData;
  cfg.jacs_key_directory = tmpKeys;

  const configPath = join(tmpDir, 'jacs.config.json');
  writeFileSync(configPath, JSON.stringify(cfg, null, 2));
  return { configPath, tmpDir };
}

interface FFIClient {
  signImage: (inPath: string, outPath: string, optsJson: string) => Promise<string>;
  verifyImage: (path: string, optsJson: string) => Promise<string>;
  extractMediaSignature: (path: string, optsJson: string) => Promise<string>;
}

function buildFFIClient(): { client: FFIClient; tmpDir: string } {
  const haiinpm = loadHaiinpm();
  if (!haiinpm) throw new Error('haiinpm not loadable');
  process.env.JACS_PRIVATE_KEY_PASSWORD = FIXTURE_AGENT_PASSWORD;

  const staged = stageFixtureAgent();
  const cfg = JSON.parse(readFileSync(staged.configPath, 'utf-8')) as Record<string, unknown>;
  const ffiConfig = JSON.stringify({
    jacs_id: (cfg.jacs_agent_id_and_version as string).split(':')[0],
    agent_name: 'FixtureAgent',
    agent_version: '1.0.0',
    key_dir: cfg.jacs_key_directory,
    jacs_config_path: staged.configPath,
    base_url: 'http://localhost:1', // never hit; sign_image is local-only
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

describeMaybe('Node SDK signing-side parity (sign_image)', () => {
  beforeAll(() => {
    if (!MEDIA_SUPPORTED) return;
    if (!existsSync(SOURCE_PNG_PATH)) {
      throw new Error(`fixtures source PNG missing: ${SOURCE_PNG_PATH}`);
    }
    const built = buildFFIClient();
    ffiClient = built.client;
    agentTmpDir = built.tmpDir;
    stageTmpDir = mkdtempSync(join(tmpdir(), 'haiai-sign-image-stage-'));
  });

  afterAll(() => {
    if (agentTmpDir) rmSync(agentTmpDir, { recursive: true, force: true });
    if (stageTmpDir) rmSync(stageTmpDir, { recursive: true, force: true });
  });

  if (!MEDIA_SUPPORTED) {
    it.skip(SKIP_REASON, () => {});
    return;
  }

  function stagedPath(name: string): string {
    const target = join(stageTmpDir!, name);
    writeFileSync(target, readFileSync(SOURCE_PNG_PATH));
    return target;
  }

  it('sign_image round-trip: sign + verify a PNG', async () => {
    const inPath = stagedPath('in.png');
    const outPath = join(stageTmpDir!, 'out.png');

    const signedJson = JSON.parse(await ffiClient!.signImage(inPath, outPath, '{}'));
    expect(signedJson.format).toBe('png');
    expect(typeof signedJson.signer_id).toBe('string');
    expect(signedJson.signer_id.length).toBeGreaterThan(0);

    const verifyJson = JSON.parse(await ffiClient!.verifyImage(outPath, '{}'));
    expect(verifyJson.status).toBe('valid');
    expect(verifyJson.signer_id).toBe(signedJson.signer_id);
  });

  it('verify_image tampered returns hash_mismatch', async () => {
    const inPath = stagedPath('in_tamper.png');
    const outPath = join(stageTmpDir!, 'out_tamper.png');
    await ffiClient!.signImage(inPath, outPath, '{}');

    // Flip a byte in the IDAT region after the iTXt signature chunk.
    const buf = readFileSync(outPath);
    const idat = buf.indexOf(Buffer.from('IDAT', 'utf-8'));
    expect(idat).toBeGreaterThan(0);
    buf[idat + 6] ^= 0x01;
    writeFileSync(outPath, buf);

    const verifyJson = JSON.parse(await ffiClient!.verifyImage(outPath, '{}'));
    expect(['hash_mismatch', 'invalid_signature']).toContain(verifyJson.status);
  });

  it('extract_media_signature returns decoded JSON payload', async () => {
    const inPath = stagedPath('in_extract.png');
    const outPath = join(stageTmpDir!, 'out_extract.png');
    await ffiClient!.signImage(inPath, outPath, '{}');

    const env = JSON.parse(await ffiClient!.extractMediaSignature(outPath, '{}'));
    expect(env.present).toBe(true);
    expect(typeof env.payload).toBe('string');
    const inner = JSON.parse(env.payload as string);
    expect(typeof inner).toBe('object');
  });

  it('extract_media_signature raw returns base64url', async () => {
    const inPath = stagedPath('in_raw.png');
    const outPath = join(stageTmpDir!, 'out_raw.png');
    await ffiClient!.signImage(inPath, outPath, '{}');

    const env = JSON.parse(
      await ffiClient!.extractMediaSignature(outPath, JSON.stringify({ raw_payload: true })),
    );
    expect(env.present).toBe(true);
    expect(typeof env.payload).toBe('string');
    expect(env.payload).toMatch(/^[A-Za-z0-9_-]+$/);
  });

  it('extract_media_signature unsigned returns present=false', async () => {
    const path = stagedPath('unsigned.png');
    const env = JSON.parse(await ffiClient!.extractMediaSignature(path, '{}'));
    expect(env.present).toBe(false);
    expect(env.payload).toBeNull();
  });

  it('signImage with noBackup skips bak (Issue 009 parity)', async () => {
    // Issue 009: Node SDK option `noBackup` must propagate to JACS as
    // `backup=false`. Mirrors Go's TestSignImageNoBackupSkipsBak and
    // python's test_sign_image_no_backup_skips_bak.
    const inPath = stagedPath('in_nobak.png');
    const outPath = join(stageTmpDir!, 'out_nobak.png');
    await ffiClient!.signImage(
      inPath,
      outPath,
      JSON.stringify({ backup: false }),
    );
    expect(existsSync(outPath + '.bak')).toBe(false);
  });
});

// =============================================================================
// SDK-level wire option mapping (Issue 009 — verifies the SDK wrapper
// translates `noBackup`/`unsafeBakMode` into the wire JSON `backup`/
// `unsafe_bak_mode` keys consumed by binding-core).
// =============================================================================

describe('signImage SDK option translation (Issue 009)', () => {
  it('noBackup=true maps to wire backup=false', async () => {
    const keypair = generateTestKeypair();
    const client = await HaiClient.fromCredentials('agent-issue-009-1', keypair.privateKeyPem, {
      url: 'https://hai.example',
    });
    const signImageMock = vi.fn(
      async (_in: string, _out: string, opts: Record<string, unknown>) => {
        expect(opts.backup).toBe(false);
        expect(opts.unsafe_bak_mode).toBeUndefined();
        return {
          out_path: _out,
          signer_id: 'agent-issue-009-1',
          format: 'png',
          robust: false,
          backup_path: null,
        };
      },
    );
    client._setFFIAdapter(createMockFFI({ signImage: signImageMock }));
    await client.signImage('/x/in.png', '/x/out.png', { noBackup: true });
    expect(signImageMock).toHaveBeenCalledTimes(1);
  });

  it('default (no noBackup) maps to wire backup=true', async () => {
    const keypair = generateTestKeypair();
    const client = await HaiClient.fromCredentials('agent-issue-009-2', keypair.privateKeyPem, {
      url: 'https://hai.example',
    });
    const signImageMock = vi.fn(
      async (_in: string, _out: string, opts: Record<string, unknown>) => {
        expect(opts.backup).toBe(true);
        return {
          out_path: _out,
          signer_id: 'agent-issue-009-2',
          format: 'png',
          robust: false,
          backup_path: '/x/out.png.bak',
        };
      },
    );
    client._setFFIAdapter(createMockFFI({ signImage: signImageMock }));
    await client.signImage('/x/in.png', '/x/out.png');
    expect(signImageMock).toHaveBeenCalledTimes(1);
  });

  it('unsafeBakMode=0o644 maps to wire unsafe_bak_mode=0o644', async () => {
    const keypair = generateTestKeypair();
    const client = await HaiClient.fromCredentials('agent-issue-009-3', keypair.privateKeyPem, {
      url: 'https://hai.example',
    });
    const signImageMock = vi.fn(
      async (_in: string, _out: string, opts: Record<string, unknown>) => {
        expect(opts.unsafe_bak_mode).toBe(0o644);
        return {
          out_path: _out,
          signer_id: 'agent-issue-009-3',
          format: 'png',
          robust: false,
          backup_path: '/x/out.png.bak',
        };
      },
    );
    client._setFFIAdapter(createMockFFI({ signImage: signImageMock }));
    await client.signImage('/x/in.png', '/x/out.png', { unsafeBakMode: 0o644 });
    expect(signImageMock).toHaveBeenCalledTimes(1);
  });
});
