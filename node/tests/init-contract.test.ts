import { afterEach, describe, expect, it, vi } from 'vitest';
import { mkdtemp, mkdir, rm, writeFile, unlink } from 'node:fs/promises';
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { HaiClient, registerNewAgent } from '../src/client.js';
import { loadConfig, loadPrivateKey } from '../src/config.js';
import { generateTestKeypair as generateKeypair } from './setup.js';
import { createMockFFI } from './ffi-mock.js';
import { FFIClientAdapter } from '../src/ffi-client.js';

interface BootstrapRegisterContract {
  method: string;
  path: string;
  auth_required: boolean;
  public_key_encoding: string;
}

interface InitContractFixture {
  bootstrap_register: BootstrapRegisterContract;
  registration_outcomes: {
    requested_name: string;
    cases: Array<{
      name: string;
      http_status: number;
      response: Record<string, unknown>;
      expected_status: string | null;
      expected_email: string | null;
    }>;
  };
  existing_identity_register: {
    response: { agent_id: string; jacs_id: string; registered_at: string };
    cases: Array<{
      name: string;
      request: { agent_json: string; owner_email: string; registration_key?: string; public_key_pem?: string };
    }>;
  };
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

  const registrationContract = loadInitContractFixture().existing_identity_register;
  it.each(registrationContract.cases)('existing identity registration: $name', async ({ request }) => {
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials('fixture-existing-agent', keypair.privateKeyPem);
    const registerNative = vi.fn(async (_optionsJson: string) =>
      JSON.stringify(registrationContract.response),
    );
    // Exercise the real adapter's JSON serializer; only the native call is replaced.
    const adapter = Object.assign(Object.create(FFIClientAdapter.prototype), {
      native: { register: registerNative },
    }) as FFIClientAdapter;
    client._setFFIAdapter(adapter);

    const result = await client.register({
      agentJson: request.agent_json,
      ownerEmail: request.owner_email,
      ...(request.public_key_pem === undefined ? {} : { publicKeyPem: request.public_key_pem }),
      ...(request.registration_key === undefined ? {} : { registrationKey: request.registration_key }),
    });

    expect(registerNative).toHaveBeenCalledOnce();
    expect(JSON.parse(registerNative.mock.calls[0][0])).toEqual(request);
    expect(result.agentId).toBe(registrationContract.response.agent_id);
  });

  for (const entrypoint of ['ordinary', 'bootstrap', 'standalone'] as const) {
    it.each(loadInitContractFixture().registration_outcomes.cases)(
      `${entrypoint} preserves registration outcome: $name`, async (testCase) => {
        const nativeCall = vi.fn(async (_optionsJson: string) => {
          if (testCase.http_status >= 400) throw new Error(String(testCase.response.message));
          return JSON.stringify(testCase.response);
        });
        const adapter = Object.assign(Object.create(FFIClientAdapter.prototype), {
          native: { register: nativeCall, registerNewAgent: nativeCall },
        }) as FFIClientAdapter;
        const output = vi.spyOn(console, 'log').mockImplementation(() => {});
        vi.spyOn(FFIClientAdapter, 'create').mockResolvedValue(adapter);
        const opts = { ownerEmail: 'owner@example.test', password: 'synthetic-password' };
        const invoke = async () => {
          if (entrypoint === 'standalone') return registerNewAgent('requested-agent', opts);
          const client = await HaiClient.fromCredentials('existing', generateKeypair().privateKeyPem);
          client._setFFIAdapter(adapter);
          return entrypoint === 'ordinary'
            ? client.register({ agentJson: '{"jacsId":"existing"}' })
            : client.registerNewAgent('requested-agent', opts);
        };
        if (testCase.http_status >= 400) {
          await expect(invoke()).rejects.toThrow('Registration key was not accepted');
        } else {
          const result = await invoke();
          expect(result.agentId).toBe(testCase.response.agent_id);
          expect(result.registrationStatus).toBe(testCase.expected_status ?? undefined);
          expect(result.email).toBe(testCase.expected_email ?? undefined);
          if (entrypoint !== 'ordinary') {
            const text = output.mock.calls.map(args => args.join(' ')).join('\n');
            expect(text).toContain(`Registration status: ${testCase.expected_status ?? 'unknown'}`);
            expect(text).not.toContain('requested-agent@hai.ai');
            expect(text.toLowerCase()).not.toContain('verification email has been sent');
            if (testCase.expected_email) expect(text).toContain(`Assigned email: ${testCase.expected_email}`);
            else expect(text).not.toContain('Assigned email:');
          }
        }
        expect(nativeCall).toHaveBeenCalledOnce();
      },
    );
  }

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
