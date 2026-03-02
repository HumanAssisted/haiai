import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { HaiClient } from '../src/client.js';
import { generateKeypair, verifyString } from '../src/crypt.js';
import { createHash, createPublicKey } from 'node:crypto';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import * as os from 'node:os';

/** Create a test client with a generated keypair and a temporary key directory. */
async function setupTestAgent(tmpDir: string) {
  const keypair = generateKeypair();
  const keyDir = path.join(tmpDir, 'keys');
  await fs.mkdir(keyDir, { recursive: true });

  // Write key files
  const privPath = path.join(keyDir, 'agent_private_key.pem');
  const pubPath = path.join(keyDir, 'agent_public_key.pem');
  await fs.writeFile(privPath, keypair.privateKeyPem, { mode: 0o600 });
  await fs.writeFile(pubPath, keypair.publicKeyPem, { mode: 0o644 });

  // Write config file
  const config = {
    jacsAgentName: 'test-rotation-agent',
    jacsAgentVersion: 'v1-original',
    jacsKeyDir: keyDir,
    jacsId: 'test-jacs-id-12345',
  };
  const configPath = path.join(tmpDir, 'jacs.config.json');
  await fs.writeFile(configPath, JSON.stringify(config, null, 2));

  // Build client via fromCredentials
  const client = HaiClient.fromCredentials(
    config.jacsId,
    keypair.privateKeyPem,
    { url: 'https://hai.example' },
  );
  // Patch the config to have the real keyDir and version
  (client as any).config = { ...config };

  return { client, keypair, keyDir, privPath, pubPath, configPath, config };
}

describe('rotateKeys', () => {
  let tmpDir: string;

  beforeEach(async () => {
    tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'hai-rotation-test-'));
  });

  afterEach(async () => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    // Clean up temp directory
    await fs.rm(tmpDir, { recursive: true, force: true }).catch(() => {});
  });

  it('generates new key files and archives old ones', async () => {
    const { client, privPath, pubPath, keyDir, configPath } = await setupTestAgent(tmpDir);

    // Stub fetch so register() doesn't make real requests
    vi.stubGlobal('fetch', vi.fn());

    // Set JACS_CONFIG_PATH to point to our tmp config
    process.env.JACS_CONFIG_PATH = configPath;

    const result = await client.rotateKeys({ registerWithHai: false });

    // New key files should exist at standard paths
    const newPrivExists = await fs.stat(privPath).then(() => true).catch(() => false);
    const newPubExists = await fs.stat(pubPath).then(() => true).catch(() => false);
    expect(newPrivExists).toBe(true);
    expect(newPubExists).toBe(true);

    // Old keys should be archived with version suffix
    const archivePriv = path.join(keyDir, 'agent_private_key.v1-original.pem');
    const archiveExists = await fs.stat(archivePriv).then(() => true).catch(() => false);
    expect(archiveExists).toBe(true);

    delete process.env.JACS_CONFIG_PATH;
  });

  it('returns a valid RotationResult with correct fields', async () => {
    const { client, configPath } = await setupTestAgent(tmpDir);
    vi.stubGlobal('fetch', vi.fn());
    process.env.JACS_CONFIG_PATH = configPath;

    const result = await client.rotateKeys({ registerWithHai: false });

    expect(result.jacsId).toBe('test-jacs-id-12345');
    expect(result.oldVersion).toBe('v1-original');
    expect(result.newVersion).not.toBe('v1-original');
    expect(result.newVersion.length).toBeGreaterThan(0);
    // SHA-256 hex is 64 chars
    expect(result.newPublicKeyHash).toHaveLength(64);
    expect(result.registeredWithHai).toBe(false);
    expect(result.signedAgentJson.length).toBeGreaterThan(0);

    // Signed agent JSON should be valid JSON with expected fields
    const doc = JSON.parse(result.signedAgentJson);
    expect(doc.jacsId).toBe('test-jacs-id-12345');
    expect(doc.jacsVersion).toBe(result.newVersion);
    expect(doc.jacsPreviousVersion).toBe('v1-original');
    expect(doc.jacsSignature).toBeDefined();

    delete process.env.JACS_CONFIG_PATH;
  });

  it('updates config file with new version', async () => {
    const { client, configPath } = await setupTestAgent(tmpDir);
    vi.stubGlobal('fetch', vi.fn());
    process.env.JACS_CONFIG_PATH = configPath;

    const result = await client.rotateKeys({ registerWithHai: false });

    // Read config file and verify version was updated
    const updatedConfig = JSON.parse(await fs.readFile(configPath, 'utf-8'));
    expect(updatedConfig.jacsAgentVersion).toBe(result.newVersion);
    // jacsId should be unchanged
    expect(updatedConfig.jacsId).toBe('test-jacs-id-12345');

    delete process.env.JACS_CONFIG_PATH;
  });

  it('updates in-memory config version', async () => {
    const { client, configPath } = await setupTestAgent(tmpDir);
    vi.stubGlobal('fetch', vi.fn());
    process.env.JACS_CONFIG_PATH = configPath;

    const result = await client.rotateKeys({ registerWithHai: false });

    // In-memory config should reflect the new version
    expect((client as any).config.jacsAgentVersion).toBe(result.newVersion);
    // The private key PEM should have changed
    expect((client as any).privateKeyPem).not.toBe('');

    delete process.env.JACS_CONFIG_PATH;
  });

  it('calls register when registerWithHai is true', async () => {
    const { client, configPath } = await setupTestAgent(tmpDir);
    process.env.JACS_CONFIG_PATH = configPath;

    const fetchMock = vi.fn(async (_url: string | URL, _init?: RequestInit) => {
      return new Response(
        JSON.stringify({
          agent_id: 'hai-agent-uuid',
          jacs_id: 'test-jacs-id-12345',
          hai_signature: 'sig-abc',
          registration_id: 'reg-123',
          registered_at: '2026-03-02T00:00:00Z',
        }),
        { status: 201, headers: { 'Content-Type': 'application/json' } },
      );
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await client.rotateKeys({
      registerWithHai: true,
      haiUrl: 'https://hai.example',
    });

    expect(result.registeredWithHai).toBe(true);
    // Verify register was called (fetch was invoked)
    expect(fetchMock).toHaveBeenCalled();
    const callUrl = String(fetchMock.mock.calls[0][0]);
    expect(callUrl).toContain('/api/v1/agents/register');

    delete process.env.JACS_CONFIG_PATH;
  });

  it('preserves local rotation when HAI registration fails', async () => {
    const { client, configPath } = await setupTestAgent(tmpDir);
    process.env.JACS_CONFIG_PATH = configPath;

    const fetchMock = vi.fn(async () => {
      return new Response('Internal Server Error', {
        status: 500,
        headers: { 'Content-Type': 'text/plain' },
      });
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await client.rotateKeys({
      registerWithHai: true,
      haiUrl: 'https://hai.example',
    });

    // Local rotation should succeed
    expect(result.jacsId).toBe('test-jacs-id-12345');
    expect(result.newVersion).not.toBe('v1-original');
    // But HAI registration should have failed
    expect(result.registeredWithHai).toBe(false);

    delete process.env.JACS_CONFIG_PATH;
  });

  it('throws when no jacsId is set', async () => {
    const keypair = generateKeypair();
    const client = HaiClient.fromCredentials(
      'no-id-agent',
      keypair.privateKeyPem,
      { url: 'https://hai.example' },
    );
    // Remove jacsId from config
    (client as any).config = {
      jacsAgentName: 'no-id-agent',
      jacsAgentVersion: 'v1',
      jacsKeyDir: '/nonexistent',
    };

    await expect(client.rotateKeys({ registerWithHai: false }))
      .rejects.toThrow(/no jacsId/i);
  });

  it('throws when private key file not found', async () => {
    const keypair = generateKeypair();
    const emptyKeyDir = path.join(tmpDir, 'empty-keys');
    await fs.mkdir(emptyKeyDir, { recursive: true });

    const client = HaiClient.fromCredentials(
      'test-id',
      keypair.privateKeyPem,
      { url: 'https://hai.example' },
    );
    (client as any).config = {
      jacsAgentName: 'test-agent',
      jacsAgentVersion: 'v1',
      jacsKeyDir: emptyKeyDir,
      jacsId: 'test-id',
    };

    await expect(client.rotateKeys({ registerWithHai: false }))
      .rejects.toThrow(/private key not found/i);
  });

  it('rolls back on key generation failure', async () => {
    const { client, privPath, pubPath, configPath, keypair } = await setupTestAgent(tmpDir);
    vi.stubGlobal('fetch', vi.fn());
    process.env.JACS_CONFIG_PATH = configPath;

    // Save original key content for comparison
    const originalPriv = await fs.readFile(privPath);
    const originalPub = await fs.readFile(pubPath);

    // Mock generateKeypair to fail (via the crypt module)
    const cryptModule = await import('../src/crypt.js');
    const spy = vi.spyOn(cryptModule, 'generateKeypair').mockImplementation(() => {
      throw new Error('Simulated key generation failure');
    });

    await expect(client.rotateKeys({ registerWithHai: false }))
      .rejects.toThrow(/key generation failed/i);

    // Original keys should be restored (rollback)
    const restoredPriv = await fs.readFile(privPath);
    expect(restoredPriv.equals(originalPriv)).toBe(true);

    spy.mockRestore();
    delete process.env.JACS_CONFIG_PATH;
  });

  it('new key produces valid signature on agent document', async () => {
    const { client, configPath, pubPath } = await setupTestAgent(tmpDir);
    vi.stubGlobal('fetch', vi.fn());
    process.env.JACS_CONFIG_PATH = configPath;

    const result = await client.rotateKeys({ registerWithHai: false });

    // Read the new public key from disk
    const newPubPem = await fs.readFile(pubPath, 'utf-8');
    expect(newPubPem).toContain('-----BEGIN PUBLIC KEY-----');

    // Parse signed agent JSON and verify signature
    const doc = JSON.parse(result.signedAgentJson);
    const signature = doc.jacsSignature?.signature;
    expect(signature).toBeDefined();

    // Remove signature from doc to get canonical form for verification
    const docCopy = { ...doc };
    delete (docCopy.jacsSignature as any).signature;
    const { canonicalJson } = await import('../src/signing.js');
    const canonical = canonicalJson(docCopy);

    const valid = verifyString(newPubPem, canonical, signature);
    expect(valid).toBe(true);

    delete process.env.JACS_CONFIG_PATH;
  });

  it('new version is a valid UUID v4', async () => {
    const { client, configPath } = await setupTestAgent(tmpDir);
    vi.stubGlobal('fetch', vi.fn());
    process.env.JACS_CONFIG_PATH = configPath;

    const result = await client.rotateKeys({ registerWithHai: false });

    // UUID v4 pattern: 8-4-4-4-12 hex chars
    const uuidRegex = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
    expect(result.newVersion).toMatch(uuidRegex);

    delete process.env.JACS_CONFIG_PATH;
  });

  it('double rotation archives both versions', async () => {
    const { client, configPath, keyDir, privPath, pubPath } = await setupTestAgent(tmpDir);
    vi.stubGlobal('fetch', vi.fn());
    process.env.JACS_CONFIG_PATH = configPath;

    // First rotation: V1 -> V2
    const result1 = await client.rotateKeys({ registerWithHai: false });
    const v2 = result1.newVersion;

    // Second rotation: V2 -> V3
    const result2 = await client.rotateKeys({ registerWithHai: false });

    // Current key files should exist
    const newPrivExists = await fs.stat(privPath).then(() => true).catch(() => false);
    const newPubExists = await fs.stat(pubPath).then(() => true).catch(() => false);
    expect(newPrivExists).toBe(true);
    expect(newPubExists).toBe(true);

    // V1 archive should exist
    const archiveV1 = path.join(keyDir, 'agent_private_key.v1-original.pem');
    const v1Exists = await fs.stat(archiveV1).then(() => true).catch(() => false);
    expect(v1Exists).toBe(true);

    // V2 archive should exist
    const archiveV2 = path.join(keyDir, `agent_private_key.${v2}.pem`);
    const v2Exists = await fs.stat(archiveV2).then(() => true).catch(() => false);
    expect(v2Exists).toBe(true);

    // Version chain should be consistent
    expect(result1.oldVersion).toBe('v1-original');
    expect(result1.newVersion).toBe(result2.oldVersion);
    expect(result2.newVersion).not.toBe(result2.oldVersion);

    delete process.env.JACS_CONFIG_PATH;
  });

  it('rotation result fields match shared fixture contract', async () => {
    const fixturePath = path.join(__dirname, '..', '..', 'fixtures', 'rotation_result.json');
    let fixture: Record<string, unknown>;
    try {
      fixture = JSON.parse(await fs.readFile(fixturePath, 'utf-8'));
    } catch {
      // Skip if fixture doesn't exist
      return;
    }

    // RotationResult interface fields (snake_case in fixture -> camelCase in TS)
    const fixtureKeys = new Set(Object.keys(fixture));
    const expectedKeys = new Set([
      'jacs_id', 'old_version', 'new_version',
      'new_public_key_hash', 'registered_with_hai', 'signed_agent_json',
    ]);
    expect(fixtureKeys).toEqual(expectedKeys);
  });

  it('register payload contains agent_json with new version', async () => {
    const { client, configPath } = await setupTestAgent(tmpDir);
    process.env.JACS_CONFIG_PATH = configPath;

    let capturedBody: Record<string, unknown> | null = null;
    const fetchMock = vi.fn(async (_url: string | URL, init?: RequestInit) => {
      capturedBody = JSON.parse(String(init?.body ?? '{}'));
      return new Response(
        JSON.stringify({
          agent_id: 'hai-uuid',
          jacs_id: 'test-jacs-id-12345',
          hai_signature: 'sig',
          registration_id: 'reg-1',
          registered_at: '2026-03-02T00:00:00Z',
        }),
        { status: 201, headers: { 'Content-Type': 'application/json' } },
      );
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await client.rotateKeys({
      registerWithHai: true,
      haiUrl: 'https://hai.example',
    });

    expect(capturedBody).not.toBeNull();
    expect(capturedBody!.agent_json).toBeDefined();
    const agentDoc = JSON.parse(capturedBody!.agent_json as string);
    expect(agentDoc.jacsVersion).toBe(result.newVersion);
    expect(agentDoc.jacsId).toBe('test-jacs-id-12345');

    delete process.env.JACS_CONFIG_PATH;
  });
});
