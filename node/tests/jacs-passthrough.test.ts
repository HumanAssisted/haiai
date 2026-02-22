import { afterEach, describe, expect, it, vi } from 'vitest';

const spawnSyncMock = vi.fn();

vi.mock('node:child_process', () => ({
  spawnSync: spawnSyncMock,
}));

import { resolveJacsCliBin, runJacsCli } from '../src/jacs.js';

describe('jacs passthrough library helpers', () => {
  afterEach(() => {
    spawnSyncMock.mockReset();
  });

  describe('resolveJacsCliBin', () => {
    it('defaults to jacs when env is unset', () => {
      expect(resolveJacsCliBin({} as NodeJS.ProcessEnv)).toBe('jacs');
    });

    it('uses JACS_CLI_BIN when configured', () => {
      expect(resolveJacsCliBin({ JACS_CLI_BIN: '/custom/jacs' } as NodeJS.ProcessEnv)).toBe('/custom/jacs');
    });

    it('ignores empty JACS_CLI_BIN', () => {
      expect(resolveJacsCliBin({ JACS_CLI_BIN: '   ' } as NodeJS.ProcessEnv)).toBe('jacs');
    });
  });

  describe('runJacsCli', () => {
    it('invokes jacs with default options', () => {
      spawnSyncMock.mockReturnValue({ status: 0, error: undefined });

      const result = runJacsCli(['verify', 'signed.json']);

      expect(spawnSyncMock).toHaveBeenCalledOnce();
      expect(spawnSyncMock).toHaveBeenCalledWith('jacs', ['verify', 'signed.json'], {
        cwd: undefined,
        env: process.env,
        stdio: 'pipe',
        encoding: 'buffer',
      });
      expect(result.status).toBe(0);
    });

    it('supports overriding binary and stdio', () => {
      spawnSyncMock.mockReturnValue({ status: 2, error: undefined });
      const env = { ...process.env, JACS_CLI_BIN: '/env/ignored' };

      const result = runJacsCli(['agent', 'lookup', 'example.com'], {
        jacsBin: '/opt/bin/jacs',
        stdio: 'inherit',
        cwd: '/tmp',
        env,
      });

      expect(spawnSyncMock).toHaveBeenCalledWith('/opt/bin/jacs', ['agent', 'lookup', 'example.com'], {
        cwd: '/tmp',
        env,
        stdio: 'inherit',
        encoding: 'buffer',
      });
      expect(result.status).toBe(2);
    });

    it('throws a clear error when binary is missing', () => {
      spawnSyncMock.mockReturnValue({
        status: null,
        error: { code: 'ENOENT', message: 'not found' },
      });

      expect(() => runJacsCli(['verify'])).toThrow('JACS CLI binary not found: jacs');
    });

    it('throws a generic execution error for non-ENOENT failures', () => {
      spawnSyncMock.mockReturnValue({
        status: null,
        error: { code: 'EACCES', message: 'permission denied' },
      });

      expect(() => runJacsCli(['verify'], { jacsBin: '/custom/jacs' })).toThrow(
        "Failed to execute JACS CLI '/custom/jacs': permission denied",
      );
    });
  });
});
