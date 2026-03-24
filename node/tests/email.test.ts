import { afterEach, describe, expect, it, vi } from 'vitest';
import { HaiClient } from '../src/client.js';
import { generateTestKeypair as generateKeypair } from './setup.js';
import { createMockFFI } from './ffi-mock.js';
import {
  HaiApiError,
  EmailNotActiveError,
  RecipientNotFoundError,
  RateLimitedError,
} from '../src/errors.js';
import { mapFFIError } from '../src/ffi-client.js';

async function makeClient(jacsId: string = 'test-agent-001'): Promise<HaiClient> {
  const keypair = generateKeypair();
  const client = await HaiClient.fromCredentials(jacsId, keypair.privateKeyPem, { url: 'https://hai.example', privateKeyPassphrase: 'keygen-password' });
  client.setAgentEmail(`${jacsId}@hai.ai`);
  return client;
}

describe('sendEmail server-side signing', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('does not include jacs_signature or jacs_timestamp in send request body', async () => {
    const client = await makeClient();
    let capturedOptions: Record<string, unknown> | null = null;

    const sendEmailMock = vi.fn(async (options: Record<string, unknown>) => {
      capturedOptions = options;
      return { message_id: 'msg-1', status: 'queued' };
    });
    client._setFFIAdapter(createMockFFI({ sendEmail: sendEmailMock }));

    await client.sendEmail({ to: 'bob@hai.ai', subject: 'Hello', body: 'World' });

    expect(capturedOptions).not.toBeNull();
    expect(capturedOptions!.to).toBe('bob@hai.ai');
    expect(capturedOptions!.subject).toBe('Hello');
    expect(capturedOptions!.body).toBe('World');
    // Server handles JACS signing -- client must NOT send these
    expect(capturedOptions!.jacs_signature).toBeUndefined();
    expect(capturedOptions!.jacs_timestamp).toBeUndefined();
  });
});

describe('getMessage', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('returns parsed EmailMessage from FFI', async () => {
    const client = await makeClient();
    const getMessageMock = vi.fn(async (messageId: string) => {
      expect(messageId).toBe('msg-abc');
      return {
        id: 'msg-abc',
        direction: 'inbound',
        from_address: 'alice@hai.ai',
        to_address: 'bob@hai.ai',
        subject: 'Hi',
        body_text: 'Hello!',
        message_id: '<msg-abc@hai.ai>',
        in_reply_to: null,
        is_read: false,
        delivery_status: 'delivered',
        created_at: '2026-02-24T00:00:00Z',
        read_at: null,
        jacs_verified: true,
      };
    });
    client._setFFIAdapter(createMockFFI({ getMessage: getMessageMock }));

    const msg = await client.getMessage('msg-abc');
    expect(msg.id).toBe('msg-abc');
    expect(msg.direction).toBe('inbound');
    expect(msg.fromAddress).toBe('alice@hai.ai');
    expect(msg.toAddress).toBe('bob@hai.ai');
    expect(msg.subject).toBe('Hi');
    expect(msg.bodyText).toBe('Hello!');
    expect(msg.messageId).toBe('<msg-abc@hai.ai>');
    expect(msg.isRead).toBe(false);
    expect(msg.deliveryStatus).toBe('delivered');
    expect(msg.readAt).toBeNull();
    expect(msg.jacsVerified).toBe(true);
  });
});

describe('deleteMessage', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('delegates to FFI deleteMessage', async () => {
    const client = await makeClient();
    const deleteMessageMock = vi.fn(async (messageId: string) => {
      expect(messageId).toBe('msg-del');
    });
    client._setFFIAdapter(createMockFFI({ deleteMessage: deleteMessageMock }));

    await client.deleteMessage('msg-del');
    expect(deleteMessageMock).toHaveBeenCalledTimes(1);
  });
});

describe('markUnread', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('delegates to FFI markUnread', async () => {
    const client = await makeClient();
    const markUnreadMock = vi.fn(async (messageId: string) => {
      expect(messageId).toBe('msg-unr');
    });
    client._setFFIAdapter(createMockFFI({ markUnread: markUnreadMock }));

    await client.markUnread('msg-unr');
    expect(markUnreadMock).toHaveBeenCalledTimes(1);
  });
});

describe('searchMessages', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('passes search options to FFI and returns messages', async () => {
    const client = await makeClient();
    const searchMessagesMock = vi.fn(async (options: Record<string, unknown>) => {
      expect(options.query).toBe('hello');
      expect(options.limit).toBe(5);
      expect(options.direction).toBe('inbound');
      return [
        {
          id: 'msg-s1',
          direction: 'inbound',
          from_address: 'alice@hai.ai',
          to_address: 'bob@hai.ai',
          subject: 'hello world',
          body_text: 'content',
          message_id: '<msg-s1@hai.ai>',
          in_reply_to: null,
          is_read: true,
          delivery_status: 'delivered',
          created_at: '2026-02-24T00:00:00Z',
          read_at: '2026-02-24T01:00:00Z',
          jacs_verified: false,
        },
      ];
    });
    client._setFFIAdapter(createMockFFI({ searchMessages: searchMessagesMock }));

    const results = await client.searchMessages({ query: 'hello', limit: 5, direction: 'inbound' });
    expect(results).toHaveLength(1);
    expect(results[0].id).toBe('msg-s1');
    expect(results[0].readAt).toBe('2026-02-24T01:00:00Z');
  });
});

describe('getUnreadCount', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('returns unread count from FFI', async () => {
    const client = await makeClient();
    const getUnreadCountMock = vi.fn(async () => 7);
    client._setFFIAdapter(createMockFFI({ getUnreadCount: getUnreadCountMock }));

    const count = await client.getUnreadCount();
    expect(count).toBe(7);
  });
});

describe('reply', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('fetches original message then sends reply with Re: subject', async () => {
    const client = await makeClient();
    const getMessageMock = vi.fn(async () => ({
      id: 'msg-orig',
      direction: 'inbound',
      from_address: 'alice@hai.ai',
      to_address: 'bob@hai.ai',
      subject: 'Original Subject',
      body_text: 'Original body',
      message_id: '<msg-orig@hai.ai>',
      in_reply_to: null,
      is_read: false,
      delivery_status: 'delivered',
      created_at: '2026-02-24T00:00:00Z',
      read_at: null,
      jacs_verified: true,
    }));
    const sendEmailMock = vi.fn(async (options: Record<string, unknown>) => {
      expect(options.to).toBe('alice@hai.ai');
      expect(options.subject).toBe('Re: Original Subject');
      expect(options.body).toBe('Thanks!');
      expect(options.in_reply_to).toBe('<msg-orig@hai.ai>');
      return { message_id: 'msg-reply', status: 'queued' };
    });
    client._setFFIAdapter(createMockFFI({ getMessage: getMessageMock, sendEmail: sendEmailMock }));

    const result = await client.reply('msg-orig', 'Thanks!');
    expect(result.messageId).toBe('msg-reply');
    expect(getMessageMock).toHaveBeenCalledTimes(1);
    expect(sendEmailMock).toHaveBeenCalledTimes(1);
  });

  it('uses subjectOverride when provided', async () => {
    const client = await makeClient();
    const getMessageMock = vi.fn(async () => ({
      id: 'msg-orig',
      direction: 'inbound',
      from_address: 'alice@hai.ai',
      to_address: 'bob@hai.ai',
      subject: 'Original',
      body_text: 'body',
      message_id: '<msg-orig@hai.ai>',
      in_reply_to: null,
      is_read: false,
      delivery_status: 'delivered',
      created_at: '2026-02-24T00:00:00Z',
      read_at: null,
      jacs_verified: false,
    }));
    const sendEmailMock = vi.fn(async (options: Record<string, unknown>) => {
      expect(options.subject).toBe('Custom Subject');
      return { message_id: 'msg-reply-2', status: 'queued' };
    });
    client._setFFIAdapter(createMockFFI({ getMessage: getMessageMock, sendEmail: sendEmailMock }));

    const result = await client.reply('msg-orig', 'body', 'Custom Subject');
    expect(result.messageId).toBe('msg-reply-2');
  });
});

describe('email method path escaping', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('passes raw messageId to FFI getMessage (Rust handles escaping)', async () => {
    const client = await makeClient('agent/special');
    const getMessageMock = vi.fn(async (messageId: string) => {
      expect(messageId).toBe('msg/../hack');
      return {
        id: 'msg/../hack',
        direction: 'inbound',
        from_address: 'a@hai.ai',
        to_address: 'b@hai.ai',
        subject: 's',
        body_text: 'b',
        message_id: '',
        in_reply_to: null,
        is_read: false,
        delivery_status: '',
        created_at: '',
        read_at: null,
        jacs_verified: false,
      };
    });
    client._setFFIAdapter(createMockFFI({ getMessage: getMessageMock }));

    await client.getMessage('msg/../hack');
    expect(getMessageMock).toHaveBeenCalledTimes(1);
  });
});

describe('sendEmail error codes', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('throws EmailNotActiveError when FFI throws NotFound with email not active', async () => {
    const client = await makeClient();
    const sendEmailMock = vi.fn(async () => {
      throw mapFFIError(new Error('NotFound: email not active'));
    });
    client._setFFIAdapter(createMockFFI({ sendEmail: sendEmailMock }));

    await expect(
      client.sendEmail({ to: 'bob@hai.ai', subject: 'Hi', body: 'test' }),
    ).rejects.toThrow(EmailNotActiveError);
  });

  it('throws RecipientNotFoundError when FFI throws NotFound with recipient', async () => {
    const client = await makeClient();
    const sendEmailMock = vi.fn(async () => {
      throw mapFFIError(new Error('NotFound: Invalid recipient'));
    });
    client._setFFIAdapter(createMockFFI({ sendEmail: sendEmailMock }));

    await expect(
      client.sendEmail({ to: 'bob@hai.ai', subject: 'Hi', body: 'test' }),
    ).rejects.toThrow(RecipientNotFoundError);
  });

  it('throws RateLimitedError when FFI throws RateLimited', async () => {
    const client = await makeClient();
    const sendEmailMock = vi.fn(async () => {
      throw mapFFIError(new Error('RateLimited: Daily limit reached'));
    });
    client._setFFIAdapter(createMockFFI({ sendEmail: sendEmailMock }));

    await expect(
      client.sendEmail({ to: 'bob@hai.ai', subject: 'Hi', body: 'test' }),
    ).rejects.toThrow(RateLimitedError);
  });

  it('throws HaiApiError for unknown API errors', async () => {
    const client = await makeClient();
    const sendEmailMock = vi.fn(async () => {
      throw mapFFIError(new Error('ApiError: Something else'));
    });
    client._setFFIAdapter(createMockFFI({ sendEmail: sendEmailMock }));

    await expect(
      client.sendEmail({ to: 'bob@hai.ai', subject: 'Hi', body: 'test' }),
    ).rejects.toThrow(HaiApiError);

    try {
      await client.sendEmail({ to: 'bob@hai.ai', subject: 'Hi', body: 'test' });
    } catch (err) {
      expect(err).toBeInstanceOf(HaiApiError);
      // Ensure it's not a specific subclass
      expect(err).not.toBeInstanceOf(EmailNotActiveError);
      expect(err).not.toBeInstanceOf(RecipientNotFoundError);
      expect(err).not.toBeInstanceOf(RateLimitedError);
    }
  });
});

describe('sendEmail with attachments', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('includes attachments as base64 without client-side signing', async () => {
    const client = await makeClient('att-agent');
    let capturedOptions: Record<string, unknown> | null = null;

    const sendEmailMock = vi.fn(async (options: Record<string, unknown>) => {
      capturedOptions = options;
      return { message_id: 'msg-att-1', status: 'queued' };
    });
    client._setFFIAdapter(createMockFFI({ sendEmail: sendEmailMock }));

    const attachments = [
      { filename: 'b.txt', contentType: 'text/plain', data: Buffer.from('bravo') },
      { filename: 'a.txt', contentType: 'text/plain', data: Buffer.from('alpha') },
    ];

    await client.sendEmail({
      to: 'bob@hai.ai',
      subject: 'With Attachments',
      body: 'See attached',
      attachments,
    });

    expect(capturedOptions).not.toBeNull();
    const payloadAtts = capturedOptions!.attachments as Array<Record<string, string>>;
    expect(payloadAtts).toHaveLength(2);
    expect(payloadAtts[0].data_base64).toBeDefined();
    expect(payloadAtts[1].data_base64).toBeDefined();
    // Server handles JACS signing -- client must NOT send these
    expect(capturedOptions!.jacs_signature).toBeUndefined();
    expect(capturedOptions!.jacs_timestamp).toBeUndefined();
  });

  it('attachment data is base64 encoded in payload', async () => {
    const client = await makeClient();
    let capturedOptions: Record<string, unknown> | null = null;

    const sendEmailMock = vi.fn(async (options: Record<string, unknown>) => {
      capturedOptions = options;
      return { message_id: 'msg-b64', status: 'queued' };
    });
    client._setFFIAdapter(createMockFFI({ sendEmail: sendEmailMock }));

    const attachments = [
      { filename: 'hello.txt', contentType: 'text/plain', data: Buffer.from('Hello World') },
      { filename: 'binary.bin', contentType: 'application/octet-stream', data: Buffer.from([0x00, 0x01, 0xff]) },
    ];

    await client.sendEmail({
      to: 'bob@hai.ai', subject: 'B64 Test', body: 'body',
      attachments,
    });

    const payloadAtts = capturedOptions!.attachments as Array<Record<string, string>>;
    expect(payloadAtts).toHaveLength(2);

    // Verify each attachment decodes back to original data
    for (let i = 0; i < attachments.length; i++) {
      const decoded = Buffer.from(payloadAtts[i].data_base64, 'base64');
      expect(decoded).toEqual(attachments[i].data);
    }
  });

  it('throws when agentEmail not set', async () => {
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials('no-email-agent', keypair.privateKeyPem, {
      url: 'https://hai.example',
      privateKeyPassphrase: 'keygen-password',
    });
    // Do NOT call setAgentEmail

    await expect(
      client.sendEmail({ to: 'bob@hai.ai', subject: 'Hi', body: 'test' }),
    ).rejects.toThrow('agent email not set');
  });
});

describe('sendSignedEmail', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('delegates to sendEmail (deprecated, TASK_017)', async () => {
    const client = await makeClient();
    let capturedOptions: Record<string, unknown> | null = null;

    const sendEmailMock = vi.fn(async (options: Record<string, unknown>) => {
      capturedOptions = options;
      return { message_id: 'msg-signed-1', status: 'sent' };
    });
    client._setFFIAdapter(createMockFFI({ sendEmail: sendEmailMock }));

    const result = await client.sendSignedEmail({
      to: 'bob@hai.ai',
      subject: 'Hello Signed',
      body: 'Signed body',
    });

    expect(result.messageId).toBe('msg-signed-1');
    expect(result.status).toBe('sent');
    // Delegates to sendEmail which uses the FFI sendEmail
    expect(capturedOptions!.to).toBe('bob@hai.ai');
    expect(capturedOptions!.subject).toBe('Hello Signed');
  });

  it('throws when agentEmail not set', async () => {
    const keypair = generateKeypair();
    const client = await HaiClient.fromCredentials('no-email-agent', keypair.privateKeyPem, {
      url: 'https://hai.example',
      privateKeyPassphrase: 'keygen-password',
    });
    // Do NOT call setAgentEmail

    await expect(
      client.sendSignedEmail({ to: 'bob@hai.ai', subject: 'Hi', body: 'test' }),
    ).rejects.toThrow('agent email not set');
  });
});

// ---------------------------------------------------------------------------
// getContacts
// ---------------------------------------------------------------------------

describe('getContacts', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('parses wrapped contacts response with all fields', async () => {
    const client = await makeClient();
    const contactsMock = vi.fn(async () => [
      {
        email: 'alice@hai.ai',
        display_name: 'Alice Agent',
        last_contact: '2026-03-13T10:00:00+00:00',
        jacs_verified: true,
        reputation_tier: 'established',
      },
      {
        email: 'external@example.com',
        last_contact: '2026-03-12T08:00:00+00:00',
        jacs_verified: false,
      },
    ]);
    client._setFFIAdapter(createMockFFI({ contacts: contactsMock }));

    const contacts = await client.getContacts();
    expect(contacts).toHaveLength(2);
    expect(contacts[0].email).toBe('alice@hai.ai');
    expect(contacts[0].displayName).toBe('Alice Agent');
    expect(contacts[0].lastContact).toBe('2026-03-13T10:00:00+00:00');
    expect(contacts[0].jacsVerified).toBe(true);
    expect(contacts[0].reputationTier).toBe('established');
    expect(contacts[1].email).toBe('external@example.com');
    expect(contacts[1].jacsVerified).toBe(false);
    expect(contacts[1].reputationTier).toBeUndefined();
    expect(contacts[1].displayName).toBeUndefined();
  });

  it('handles bare array response', async () => {
    const client = await makeClient();
    const contactsMock = vi.fn(async () => [
      { email: 'alice@hai.ai', last_contact: '2026-01-01T00:00:00Z', jacs_verified: false },
    ]);
    client._setFFIAdapter(createMockFFI({ contacts: contactsMock }));

    const contacts = await client.getContacts();
    expect(contacts).toHaveLength(1);
    expect(contacts[0].email).toBe('alice@hai.ai');
  });

  it('returns empty array when no contacts', async () => {
    const client = await makeClient();
    const contactsMock = vi.fn(async () => []);
    client._setFFIAdapter(createMockFFI({ contacts: contactsMock }));

    const contacts = await client.getContacts();
    expect(contacts).toHaveLength(0);
  });
});

describe('getEmailStatus nested fields', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('parses volume, delivery, and reputation from response', async () => {
    const client = await makeClient();
    const getEmailStatusMock = vi.fn(async () => ({
      email: 'bot@hai.ai',
      status: 'active',
      tier: 'established',
      billing_tier: 'pro',
      messages_sent_24h: 10,
      daily_limit: 100,
      daily_used: 10,
      resets_at: '2026-03-15T00:00:00Z',
      messages_sent_total: 500,
      external_enabled: true,
      external_sends_today: 3,
      last_tier_change: '2026-01-01T00:00:00Z',
      volume: {
        sent_total: 500,
        received_total: 300,
        sent_24h: 10,
      },
      delivery: {
        bounce_count: 2,
        spam_report_count: 1,
        delivery_rate: 0.98,
      },
      reputation: {
        score: 85.5,
        tier: 'established',
        email_score: 90.0,
        hai_score: 80.0,
      },
    }));
    client._setFFIAdapter(createMockFFI({ getEmailStatus: getEmailStatusMock }));

    const status = await client.getEmailStatus();

    expect(status.email).toBe('bot@hai.ai');
    expect(status.tier).toBe('established');

    // Volume
    expect(status.volume).not.toBeNull();
    expect(status.volume!.sentTotal).toBe(500);
    expect(status.volume!.receivedTotal).toBe(300);
    expect(status.volume!.sent24h).toBe(10);

    // Delivery
    expect(status.delivery).not.toBeNull();
    expect(status.delivery!.bounceCount).toBe(2);
    expect(status.delivery!.spamReportCount).toBe(1);
    expect(status.delivery!.deliveryRate).toBe(0.98);

    // Reputation
    expect(status.reputation).not.toBeNull();
    expect(status.reputation!.score).toBe(85.5);
    expect(status.reputation!.tier).toBe('established');
    expect(status.reputation!.emailScore).toBe(90.0);
    expect(status.reputation!.haiScore).toBe(80.0);
  });

  it('returns null for nested fields when absent', async () => {
    const client = await makeClient();
    const getEmailStatusMock = vi.fn(async () => ({
      email: 'bot@hai.ai',
      status: 'active',
      tier: 'new',
      billing_tier: 'free',
      messages_sent_24h: 0,
      daily_limit: 10,
      daily_used: 0,
      resets_at: '2026-03-15T00:00:00Z',
      messages_sent_total: 0,
      external_enabled: false,
      external_sends_today: 0,
      last_tier_change: null,
    }));
    client._setFFIAdapter(createMockFFI({ getEmailStatus: getEmailStatusMock }));

    const status = await client.getEmailStatus();

    expect(status.volume).toBeNull();
    expect(status.delivery).toBeNull();
    expect(status.reputation).toBeNull();
  });

  it('handles hai_score null in reputation', async () => {
    const client = await makeClient();
    const getEmailStatusMock = vi.fn(async () => ({
      email: 'bot@hai.ai',
      status: 'active',
      tier: 'new',
      billing_tier: 'free',
      messages_sent_24h: 0,
      daily_limit: 10,
      daily_used: 0,
      resets_at: '2026-03-15T00:00:00Z',
      messages_sent_total: 0,
      external_enabled: false,
      external_sends_today: 0,
      last_tier_change: null,
      reputation: {
        score: 50.0,
        tier: 'new',
        email_score: 50.0,
        hai_score: null,
      },
    }));
    client._setFFIAdapter(createMockFFI({ getEmailStatus: getEmailStatusMock }));

    const status = await client.getEmailStatus();

    expect(status.reputation).not.toBeNull();
    expect(status.reputation!.haiScore).toBeNull();
  });
});
