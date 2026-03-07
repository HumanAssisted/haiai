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
    expect(toolNames).toContain('hai_generate_verify_link');
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

  it('passes owner_email through registration calls', async () => {
    const register = vi.fn(async () => ({ jacs_id: 'agent-1' }));
    createMock.mockResolvedValue({ register });

    const result = await handleToolCall('hai_register_agent', {
      owner_email: 'owner@hai.ai',
      config_path: '/tmp/jacs.config.json',
    });

    expect(createMock).toHaveBeenCalledWith({
      configPath: '/tmp/jacs.config.json',
    });
    expect(register).toHaveBeenCalledWith({ ownerEmail: 'owner@hai.ai' });
    expect(JSON.parse(result.content[0].text)).toEqual({ jacs_id: 'agent-1' });
  });

  it('maps search filters to HaiClient.searchMessages fields', async () => {
    const searchMessages = vi.fn(async () => ([{ message_id: 'msg-2' }]));
    createMock.mockResolvedValue({ searchMessages });

    const result = await handleToolCall('hai_search_messages', {
      q: 'subject',
      direction: 'outbound',
      from_address: 'sender@hai.ai',
      to_address: 'dest@hai.ai',
      limit: 10,
      offset: 5,
    });

    expect(searchMessages).toHaveBeenCalledWith({
      query: 'subject',
      limit: 10,
      offset: 5,
      direction: 'outbound',
      fromAddress: 'sender@hai.ai',
      toAddress: 'dest@hai.ai',
    });
    expect(JSON.parse(result.content[0].text)).toEqual([{ message_id: 'msg-2' }]);
  });

  it('returns synthetic payloads for delete and read state tools', async () => {
    const deleteMessage = vi.fn(async () => undefined);
    const markRead = vi.fn(async () => undefined);
    const markUnread = vi.fn(async () => undefined);
    createMock.mockResolvedValue({ deleteMessage, markRead, markUnread });

    const deleted = await handleToolCall('hai_delete_message', { message_id: 'msg-3' });
    const read = await handleToolCall('hai_mark_read', { message_id: 'msg-3' });
    const unread = await handleToolCall('hai_mark_unread', { message_id: 'msg-3' });

    expect(deleteMessage).toHaveBeenCalledWith('msg-3');
    expect(markRead).toHaveBeenCalledWith('msg-3');
    expect(markUnread).toHaveBeenCalledWith('msg-3');
    expect(JSON.parse(deleted.content[0].text)).toEqual({ deleted: true, message_id: 'msg-3' });
    expect(JSON.parse(read.content[0].text)).toEqual({ message_id: 'msg-3', is_read: true });
    expect(JSON.parse(unread.content[0].text)).toEqual({ message_id: 'msg-3', is_read: false });
  });

  it('passes reply subject override through to HaiClient.reply', async () => {
    const reply = vi.fn(async () => ({ message_id: 'reply-1' }));
    createMock.mockResolvedValue({ reply });

    const result = await handleToolCall('hai_reply_email', {
      message_id: 'msg-4',
      body: 'Reply body',
      subject_override: 'Custom subject',
    });

    expect(reply).toHaveBeenCalledWith('msg-4', 'Reply body', 'Custom subject');
    expect(JSON.parse(result.content[0].text)).toEqual({ message_id: 'reply-1' });
  });

  it('wraps verify-link generation results', async () => {
    createMock.mockResolvedValue({});

    const result = await handleToolCall('hai_generate_verify_link', {
      document: '{"signed":true}',
      base_url: 'https://hai.example',
    });

    expect(JSON.parse(result.content[0].text)).toEqual({
      verify_url: 'https://hai.example/jacs/verify?s=eyJzaWduZWQiOnRydWV9',
    });
  });

  it('returns an MCP error payload for unknown tools', async () => {
    createMock.mockResolvedValue({});

    const result = await handleToolCall('hai_missing_tool', {});

    expect(result.isError).toBe(true);
    expect(result.content[0].text).toContain('unknown tool');
  });
});
