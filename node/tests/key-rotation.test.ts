import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { HaiClient } from '../src/client.js';
import { AuthenticationError } from '../src/errors.js';
import { generateTestKeypair as generateKeypair } from './setup.js';
import { createMockFFI } from './ffi-mock.js';
import { mapFFIError } from '../src/ffi-client.js';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import * as os from 'node:os';

describe('rotateKeys', () => {
  let tmpDir: string;

  beforeEach(async () => {
    tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'hai-rotation-test-'));
  });

  afterEach(async () => {
    vi.restoreAllMocks();
    await fs.rm(tmpDir, { recursive: true, force: true }).catch(() => {});
  });

  it('delegates to FFI rotateKeys and returns RotationResult', async () => {
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials(
      'test-jacs-id-12345',
      keypair.privateKeyPem,
      { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' },
    );

    const rotateKeysMock = vi.fn(async (options: Record<string, unknown>) => {
      expect(options.register_with_hai).toBe(false);
      return {
        jacs_id: 'test-jacs-id-12345',
        old_version: 'v1-original',
        new_version: 'aaaaaaaa-bbbb-4ccc-9ddd-eeeeeeeeeeee',
        new_public_key_hash: 'a'.repeat(64),
        registered_with_hai: false,
        signed_agent_json: '{"jacsId":"test-jacs-id-12345","jacsVersion":"aaaaaaaa-bbbb-4ccc-9ddd-eeeeeeeeeeee"}',
      };
    });
    client._setFFIAdapter(createMockFFI({ rotateKeys: rotateKeysMock }));

    const result = await client.rotateKeys({ registerWithHai: false });

    expect(result.jacsId).toBe('test-jacs-id-12345');
    expect(result.oldVersion).toBe('v1-original');
    expect(result.newVersion).toBe('aaaaaaaa-bbbb-4ccc-9ddd-eeeeeeeeeeee');
    expect(result.newPublicKeyHash).toHaveLength(64);
    expect(result.registeredWithHai).toBe(false);
    expect(result.signedAgentJson.length).toBeGreaterThan(0);
  });

  it('returns a valid RotationResult with correct fields', async () => {
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials(
      'test-jacs-id-12345',
      keypair.privateKeyPem,
      { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' },
    );

    const newVersion = 'aaaaaaaa-bbbb-4ccc-9ddd-eeeeeeeeeeee';
    const rotateKeysMock = vi.fn(async () => ({
      jacs_id: 'test-jacs-id-12345',
      old_version: 'v1-original',
      new_version: newVersion,
      new_public_key_hash: 'a'.repeat(64),
      registered_with_hai: false,
      signed_agent_json: JSON.stringify({
        jacsId: 'test-jacs-id-12345',
        jacsVersion: newVersion,
        jacsPreviousVersion: 'v1-original',
        jacsSignature: { agentID: 'test-jacs-id-12345', signature: 'sig' },
        jacsPublicKey: '-----BEGIN PUBLIC KEY-----\ntest\n-----END PUBLIC KEY-----',
      }),
    }));
    client._setFFIAdapter(createMockFFI({ rotateKeys: rotateKeysMock }));

    const result = await client.rotateKeys({ registerWithHai: false });

    expect(result.jacsId).toBe('test-jacs-id-12345');
    expect(result.oldVersion).toBe('v1-original');
    expect(result.newVersion).not.toBe('v1-original');
    expect(result.newPublicKeyHash).toHaveLength(64);
    expect(result.registeredWithHai).toBe(false);

    // Signed agent JSON should be valid JSON with expected fields
    const doc = JSON.parse(result.signedAgentJson);
    expect(doc.jacsId).toBe('test-jacs-id-12345');
    expect(doc.jacsVersion).toBe(result.newVersion);
    expect(doc.jacsPreviousVersion).toBe('v1-original');
    expect(doc.jacsSignature).toBeDefined();
  });

  it('updates in-memory config version', async () => {
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials(
      'test-jacs-id-12345',
      keypair.privateKeyPem,
      { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' },
    );

    const newVersion = 'aaaaaaaa-bbbb-4ccc-9ddd-eeeeeeeeeeee';
    const rotateKeysMock = vi.fn(async () => ({
      jacs_id: 'test-jacs-id-12345',
      old_version: 'v1-original',
      new_version: newVersion,
      new_public_key_hash: 'a'.repeat(64),
      registered_with_hai: false,
      signed_agent_json: '{}',
    }));
    client._setFFIAdapter(createMockFFI({ rotateKeys: rotateKeysMock }));

    await client.rotateKeys({ registerWithHai: false });

    // In-memory config should reflect the new version
    expect((client as any).config.jacsAgentVersion).toBe(newVersion);
  });

  it('calls FFI with registerWithHai true', async () => {
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials(
      'test-jacs-id-12345',
      keypair.privateKeyPem,
      { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' },
    );

    const rotateKeysMock = vi.fn(async (options: Record<string, unknown>) => {
      expect(options.register_with_hai).toBe(true);
      expect(options.hai_url).toBe('https://hai.example');
      return {
        jacs_id: 'test-jacs-id-12345',
        old_version: 'v1-original',
        new_version: 'new-v2',
        new_public_key_hash: 'a'.repeat(64),
        registered_with_hai: true,
        signed_agent_json: '{}',
      };
    });
    client._setFFIAdapter(createMockFFI({ rotateKeys: rotateKeysMock }));

    const result = await client.rotateKeys({
      registerWithHai: true,
      haiUrl: 'https://hai.example',
    });

    expect(result.registeredWithHai).toBe(true);
    expect(rotateKeysMock).toHaveBeenCalledTimes(1);
  });

  it('preserves local rotation when HAI registration fails in FFI', async () => {
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials(
      'test-jacs-id-12345',
      keypair.privateKeyPem,
      { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' },
    );

    const rotateKeysMock = vi.fn(async () => ({
      jacs_id: 'test-jacs-id-12345',
      old_version: 'v1-original',
      new_version: 'new-v2',
      new_public_key_hash: 'a'.repeat(64),
      registered_with_hai: false,
      signed_agent_json: '{}',
    }));
    client._setFFIAdapter(createMockFFI({ rotateKeys: rotateKeysMock }));

    const result = await client.rotateKeys({
      registerWithHai: true,
      haiUrl: 'https://hai.example',
    });

    expect(result.jacsId).toBe('test-jacs-id-12345');
    expect(result.newVersion).not.toBe('v1-original');
    expect(result.registeredWithHai).toBe(false);
  });

  it('throws when FFI reports no jacsId', async () => {
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials(
      'no-id-agent',
      keypair.privateKeyPem,
      { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' },
    );

    const rotateKeysMock = vi.fn(async () => {
      throw mapFFIError(new Error('ConfigFailed: Cannot rotate keys: no jacsId in config'));
    });
    client._setFFIAdapter(createMockFFI({ rotateKeys: rotateKeysMock }));

    await expect(client.rotateKeys({ registerWithHai: false }))
      .rejects.toThrow(/no jacsId/i);
  });

  it('throws when FFI reports private key not found', async () => {
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials(
      'test-id',
      keypair.privateKeyPem,
      { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' },
    );

    const rotateKeysMock = vi.fn(async () => {
      throw mapFFIError(new Error('ConfigFailed: Cannot rotate keys: private key not found'));
    });
    client._setFFIAdapter(createMockFFI({ rotateKeys: rotateKeysMock }));

    await expect(client.rotateKeys({ registerWithHai: false }))
      .rejects.toThrow(/private key not found/i);
  });

  it('new version UUID from FFI is valid', async () => {
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials(
      'test-jacs-id-12345',
      keypair.privateKeyPem,
      { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' },
    );

    const newVersion = 'aaaaaaaa-bbbb-4ccc-9ddd-eeeeeeeeeeee';
    const rotateKeysMock = vi.fn(async () => ({
      jacs_id: 'test-jacs-id-12345',
      old_version: 'v1-original',
      new_version: newVersion,
      new_public_key_hash: 'a'.repeat(64),
      registered_with_hai: false,
      signed_agent_json: '{}',
    }));
    client._setFFIAdapter(createMockFFI({ rotateKeys: rotateKeysMock }));

    const result = await client.rotateKeys({ registerWithHai: false });

    // UUID v4 pattern
    const uuidRegex = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
    expect(result.newVersion).toMatch(uuidRegex);
  });

  it('signed agent JSON includes new version and key', async () => {
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials(
      'test-jacs-id-12345',
      keypair.privateKeyPem,
      { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' },
    );

    const newVersion = 'aaaaaaaa-bbbb-4ccc-9ddd-eeeeeeeeeeee';
    const signedDoc = {
      jacsId: 'test-jacs-id-12345',
      jacsVersion: newVersion,
      jacsPreviousVersion: 'v1-original',
      jacsSignature: { agentID: 'test-jacs-id-12345', signature: 'sig' },
      jacsPublicKey: '-----BEGIN PUBLIC KEY-----\ntest\n-----END PUBLIC KEY-----',
    };

    const rotateKeysMock = vi.fn(async () => ({
      jacs_id: 'test-jacs-id-12345',
      old_version: 'v1-original',
      new_version: newVersion,
      new_public_key_hash: 'a'.repeat(64),
      registered_with_hai: false,
      signed_agent_json: JSON.stringify(signedDoc),
    }));
    client._setFFIAdapter(createMockFFI({ rotateKeys: rotateKeysMock }));

    const result = await client.rotateKeys({ registerWithHai: false });

    const doc = JSON.parse(result.signedAgentJson);
    expect(doc.jacsSignature?.signature).toBeDefined();
    expect(doc.jacsPublicKey).toContain('BEGIN PUBLIC KEY');
    expect(doc.jacsVersion).toBe(result.newVersion);
    expect(doc.jacsPreviousVersion).toBe('v1-original');
  });

  it('double rotation produces correct version chain', async () => {
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials(
      'test-jacs-id-12345',
      keypair.privateKeyPem,
      { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' },
    );

    let callCount = 0;
    const rotateKeysMock = vi.fn(async () => {
      callCount++;
      if (callCount === 1) {
        return {
          jacs_id: 'test-jacs-id-12345',
          old_version: 'v1-original',
          new_version: 'v2-rotated',
          new_public_key_hash: 'a'.repeat(64),
          registered_with_hai: false,
          signed_agent_json: '{}',
        };
      }
      return {
        jacs_id: 'test-jacs-id-12345',
        old_version: 'v2-rotated',
        new_version: 'v3-rotated',
        new_public_key_hash: 'b'.repeat(64),
        registered_with_hai: false,
        signed_agent_json: '{}',
      };
    });
    client._setFFIAdapter(createMockFFI({ rotateKeys: rotateKeysMock }));

    const result1 = await client.rotateKeys({ registerWithHai: false });
    const result2 = await client.rotateKeys({ registerWithHai: false });

    expect(result1.oldVersion).toBe('v1-original');
    expect(result1.newVersion).toBe('v2-rotated');
    expect(result2.oldVersion).toBe('v2-rotated');
    expect(result2.newVersion).toBe('v3-rotated');
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

  it('register payload from FFI contains new version', async () => {
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials(
      'test-jacs-id-12345',
      keypair.privateKeyPem,
      { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' },
    );

    let capturedOptions: Record<string, unknown> | null = null;
    const rotateKeysMock = vi.fn(async (options: Record<string, unknown>) => {
      capturedOptions = options;
      return {
        jacs_id: 'test-jacs-id-12345',
        old_version: 'v1-original',
        new_version: 'v2-rotated',
        new_public_key_hash: 'a'.repeat(64),
        registered_with_hai: true,
        signed_agent_json: JSON.stringify({
          jacsId: 'test-jacs-id-12345',
          jacsVersion: 'v2-rotated',
        }),
      };
    });
    client._setFFIAdapter(createMockFFI({ rotateKeys: rotateKeysMock }));

    const result = await client.rotateKeys({
      registerWithHai: true,
      haiUrl: 'https://hai.example',
    });

    expect(capturedOptions).not.toBeNull();
    expect(capturedOptions!.register_with_hai).toBe(true);
    expect(result.newVersion).toBe('v2-rotated');
    expect(result.jacsId).toBe('test-jacs-id-12345');
  });
});
