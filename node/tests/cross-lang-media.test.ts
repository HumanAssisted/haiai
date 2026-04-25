/**
 * Cross-language media verify-parity tests (Node side).
 *
 * Mirrors `rust/haiai/tests/cross_lang_contract.rs`
 * (`cross_lang_signed_image_*` and `cross_lang_signed_text_md_*`) and
 * `python/tests/test_cross_lang_media.py`. Loads the same pre-signed
 * fixtures from `fixtures/media/signed.{png,jpg,webp,md}` (signed once by
 * the Rust regenerator in `rust/haiai/tests/regen_media_fixtures.rs`,
 * signer = the shared test agent in `fixtures/jacs-agent/`) and asserts
 * that the Node FFI verifyImage / verifyText paths return the same
 * Valid / HashMismatch verdicts.
 *
 * Any drift between languages here MUST be a parity bug, not a test-only
 * quirk — that is the entire point of this suite (PRD §5.5, TASK_011).
 *
 * Skips gracefully when the installed haiinpm native binding does not yet
 * expose the media methods (i.e., it predates TASK_008 napi-rs bindings —
 * rebuild via `cargo build -p haiinpm --release` to re-run).
 */

import { afterAll, beforeAll, describe, it, expect } from 'vitest';
import { createRequire } from 'node:module';
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, statSync, copyFileSync, writeFileSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { createHash } from 'node:crypto';

const __filenameLocal = fileURLToPath(import.meta.url);
const __dirnameLocal = dirname(__filenameLocal);

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

const REPO_ROOT = resolve(__dirnameLocal, '../..');
const MEDIA_DIR = join(REPO_ROOT, 'fixtures', 'media');
const JACS_AGENT_DIR = join(REPO_ROOT, 'fixtures', 'jacs-agent');
const SIGNER_FIXTURE_PATH = join(MEDIA_DIR, 'SIGNER.json');
const CHECKSUMS_PATH = join(MEDIA_DIR, 'CHECKSUMS.txt');

// Password for the shared fixture agent (matches Rust + Python).
const FIXTURE_AGENT_PASSWORD = 'secretpassord';

// Set the password before any FFI module is loaded — JACS reads this at
// agent-construction time, and vitest's workers do not always propagate
// process.env mutations made later in the test lifecycle.
process.env.JACS_PRIVATE_KEY_PASSWORD = FIXTURE_AGENT_PASSWORD;

// ---------------------------------------------------------------------------
// FFI availability — skip when haiinpm pre-dates TASK_008.
// ---------------------------------------------------------------------------

interface NativeHaiClientCtor {
  new (configJson: string): {
    verifyImage?: (path: string, optsJson: string) => Promise<string>;
    verifyText?: (path: string, optsJson: string) => Promise<string>;
    signImage?: (inPath: string, outPath: string, optsJson: string) => Promise<string>;
    signText?: (path: string, optsJson: string) => Promise<string>;
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
      typeof c.verifyImage === 'function' &&
      typeof c.verifyText === 'function' &&
      typeof c.signImage === 'function' &&
      typeof c.signText === 'function' &&
      typeof c.extractMediaSignature === 'function'
    );
  } catch {
    return false;
  }
}

const MEDIA_SUPPORTED = probeMediaSupport();
const SKIP_REASON =
  'Installed haiinpm native binding does not expose the Layer-8 media methods. ' +
  'Rebuild via `cargo build -p haiinpm --release` then refresh ' +
  '`node/npm/haiinpm/haiinpm.darwin-arm64.node` once JACS 0.10.0+ is available.';

// ---------------------------------------------------------------------------
// Fixture-staging helpers — mirror Rust `cross_lang_contract.rs::stage_fixture_agent`.
// ---------------------------------------------------------------------------

interface SignerFixture {
  signer_id: string;
  algorithm: string;
}

function loadSigner(): SignerFixture {
  return JSON.parse(readFileSync(SIGNER_FIXTURE_PATH, 'utf-8')) as SignerFixture;
}

function loadChecksum(name: string): string {
  for (const line of readFileSync(CHECKSUMS_PATH, 'utf-8').split('\n')) {
    const parts = line.trim().split(/\s+/);
    if (parts.length === 2 && parts[1] === name) return parts[0];
  }
  throw new Error(`no checksum for ${name} in CHECKSUMS.txt`);
}

function readSignedWithChecksum(name: string): Buffer {
  const bytes = readFileSync(join(MEDIA_DIR, name));
  const expected = loadChecksum(name);
  const got = createHash('sha256').update(bytes).digest('hex');
  if (got !== expected) {
    throw new Error(`checksum drift on fixtures/media/${name}: got ${got}, expected ${expected}`);
  }
  return bytes;
}

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

  const tmpDir = mkdtempSync(join(tmpdir(), 'haiai-media-parity-'));
  const cfg = JSON.parse(readFileSync(join(JACS_AGENT_DIR, 'jacs.config.json'), 'utf-8')) as Record<
    string,
    unknown
  >;

  // Keys: copy verbatim.
  const srcKeys = join(JACS_AGENT_DIR, cfg.jacs_key_directory as string);
  const tmpKeys = join(tmpDir, 'keys');
  mkdirSync(tmpKeys, { recursive: true });
  for (const name of readdirSync(srcKeys)) {
    copyFileSync(join(srcKeys, name), join(tmpKeys, name));
  }

  // Data: agent JSON filenames use `_` placeholders for `:`. Map back.
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
  verifyImage: (path: string, optsJson: string) => Promise<string>;
  verifyText: (path: string, optsJson: string) => Promise<string>;
}

function buildFFIClient(): { client: FFIClient; tmpDir: string } {
  const haiinpm = loadHaiinpm();
  if (!haiinpm) throw new Error('haiinpm not loadable');

  // The JACS native side reads `JACS_PRIVATE_KEY_PASSWORD` via Rust's
  // `std::env::var` (the OS environment block, not Node's `process.env`
  // proxy). vitest's default `threads` pool wraps env mutations so the
  // native side does not see them; this test runs in the `forks` pool via
  // `poolMatchGlobs` in `vitest.config.ts` so re-setting here is sufficient.
  process.env.JACS_PRIVATE_KEY_PASSWORD = FIXTURE_AGENT_PASSWORD;

  const staged = stageFixtureAgent();
  const cfg = JSON.parse(readFileSync(staged.configPath, 'utf-8')) as Record<string, unknown>;
  const ffiConfig = JSON.stringify({
    jacs_id: (cfg.jacs_agent_id_and_version as string).split(':')[0],
    agent_name: 'FixtureAgent',
    agent_version: '1.0.0',
    key_dir: cfg.jacs_key_directory,
    jacs_config_path: staged.configPath,
    base_url: 'http://localhost:1', // never used; verify is local-only
  });
  const native = new haiinpm.HaiClient(ffiConfig);
  return {
    client: native as unknown as FFIClient,
    tmpDir: staged.tmpDir,
  };
}

// ---------------------------------------------------------------------------
// Tampering helpers — mirror Rust `tamper_after` + `tamper_text_body`.
// ---------------------------------------------------------------------------

function tamperAfter(buf: Buffer, marker: Buffer, offset: number): void {
  const idx = buf.indexOf(marker);
  if (idx === -1) throw new Error(`marker ${marker.toString('hex')} not found`);
  const target = idx + marker.length + offset;
  buf[target] ^= 0x01;
}

function tamperTextBody(buf: Buffer): void {
  const marker = Buffer.from('-----BEGIN JACS SIGNATURE-----', 'utf-8');
  const bodyEnd = buf.indexOf(marker);
  if (bodyEnd === -1) throw new Error('BEGIN marker present in signed.md');
  for (let i = bodyEnd - 1; i >= 0; i--) {
    const c = buf[i];
    if ((c >= 0x41 && c <= 0x5a) || (c >= 0x61 && c <= 0x7a)) {
      buf[i] ^= 0b0010_0000;
      return;
    }
  }
  throw new Error('no ASCII letter found before signature block');
}

// ---------------------------------------------------------------------------
// Per-suite state
// ---------------------------------------------------------------------------

let ffiClient: FFIClient | null = null;
let parityTmpDir: string | null = null;
let stageTmpDir: string | null = null;
let signer: SignerFixture | null = null;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

const describeMaybe = MEDIA_SUPPORTED ? describe : describe.skip;

describeMaybe('cross-language media verify-parity (node)', () => {
  beforeAll(() => {
    if (!MEDIA_SUPPORTED) return;
    signer = loadSigner();
    const built = buildFFIClient();
    ffiClient = built.client;
    parityTmpDir = built.tmpDir;
    stageTmpDir = mkdtempSync(join(tmpdir(), 'haiai-media-stage-'));
  });

  afterAll(() => {
    if (parityTmpDir) rmSync(parityTmpDir, { recursive: true, force: true });
    if (stageTmpDir) rmSync(stageTmpDir, { recursive: true, force: true });
  });

  if (!MEDIA_SUPPORTED) {
    // Single placeholder so the suite still appears in test output.
    it.skip(SKIP_REASON, () => {});
    return;
  }

  // -------------------------------------------------------------------------
  // verify_image — Node parity
  // -------------------------------------------------------------------------

  async function verifyImage(path: string): Promise<Record<string, unknown>> {
    return JSON.parse(await ffiClient!.verifyImage(path, '{}')) as Record<string, unknown>;
  }

  async function verifyText(path: string): Promise<Record<string, unknown>> {
    return JSON.parse(await ffiClient!.verifyText(path, '{}')) as Record<string, unknown>;
  }

  function stageSigned(name: string): string {
    const target = join(stageTmpDir!, name);
    writeFileSync(target, readSignedWithChecksum(name));
    return target;
  }

  function stageTampered(name: string, marker: Buffer, offset: number): string {
    const target = join(stageTmpDir!, name);
    const buf = Buffer.from(readSignedWithChecksum(name));
    tamperAfter(buf, marker, offset);
    writeFileSync(target, buf);
    return target;
  }

  it('signed.png verifies under fixture agent', async () => {
    const res = await verifyImage(stageSigned('signed.png'));
    expect(res.status).toBe('valid');
    expect(res.signer_id).toBe(signer!.signer_id);
  });

  it('signed.png tampered returns hash_mismatch', async () => {
    const path = stageTampered('signed.png', Buffer.from('IDAT', 'utf-8'), 6);
    const res = await verifyImage(path);
    expect(res.status).toBe('hash_mismatch');
  });

  it('signed.jpg verifies under fixture agent', async () => {
    const res = await verifyImage(stageSigned('signed.jpg'));
    expect(res.status).toBe('valid');
    expect(res.signer_id).toBe(signer!.signer_id);
  });

  it('signed.jpg tampered returns hash_mismatch', async () => {
    const path = stageTampered('signed.jpg', Buffer.from([0xff, 0xda]), 4);
    const res = await verifyImage(path);
    expect(res.status).toBe('hash_mismatch');
  });

  it('signed.webp verifies under fixture agent', async () => {
    const res = await verifyImage(stageSigned('signed.webp'));
    expect(res.status).toBe('valid');
    expect(res.signer_id).toBe(signer!.signer_id);
  });

  it('signed.webp tampered returns hash_mismatch', async () => {
    const path = stageTampered('signed.webp', Buffer.from('VP8L', 'utf-8'), 4);
    const res = await verifyImage(path);
    expect(res.status).toBe('hash_mismatch');
  });

  // -------------------------------------------------------------------------
  // verify_text — Node parity
  // -------------------------------------------------------------------------

  it('signed.md verifies under fixture agent', async () => {
    const target = join(stageTmpDir!, 'signed.md');
    writeFileSync(target, readSignedWithChecksum('signed.md'));
    const res = await verifyText(target);
    expect(res.status).toBe('signed');
    const sigs = (res.signatures as Array<Record<string, unknown>> | undefined) ?? [];
    expect(sigs).toHaveLength(1);
    expect(sigs[0].status).toBe('valid');
    expect(sigs[0].signer_id).toBe(signer!.signer_id);
  });

  it('signed.md tampered returns hash_mismatch', async () => {
    const target = join(stageTmpDir!, 'signed.md');
    const buf = Buffer.from(readSignedWithChecksum('signed.md'));
    tamperTextBody(buf);
    writeFileSync(target, buf);
    const res = await verifyText(target);
    expect(res.status).toBe('signed');
    const sigs = (res.signatures as Array<Record<string, unknown>> | undefined) ?? [];
    expect(sigs).toHaveLength(1);
    expect(sigs[0].status).toBe('hash_mismatch');
  });
});
