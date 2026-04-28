import { afterEach, describe, expect, it, vi } from 'vitest';
import { mkdtemp, mkdir, rm, writeFile, unlink } from 'node:fs/promises';
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { HaiClient } from '../src/client.js';
import { loadConfig, loadPrivateKey } from '../src/config.js';
import { generateTestKeypair as generateKeypair } from './setup.js';
import { createMockFFI } from './ffi-mock.js';

interface BootstrapRegisterContract {
  method: string;
  path: string;
  auth_required: boolean;
  public_key_encoding: string;
}

interface InitContractFixture {
  bootstrap_register: BootstrapRegisterContract;
  private_key_candidate_order: string[];
  config_discovery_order: string[];
  private_key_password_sources: string[];
  private_key_password_strategy: string;
}

function loadInitContractFixture(): InitContractFixture {
  const here = dirname(fileURLToPath(import.meta.url));
  const fixturePath = resolve(here, '../../fixtures/init_contract.json');
  return JSON.parse(readFileSync(fixturePath, 'utf-8')) as InitContractFixture;
}

describe('shared init contract (node)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('private key candidate order matches shared fixture', async () => {
    const fixture = loadInitContractFixture();
    expect(fixture.config_discovery_order).toEqual([
      'explicit_path',
      'JACS_CONFIG_PATH',
      './jacs.config.json',
    ]);
    expect(fixture.private_key_password_sources).toEqual([
      'JACS_PRIVATE_KEY_PASSWORD',
      'JACS_PASSWORD_FILE',
    ]);
    expect(fixture.private_key_password_strategy).toBe('single_source_required');

    const tmp = await mkdtemp(join(tmpdir(), 'haiai-node-init-contract-'));
    try {
      const keyDir = join(tmp, 'keys');
      const configPath = join(tmp, 'jacs.config.json');
      await mkdir(keyDir, { recursive: true });
      await writeFile(configPath, JSON.stringify({
        jacsAgentName: 'agent-alpha',
        jacsAgentVersion: '1.0.0',
        jacsKeyDir: './keys',
      }));
      const config = await loadConfig(configPath);

      const fileNames = fixture.private_key_candidate_order.map((name) =>
        name.replace('{agentName}', 'agent-alpha'),
      );
      const candidates = fileNames.map((name) => join(keyDir, name));
      await writeFile(candidates[0], 'first');
      await writeFile(candidates[1], 'second');
      await writeFile(candidates[2], 'third');

      await expect(loadPrivateKey(config)).resolves.toBe('first');

      await unlink(candidates[0]);
      await expect(loadPrivateKey(config)).resolves.toBe('second');

      await unlink(candidates[1]);
      await expect(loadPrivateKey(config)).resolves.toBe('third');
    } finally {
      await rm(tmp, { recursive: true, force: true });
    }
  });

  it('bootstrap register contract matches shared fixture', async () => {
    const fixture = loadInitContractFixture();
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials(
      'bootstrap-agent',
      keypair.privateKeyPem,
      { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' },
    );

    const registerMock = vi.fn(async (options: Record<string, unknown>) => {
      expect(options.owner_email).toBe('owner@hai.ai');
      expect(options.domain).toBe('agent.example');
      return {
        agent_id: 'agent-123',
        jacs_id: 'bootstrap-agent',
        registration_id: 'reg-1',
        registered_at: '2026-01-01T00:00:00Z',
      };
    });
    client._setFFIAdapter(createMockFFI({ registerNewAgent: registerMock }));

    await client.registerNewAgent('bootstrap-agent', {
      ownerEmail: 'owner@hai.ai',
      domain: 'agent.example',
      description: 'Node shared init contract',
      password: 'keygen-password',
      quiet: true,
    });
  });
});
