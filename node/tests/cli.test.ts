import { afterEach, describe, expect, it, vi } from 'vitest';
import { mkdtemp, mkdir, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';

const {
  createMock,
  fromCredentialsMock,
  createAgentSyncMock,
  loadPrivateKeyPassphraseMock,
  runJacsCliMock,
} = vi.hoisted(() => ({
  createMock: vi.fn(),
  fromCredentialsMock: vi.fn(),
  createAgentSyncMock: vi.fn(),
  loadPrivateKeyPassphraseMock: vi.fn(),
  runJacsCliMock: vi.fn(),
}));

vi.mock('../src/client.js', () => ({
  HaiClient: {
    create: createMock,
    fromCredentials: fromCredentialsMock,
  },
}));

vi.mock('@hai.ai/jacs', () => ({
  createAgentSync: createAgentSyncMock,
}));

vi.mock('../src/config.js', () => ({
  loadPrivateKeyPassphrase: loadPrivateKeyPassphraseMock,
}));

vi.mock('../src/jacs.js', () => ({
  runJacsCli: runJacsCliMock,
}));

import { main } from '../src/cli.js';

function joinWrites(spy: ReturnType<typeof vi.spyOn>): string {
  return spy.mock.calls.map(([chunk]) => String(chunk)).join('');
}

describe('CLI command handling', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    createMock.mockReset();
    fromCredentialsMock.mockReset();
    createAgentSyncMock.mockReset();
    loadPrivateKeyPassphraseMock.mockReset();
    runJacsCliMock.mockReset();
  });

  it('prints global help without creating a client', async () => {
    const stdoutSpy = vi.spyOn(process.stdout, 'write').mockImplementation(() => true);

    const code = await main([]);

    expect(code).toBe(0);
    expect(joinWrites(stdoutSpy)).toContain('Usage: haisdk <command> [options]');
    expect(createMock).not.toHaveBeenCalled();
  });

  it('prints command-specific help', async () => {
    const stdoutSpy = vi.spyOn(process.stdout, 'write').mockImplementation(() => true);

    const code = await main(['hello', '--help']);

    expect(code).toBe(0);
    expect(joinWrites(stdoutSpy)).toContain('Usage: haisdk hello');
    expect(createMock).not.toHaveBeenCalled();
  });

  it('dispatches hello with include-test and URL override', async () => {
    const hello = vi.fn(async () => ({ message: 'ok', helloId: 'hello-1' }));
    createMock.mockResolvedValue({ hello });
    const stdoutSpy = vi.spyOn(process.stdout, 'write').mockImplementation(() => true);

    const code = await main(['hello', '--include-test', '--url', 'https://hai.example']);

    expect(code).toBe(0);
    expect(createMock).toHaveBeenCalledWith({ url: 'https://hai.example' });
    expect(hello).toHaveBeenCalledWith(true);
    expect(JSON.parse(joinWrites(stdoutSpy))).toEqual({ message: 'ok', helloId: 'hello-1' });
  });

  it('routes status with --jacs-id to agent attestation lookup', async () => {
    const verify = vi.fn();
    const getAgentAttestation = vi.fn(async () => ({ jacsId: 'agent-1', registered: true }));
    createMock.mockResolvedValue({ verify, getAgentAttestation });
    const stdoutSpy = vi.spyOn(process.stdout, 'write').mockImplementation(() => true);

    const code = await main(['status', '--jacs-id', 'agent-1']);

    expect(code).toBe(0);
    expect(getAgentAttestation).toHaveBeenCalledWith('agent-1');
    expect(verify).not.toHaveBeenCalled();
    expect(JSON.parse(joinWrites(stdoutSpy))).toEqual({ jacsId: 'agent-1', registered: true });
  });

  it('parses list-messages pagination arguments', async () => {
    const listMessages = vi.fn(async () => ([{ message_id: 'msg-1' }]));
    createMock.mockResolvedValue({ listMessages });
    const stdoutSpy = vi.spyOn(process.stdout, 'write').mockImplementation(() => true);

    const code = await main([
      'list-messages',
      '--limit',
      '5',
      '--direction',
      'outbound',
      '--url',
      'https://hai.example',
    ]);

    expect(code).toBe(0);
    expect(createMock).toHaveBeenCalledWith({ url: 'https://hai.example' });
    expect(listMessages).toHaveBeenCalledWith({
      limit: 5,
      direction: 'outbound',
    });
    expect(JSON.parse(joinWrites(stdoutSpy))).toEqual([{ message_id: 'msg-1' }]);
  });

  it('dispatches send-email with required fields', async () => {
    const sendEmail = vi.fn(async () => ({ message_id: 'msg-1' }));
    createMock.mockResolvedValue({ sendEmail });
    const stdoutSpy = vi.spyOn(process.stdout, 'write').mockImplementation(() => true);

    const code = await main([
      'send-email',
      '--to',
      'ops@hai.ai',
      '--subject',
      'Subject',
      '--body',
      'Body',
    ]);

    expect(code).toBe(0);
    expect(sendEmail).toHaveBeenCalledWith({
      to: 'ops@hai.ai',
      subject: 'Subject',
      body: 'Body',
    });
    expect(JSON.parse(joinWrites(stdoutSpy))).toEqual({ message_id: 'msg-1' });
  });

  it('registers a new agent from generated keys and writes config', async () => {
    const tempDir = await mkdtemp(join(tmpdir(), 'haisdk-cli-'));
    const keyDir = join(tempDir, 'keys');
    const configPath = join(tempDir, 'nested', 'jacs.config.json');
    const publicKeyPath = join(keyDir, 'jacs.public.pem');
    const privateKeyPath = join(keyDir, 'jacs.private.pem.enc');

    await mkdir(keyDir, { recursive: true });
    await writeFile(publicKeyPath, '-----BEGIN PUBLIC KEY-----\nPUB\n-----END PUBLIC KEY-----\n');
    await writeFile(privateKeyPath, 'PRIVATE-KEY-DATA');

    createAgentSyncMock.mockReturnValue(JSON.stringify({
      public_key_path: publicKeyPath,
      private_key_path: privateKeyPath,
    }));
    loadPrivateKeyPassphraseMock.mockResolvedValue('secret-password');

    const register = vi.fn(async () => ({ jacsId: 'agent-1', agentId: 'hai-1' }));
    fromCredentialsMock.mockResolvedValue({ register });
    vi.spyOn(process.stdout, 'write').mockImplementation(() => true);

    const code = await main([
      'register',
      '--name',
      'demo-agent',
      '--description',
      'Demo agent',
      '--dns',
      'agent.example',
      '--owner-email',
      'owner@hai.ai',
      '--key-dir',
      keyDir,
      '--config-path',
      configPath,
      '--url',
      'https://hai.example',
    ]);

    expect(code).toBe(0);
    expect(createAgentSyncMock).toHaveBeenCalledWith(
      'demo-agent',
      'secret-password',
      'pq2025',
      join(dirname(configPath), 'jacs_data'),
      keyDir,
      configPath,
      null,
      'Demo agent',
      'agent.example',
      null,
    );
    expect(fromCredentialsMock).toHaveBeenCalledWith('demo-agent', 'PRIVATE-KEY-DATA', {
      url: 'https://hai.example',
      privateKeyPassphrase: 'secret-password',
    });
    expect(register).toHaveBeenCalledWith({
      ownerEmail: 'owner@hai.ai',
      description: 'Demo agent',
      domain: 'agent.example',
      publicKeyPem: '-----BEGIN PUBLIC KEY-----\nPUB\n-----END PUBLIC KEY-----\n',
    });

    const savedConfig = JSON.parse(await readFile(configPath, 'utf-8')) as Record<string, string>;
    expect(savedConfig).toEqual({
      jacsAgentName: 'demo-agent',
      jacsAgentVersion: '1.0.0',
      jacsKeyDir: keyDir,
      jacsId: 'agent-1',
    });
  });

  it('returns an error for missing required command arguments', async () => {
    const stderrSpy = vi.spyOn(process.stderr, 'write').mockImplementation(() => true);

    const code = await main(['check-username']);

    expect(code).toBe(1);
    expect(joinWrites(stderrSpy)).toContain('Username is required (--username)');
    expect(createMock).not.toHaveBeenCalled();
  });
});
