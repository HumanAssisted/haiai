import { afterEach, describe, expect, it, vi } from 'vitest';

const { createMock } = vi.hoisted(() => ({
  createMock: vi.fn(),
}));

vi.mock('../src/client.js', () => ({
  HaiClient: {
    create: createMock,
  },
}));

import { TOOLS, handleToolCall } from '../src/mcp-server.js';

describe('mcp server tool dispatch', () => {
  afterEach(() => {
    createMock.mockReset();
  });

  it('publishes core HAI tools', () => {
    const toolNames = TOOLS.map((tool) => tool.name);
    expect(toolNames).toContain('hai_hello');
    expect(toolNames).toContain('hai_send_email');
    expect(toolNames).toContain('hai_reply_email');
  });

  it('routes tool calls through HaiClient.create with config and URL overrides', async () => {
    const sendEmail = vi.fn(async () => ({ message_id: 'msg-1' }));
    createMock.mockResolvedValue({
      sendEmail,
    });

    const result = await handleToolCall('hai_send_email', {
      to: 'ops@hai.ai',
      subject: 'subject',
      body: 'body',
      config_path: '/tmp/jacs.config.json',
      hai_url: 'https://hai.example',
    });

    expect(createMock).toHaveBeenCalledWith({
      configPath: '/tmp/jacs.config.json',
      url: 'https://hai.example',
    });
    expect(sendEmail).toHaveBeenCalledWith({
      to: 'ops@hai.ai',
      subject: 'subject',
      body: 'body',
      inReplyTo: undefined,
    });
    expect(JSON.parse(result.content[0].text)).toEqual({ message_id: 'msg-1' });
  });

  it('returns an MCP error payload for unknown tools', async () => {
    createMock.mockResolvedValue({});

    const result = await handleToolCall('hai_missing_tool', {});

    expect(result.isError).toBe(true);
    expect(result.content[0].text).toContain('unknown tool');
  });
});
