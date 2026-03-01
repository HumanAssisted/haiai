import { afterEach, describe, expect, it, vi } from 'vitest';
import { HaiClient } from '../src/client.js';
import { generateKeypair } from '../src/crypt.js';
import {
  HaiApiError,
  EmailNotActiveError,
  RecipientNotFoundError,
  RateLimitedError,
} from '../src/errors.js';

function makeClient(jacsId: string = 'test-agent-001'): HaiClient {
  const keypair = generateKeypair();
  const client = HaiClient.fromCredentials(jacsId, keypair.privateKeyPem, { url: 'https://hai.example' });
  client.setAgentEmail(`${jacsId}@hai.ai`);
  return client;
}

function jsonResponse(body: Record<string, unknown>, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' },
  });
}

describe('sendEmail server-side signing', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('does not include jacs_signature or jacs_timestamp in send request body', async () => {
    const client = makeClient();
    let capturedBody: Record<string, unknown> | null = null;

    const fetchMock = vi.fn(async (_url: string | URL, init?: RequestInit) => {
      capturedBody = JSON.parse(init?.body as string);
      return jsonResponse({ message_id: 'msg-1', status: 'queued' });
    });
    vi.stubGlobal('fetch', fetchMock);

    await client.sendEmail({ to: 'bob@hai.ai', subject: 'Hello', body: 'World' });

    expect(capturedBody).not.toBeNull();
    expect(capturedBody!.to).toBe('bob@hai.ai');
    expect(capturedBody!.subject).toBe('Hello');
    expect(capturedBody!.body).toBe('World');
    // Server handles JACS signing -- client must NOT send these
    expect(capturedBody!.jacs_signature).toBeUndefined();
    expect(capturedBody!.jacs_timestamp).toBeUndefined();
  });
});

describe('getMessage', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('GETs the correct URL and returns parsed EmailMessage', async () => {
    const client = makeClient();
    const fetchMock = vi.fn(async (url: string | URL) => {
      expect(String(url)).toBe(
        'https://hai.example/api/agents/test-agent-001/email/messages/msg-abc',
      );
      return jsonResponse({
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
      });
    });
    vi.stubGlobal('fetch', fetchMock);

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
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('sends DELETE to the correct URL', async () => {
    const client = makeClient();
    const fetchMock = vi.fn(async (url: string | URL, init?: RequestInit) => {
      expect(String(url)).toBe(
        'https://hai.example/api/agents/test-agent-001/email/messages/msg-del',
      );
      expect(init?.method).toBe('DELETE');
      return jsonResponse({});
    });
    vi.stubGlobal('fetch', fetchMock);

    await client.deleteMessage('msg-del');
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});

describe('markUnread', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('POSTs to the unread endpoint', async () => {
    const client = makeClient();
    const fetchMock = vi.fn(async (url: string | URL, init?: RequestInit) => {
      expect(String(url)).toBe(
        'https://hai.example/api/agents/test-agent-001/email/messages/msg-unr/unread',
      );
      expect(init?.method).toBe('POST');
      return jsonResponse({});
    });
    vi.stubGlobal('fetch', fetchMock);

    await client.markUnread('msg-unr');
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});

describe('searchMessages', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('GETs search endpoint with query params and returns messages', async () => {
    const client = makeClient();
    const fetchMock = vi.fn(async (url: string | URL) => {
      const urlStr = String(url);
      expect(urlStr).toContain('/api/agents/test-agent-001/email/search?');
      expect(urlStr).toContain('q=hello');
      expect(urlStr).toContain('limit=5');
      expect(urlStr).toContain('direction=inbound');
      return jsonResponse({
        messages: [
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
        ],
      });
    });
    vi.stubGlobal('fetch', fetchMock);

    const results = await client.searchMessages({ query: 'hello', limit: 5, direction: 'inbound' });
    expect(results).toHaveLength(1);
    expect(results[0].id).toBe('msg-s1');
    expect(results[0].readAt).toBe('2026-02-24T01:00:00Z');
  });
});

describe('getUnreadCount', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('GETs unread-count endpoint and returns number', async () => {
    const client = makeClient();
    const fetchMock = vi.fn(async (url: string | URL) => {
      expect(String(url)).toBe(
        'https://hai.example/api/agents/test-agent-001/email/unread-count',
      );
      return jsonResponse({ count: 7 });
    });
    vi.stubGlobal('fetch', fetchMock);

    const count = await client.getUnreadCount();
    expect(count).toBe(7);
  });
});

describe('reply', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('fetches original message then sends reply with Re: subject', async () => {
    const client = makeClient();
    let callCount = 0;

    const fetchMock = vi.fn(async (url: string | URL, init?: RequestInit) => {
      callCount++;
      const urlStr = String(url);

      if (callCount === 1) {
        // getMessage call
        expect(urlStr).toContain('/email/messages/msg-orig');
        expect(init?.method).toBe('GET');
        return jsonResponse({
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
        });
      }

      // sendEmail call
      expect(urlStr).toContain('/email/send');
      expect(init?.method).toBe('POST');
      const body = JSON.parse(init?.body as string);
      expect(body.to).toBe('alice@hai.ai');
      expect(body.subject).toBe('Re: Original Subject');
      expect(body.body).toBe('Thanks!');
      expect(body.in_reply_to).toBe('<msg-orig@hai.ai>');
      return jsonResponse({ message_id: 'msg-reply', status: 'queued' });
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await client.reply('msg-orig', 'Thanks!');
    expect(result.messageId).toBe('msg-reply');
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('uses subjectOverride when provided', async () => {
    const client = makeClient();
    let callCount = 0;

    const fetchMock = vi.fn(async (_url: string | URL, init?: RequestInit) => {
      callCount++;
      if (callCount === 1) {
        return jsonResponse({
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
        });
      }
      const body = JSON.parse(init?.body as string);
      expect(body.subject).toBe('Custom Subject');
      return jsonResponse({ message_id: 'msg-reply-2', status: 'queued' });
    });
    vi.stubGlobal('fetch', fetchMock);

    const result = await client.reply('msg-orig', 'body', 'Custom Subject');
    expect(result.messageId).toBe('msg-reply-2');
  });
});

describe('email method path escaping', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('escapes special characters in messageId for getMessage', async () => {
    const client = makeClient('agent/special');
    const fetchMock = vi.fn(async (url: string | URL) => {
      expect(String(url)).toBe(
        'https://hai.example/api/agents/agent%2Fspecial/email/messages/msg%2F..%2Fhack',
      );
      return jsonResponse({
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
      });
    });
    vi.stubGlobal('fetch', fetchMock);

    await client.getMessage('msg/../hack');
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});

describe('sendEmail error codes', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('throws EmailNotActiveError when error_code is EMAIL_NOT_ACTIVE', async () => {
    const client = makeClient();
    const fetchMock = vi.fn(async () => {
      return new Response(
        JSON.stringify({
          error: 'Agent email is allocated and cannot send messages',
          error_code: 'EMAIL_NOT_ACTIVE',
          status: 403,
        }),
        { status: 403, headers: { 'Content-Type': 'application/json' } },
      );
    });
    vi.stubGlobal('fetch', fetchMock);

    await expect(
      client.sendEmail({ to: 'bob@hai.ai', subject: 'Hi', body: 'test' }),
    ).rejects.toThrow(EmailNotActiveError);

    try {
      await client.sendEmail({ to: 'bob@hai.ai', subject: 'Hi', body: 'test' });
    } catch (err) {
      expect(err).toBeInstanceOf(EmailNotActiveError);
      expect((err as EmailNotActiveError).errorCode).toBe('EMAIL_NOT_ACTIVE');
    }
  });

  it('throws RecipientNotFoundError when error_code is RECIPIENT_NOT_FOUND', async () => {
    const client = makeClient();
    const fetchMock = vi.fn(async () => {
      return new Response(
        JSON.stringify({
          error: 'Invalid recipient',
          error_code: 'RECIPIENT_NOT_FOUND',
          status: 400,
        }),
        { status: 400, headers: { 'Content-Type': 'application/json' } },
      );
    });
    vi.stubGlobal('fetch', fetchMock);

    await expect(
      client.sendEmail({ to: 'bob@hai.ai', subject: 'Hi', body: 'test' }),
    ).rejects.toThrow(RecipientNotFoundError);
  });

  it('throws RateLimitedError when error_code is RATE_LIMITED', async () => {
    const client = makeClient();
    const fetchMock = vi.fn(async () => {
      return new Response(
        JSON.stringify({
          error: 'Daily limit reached',
          error_code: 'RATE_LIMITED',
          status: 429,
        }),
        { status: 429, headers: { 'Content-Type': 'application/json' } },
      );
    });
    vi.stubGlobal('fetch', fetchMock);

    await expect(
      client.sendEmail({ to: 'bob@hai.ai', subject: 'Hi', body: 'test' }),
    ).rejects.toThrow(RateLimitedError);
  });

  it('throws HaiApiError for unknown error_code', async () => {
    const client = makeClient();
    const fetchMock = vi.fn(async () => {
      return new Response(
        JSON.stringify({
          error: 'Something else',
          error_code: 'UNKNOWN_CODE',
          status: 400,
        }),
        { status: 400, headers: { 'Content-Type': 'application/json' } },
      );
    });
    vi.stubGlobal('fetch', fetchMock);

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
      expect((err as HaiApiError).errorCode).toBe('UNKNOWN_CODE');
    }
  });
});

describe('sendEmail with attachments', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('includes attachments as base64 without client-side signing', async () => {
    const client = makeClient('att-agent');
    let capturedBody: Record<string, unknown> | null = null;

    const fetchMock = vi.fn(async (_url: string | URL, init?: RequestInit) => {
      capturedBody = JSON.parse(init?.body as string);
      return jsonResponse({ message_id: 'msg-att-1', status: 'queued' });
    });
    vi.stubGlobal('fetch', fetchMock);

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

    expect(capturedBody).not.toBeNull();
    const payloadAtts = capturedBody!.attachments as Array<Record<string, string>>;
    expect(payloadAtts).toHaveLength(2);
    expect(payloadAtts[0].data_base64).toBeDefined();
    expect(payloadAtts[1].data_base64).toBeDefined();
    // Server handles JACS signing -- client must NOT send these
    expect(capturedBody!.jacs_signature).toBeUndefined();
    expect(capturedBody!.jacs_timestamp).toBeUndefined();
  });

  it('attachment data is base64 encoded in payload', async () => {
    const client = makeClient();
    let capturedBody: Record<string, unknown> | null = null;

    const fetchMock = vi.fn(async (_url: string | URL, init?: RequestInit) => {
      capturedBody = JSON.parse(init?.body as string);
      return jsonResponse({ message_id: 'msg-b64', status: 'queued' });
    });
    vi.stubGlobal('fetch', fetchMock);

    const attachments = [
      { filename: 'hello.txt', contentType: 'text/plain', data: Buffer.from('Hello World') },
      { filename: 'binary.bin', contentType: 'application/octet-stream', data: Buffer.from([0x00, 0x01, 0xff]) },
    ];

    await client.sendEmail({
      to: 'bob@hai.ai', subject: 'B64 Test', body: 'body',
      attachments,
    });

    const payloadAtts = capturedBody!.attachments as Array<Record<string, string>>;
    expect(payloadAtts).toHaveLength(2);

    // Verify each attachment decodes back to original data
    for (let i = 0; i < attachments.length; i++) {
      const decoded = Buffer.from(payloadAtts[i].data_base64, 'base64');
      expect(decoded).toEqual(attachments[i].data);
    }
  });

  it('throws when agentEmail not set', async () => {
    const keypair = generateKeypair();
    const client = HaiClient.fromCredentials('no-email-agent', keypair.privateKeyPem, {
      url: 'https://hai.example',
    });
    // Do NOT call setAgentEmail

    await expect(
      client.sendEmail({ to: 'bob@hai.ai', subject: 'Hi', body: 'test' }),
    ).rejects.toThrow('agent email not set');
  });
});

