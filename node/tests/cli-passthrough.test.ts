import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('../src/jacs.js', () => ({
  runJacsCli: vi.fn(),
}));

import { main, resolveJacsPassthroughArgs } from '../src/cli.js';
import { runJacsCli } from '../src/jacs.js';

const runJacsCliMock = vi.mocked(runJacsCli);

describe('CLI passthrough', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    runJacsCliMock.mockReset();
  });

  describe('resolveJacsPassthroughArgs', () => {
    it('returns null for known haisdk command', () => {
      expect(resolveJacsPassthroughArgs(['status'])).toBeNull();
    });

    it('forwards explicit jacs command', () => {
      expect(resolveJacsPassthroughArgs(['jacs', 'agent', 'lookup', 'example.com'])).toEqual([
        'agent',
        'lookup',
        'example.com',
      ]);
    });

    it('forwards unknown top-level command transparently', () => {
      expect(resolveJacsPassthroughArgs(['verify', 'signed.json'])).toEqual(['verify', 'signed.json']);
    });
  });

  describe('main', () => {
    it('forwards explicit `haisdk jacs ...` to JACS CLI', async () => {
      runJacsCliMock.mockReturnValue({ status: 0 } as never);

      const code = await main(['jacs', 'agent', 'lookup', 'example.com']);

      expect(code).toBe(0);
      expect(runJacsCliMock).toHaveBeenCalledWith(['agent', 'lookup', 'example.com'], { stdio: 'inherit' });
    });

    it('forwards unknown commands to JACS CLI', async () => {
      runJacsCliMock.mockReturnValue({ status: 7 } as never);

      const code = await main(['verify', 'signed.json']);

      expect(code).toBe(7);
      expect(runJacsCliMock).toHaveBeenCalledWith(['verify', 'signed.json'], { stdio: 'inherit' });
    });

    it('returns 1 and prints error when passthrough throws', async () => {
      runJacsCliMock.mockImplementation(() => {
        throw new Error('no jacs binary');
      });
      const stderrSpy = vi.spyOn(process.stderr, 'write').mockImplementation(() => true);

      const code = await main(['verify', 'signed.json']);

      expect(code).toBe(1);
      expect(stderrSpy).toHaveBeenCalled();
      expect(String(stderrSpy.mock.calls[0]?.[0] ?? '')).toContain('no jacs binary');
    });
  });
});
