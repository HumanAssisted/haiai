import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

// Track call order across mock methods
let callOrder: string[];

vi.mock('@hai.ai/jacs', () => {
  class MockJacsAgent {
    setPrivateKeyPassword(pw: string) {
      callOrder.push(`setPrivateKeyPassword:${pw}`);
    }
    async load(_configPath: string) {
      callOrder.push('load');
    }
  }
  return {
    JacsAgent: MockJacsAgent,
    createAgentSync: vi.fn(),
    verifyDocumentStandalone: vi.fn(),
    hashString: vi.fn(),
  };
});

describe('HaiClient.create password option', () => {
  let tmpDir: string;
  let configPath: string;

  beforeEach(async () => {
    callOrder = [];
    tmpDir = await mkdtemp(join(tmpdir(), 'haiai-pw-test-'));
    const keyDir = join(tmpDir, 'keys');
    await mkdir(keyDir, { recursive: true });
    configPath = join(tmpDir, 'jacs.config.json');
    await writeFile(configPath, JSON.stringify({
      jacsAgentName: 'pw-test-agent',
      jacsAgentVersion: '1.0.0',
      jacsKeyDir: './keys',
    }));
  });

  afterEach(async () => {
    vi.restoreAllMocks();
    await rm(tmpDir, { recursive: true, force: true });
  });

  it('passes password to agent via setPrivateKeyPassword before load', async () => {
    const { HaiClient } = await import('../src/client.js');

    await HaiClient.create({
      configPath,
      password: 'test-secret-pw',
    });

    expect(callOrder).toEqual([
      'setPrivateKeyPassword:test-secret-pw',
      'load',
    ]);
  });

  it('does not call setPrivateKeyPassword when password is undefined', async () => {
    const { HaiClient } = await import('../src/client.js');

    await HaiClient.create({ configPath });

    expect(callOrder).toEqual(['load']);
  });

  it('calls setPrivateKeyPassword even for empty string (null-check, not truthiness)', async () => {
    const { HaiClient } = await import('../src/client.js');

    await HaiClient.create({
      configPath,
      password: '',
    });

    expect(callOrder).toEqual([
      'setPrivateKeyPassword:',
      'load',
    ]);
  });
});
