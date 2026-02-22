import { describe, expect, it, vi } from 'vitest';

import {
  createAgentSdkToolWrapper,
  JacsModuleError,
  langchainSignedTool,
  verifyAgentSdkPayload,
} from '../src/integrations.js';

describe('integration wrappers', () => {
  it('returns a clear error when optional langchain integration is missing', async () => {
    await expect(langchainSignedTool({}, { client: {} })).rejects.toThrow(
      "Optional dependency '@hai.ai/jacs/langchain' is required",
    );
  });

  it('wraps Agent SDK tool output and signs it', async () => {
    const signer = {
      signMessage: vi.fn(async (payload: unknown) => ({
        raw: JSON.stringify(payload),
      })),
    };

    const wrapTool = createAgentSdkToolWrapper({ signer });
    const wrapped = wrapTool(async (topic: string) => ({ summary: topic }), 'summarize');
    const signed = await wrapped('safety');
    const parsed = JSON.parse(signed) as Record<string, unknown>;

    expect(signer.signMessage).toHaveBeenCalledOnce();
    expect(parsed.tool).toBe('summarize');
    expect(parsed.result).toEqual({ summary: 'safety' });
  });

  it('falls back to passthrough output when signing fails in permissive mode', async () => {
    const signer = {
      signMessage: vi.fn(async () => {
        throw new Error('sign failed');
      }),
    };
    const wrapTool = createAgentSdkToolWrapper({ signer, strict: false });
    const wrapped = wrapTool(() => ({ ok: true }), 'check');

    await expect(wrapped()).resolves.toBe('{"ok":true}');
  });

  it('raises signing failures in strict mode', async () => {
    const signer = {
      signMessage: vi.fn(async () => {
        throw new Error('boom');
      }),
    };
    const wrapTool = createAgentSdkToolWrapper({ signer, strict: true });
    const wrapped = wrapTool(() => 'hello');

    await expect(wrapped()).rejects.toThrow('boom');
  });

  it('verifies Agent SDK payload when signer supports verify', async () => {
    const signer = {
      signMessage: vi.fn(),
      verify: vi.fn(async () => ({ valid: true })),
    };

    await expect(verifyAgentSdkPayload(signer, '{"signed":true}')).resolves.toEqual({ valid: true });
    expect(signer.verify).toHaveBeenCalledWith('{"signed":true}');
  });

  it('returns payload when verify is unavailable in permissive mode', async () => {
    const signer = {
      signMessage: vi.fn(),
    };
    await expect(verifyAgentSdkPayload(signer, 'raw')).resolves.toBe('raw');
  });

  it('raises when verify is unavailable in strict mode', async () => {
    const signer = {
      signMessage: vi.fn(),
    };
    await expect(verifyAgentSdkPayload(signer, 'raw', { strict: true })).rejects.toThrow(
      JacsModuleError,
    );
  });
});
