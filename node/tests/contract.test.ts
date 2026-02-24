import { afterEach, describe, expect, it, vi } from 'vitest';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { HaiClient } from '../src/client.js';
import { generateKeypair } from '../src/crypt.js';
import type { EmailMessage, EmailStatus, KeyRegistryResponse, EmailVerificationResult } from '../src/types.js';

interface EndpointContract {
  method: string;
  path: string;
  auth_required: boolean;
}

interface ContractFixture {
  base_url: string;
  hello: EndpointContract;
  check_username: EndpointContract;
  submit_response: EndpointContract;
}

function loadContractFixture(): ContractFixture {
  const here = dirname(fileURLToPath(import.meta.url));
  const fixturePath = resolve(here, '../../fixtures/contract_endpoints.json');
  return JSON.parse(readFileSync(fixturePath, 'utf-8')) as ContractFixture;
}

function makeClient(baseUrl: string): HaiClient {
  const keypair = generateKeypair();
  return HaiClient.fromCredentials('test-agent-001', keypair.privateKeyPem, { url: baseUrl });
}

describe('mock API contract (node)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('hello uses the shared method/path/auth contract', async () => {
    const contract = loadContractFixture();
    const client = makeClient(contract.base_url);

    const fetchMock = vi.fn(async (url: string | URL, init?: RequestInit) => {
      expect(String(url)).toBe(`${contract.base_url}${contract.hello.path}`);
      expect(init?.method).toBe(contract.hello.method);
      const headers = (init?.headers ?? {}) as Record<string, string>;
      if (contract.hello.auth_required) {
        expect(headers.Authorization).toMatch(/^JACS /);
      } else {
        expect(headers.Authorization).toBeUndefined();
      }
      return new Response(JSON.stringify({
        timestamp: '2026-01-01T00:00:00Z',
        client_ip: '127.0.0.1',
        hai_public_key_fingerprint: 'fp',
        message: 'ok',
        hello_id: 'h1',
      }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });
    });
    vi.stubGlobal('fetch', fetchMock);

    await client.hello();
  });

  it('checkUsername uses the shared method/path/auth contract', async () => {
    const contract = loadContractFixture();
    const client = makeClient(contract.base_url);

    const fetchMock = vi.fn(async (url: string | URL, init?: RequestInit) => {
      const parsed = new URL(String(url));
      expect(parsed.origin + parsed.pathname).toBe(`${contract.base_url}${contract.check_username.path}`);
      expect(parsed.searchParams.get('username')).toBe('alice');
      expect(init?.method).toBe(contract.check_username.method);
      const headers = (init?.headers ?? {}) as Record<string, string>;
      if (contract.check_username.auth_required) {
        expect(headers.Authorization).toMatch(/^JACS /);
      } else {
        expect(headers.Authorization).toBeUndefined();
      }

      return new Response(JSON.stringify({
        available: true,
        username: 'alice',
      }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });
    });
    vi.stubGlobal('fetch', fetchMock);

    await client.checkUsername('alice');
  });

  it('submitResponse uses the shared method/path/auth contract', async () => {
    const contract = loadContractFixture();
    const client = makeClient(contract.base_url);
    const jobId = 'job-123';
    const expectedPath = contract.submit_response.path.replace('{job_id}', jobId);

    const fetchMock = vi.fn(async (url: string | URL, init?: RequestInit) => {
      expect(String(url)).toBe(`${contract.base_url}${expectedPath}`);
      expect(init?.method).toBe(contract.submit_response.method);
      const headers = (init?.headers ?? {}) as Record<string, string>;
      if (contract.submit_response.auth_required) {
        expect(headers.Authorization).toMatch(/^JACS /);
      } else {
        expect(headers.Authorization).toBeUndefined();
      }
      return new Response(JSON.stringify({
        success: true,
        job_id: jobId,
        message: 'ok',
      }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });
    });
    vi.stubGlobal('fetch', fetchMock);

    await client.submitResponse(jobId, 'response body');
  });
});

// ---------------------------------------------------------------------------
// Email contract JSON helpers
// ---------------------------------------------------------------------------

function loadEmailContract(filename: string): Record<string, unknown> {
  const here = dirname(fileURLToPath(import.meta.url));
  const contractPath = resolve(here, '../../contract', filename);
  return JSON.parse(readFileSync(contractPath, 'utf-8')) as Record<string, unknown>;
}

/**
 * Replicates the private `HaiClient.parseEmailMessage` field mapping so we
 * can validate contract JSON deserialization without needing a live client.
 */
function parseEmailMessage(m: Record<string, unknown>): EmailMessage {
  return {
    id: (m.id as string) || '',
    direction: (m.direction as string) || '',
    fromAddress: (m.from_address as string) || '',
    toAddress: (m.to_address as string) || '',
    subject: (m.subject as string) || '',
    bodyText: (m.body_text as string) || '',
    messageId: (m.message_id as string) || '',
    inReplyTo: (m.in_reply_to as string | null) ?? null,
    isRead: (m.is_read as boolean) ?? false,
    deliveryStatus: (m.delivery_status as string) || '',
    createdAt: (m.created_at as string) || '',
    readAt: (m.read_at as string | null) ?? null,
    jacsVerified: (m.jacs_verified as boolean) ?? false,
  };
}

/**
 * Replicates the `getEmailStatus` response mapping from HaiClient.
 */
function parseEmailStatus(data: Record<string, unknown>): EmailStatus {
  return {
    email: (data.email as string) || '',
    status: (data.status as string) || '',
    tier: (data.tier as string) || '',
    billingTier: (data.billing_tier as string) || '',
    messagesSent24h: (data.messages_sent_24h as number) || 0,
    dailyLimit: (data.daily_limit as number) || 0,
    dailyUsed: (data.daily_used as number) || 0,
    resetsAt: (data.resets_at as string) || '',
    messagesSentTotal: (data.messages_sent_total as number) || 0,
    externalEnabled: (data.external_enabled as boolean) ?? false,
    externalSendsToday: (data.external_sends_today as number) ?? 0,
    lastTierChange: (data.last_tier_change as string | null) ?? null,
  };
}

function parseKeyRegistryResponse(data: Record<string, unknown>): KeyRegistryResponse {
  return {
    email: (data.email as string) || '',
    jacsId: (data.jacs_id as string) || '',
    publicKey: (data.public_key as string) || '',
    algorithm: (data.algorithm as string) || '',
    reputationTier: (data.reputation_tier as string) || '',
    registeredAt: (data.registered_at as string) || '',
  };
}

function parseEmailVerificationResult(data: Record<string, unknown>): EmailVerificationResult {
  return {
    valid: (data.valid as boolean) ?? false,
    jacsId: (data.jacs_id as string) || '',
    reputationTier: (data.reputation_tier as string) || '',
    error: (data.error as string | null) ?? null,
  };
}

// ---------------------------------------------------------------------------
// Email contract deserialization tests
// ---------------------------------------------------------------------------

describe('contract: deserialize email message', () => {
  it('maps all snake_case API fields to camelCase SDK fields', () => {
    const raw = loadEmailContract('email_message.json');
    const msg = parseEmailMessage(raw);

    expect(msg.id).toBe('550e8400-e29b-41d4-a716-446655440000');
    expect(msg.direction).toBe('inbound');
    expect(msg.fromAddress).toBe('sender@hai.ai');
    expect(msg.toAddress).toBe('recipient@hai.ai');
    expect(msg.subject).toBe('Test Subject');
    expect(msg.bodyText).toBe('Hello, this is a test email body.');
    expect(msg.messageId).toBe('<550e8400@hai.ai>');
    expect(msg.inReplyTo).toBeNull();
    expect(msg.isRead).toBe(false);
    expect(msg.deliveryStatus).toBe('delivered');
    expect(msg.createdAt).toBe('2026-02-24T12:00:00Z');
    expect(msg.readAt).toBeNull();
    expect(msg.jacsVerified).toBe(true);
  });
});

describe('contract: deserialize list messages response', () => {
  it('parses messages array, total, and unread from envelope', () => {
    const raw = loadEmailContract('list_messages_response.json');
    const messagesRaw = raw.messages as Array<Record<string, unknown>>;
    const messages = messagesRaw.map((m) => parseEmailMessage(m));

    expect(messages).toHaveLength(1);
    expect(raw.total).toBe(1);
    expect(raw.unread).toBe(1);

    // Verify the nested message was deserialized correctly
    expect(messages[0].id).toBe('550e8400-e29b-41d4-a716-446655440000');
    expect(messages[0].fromAddress).toBe('sender@hai.ai');
    expect(messages[0].jacsVerified).toBe(true);
  });
});

describe('contract: deserialize email status', () => {
  it('maps all snake_case API fields to camelCase EmailStatus', () => {
    const raw = loadEmailContract('email_status_response.json');
    const status = parseEmailStatus(raw);

    expect(status.email).toBe('testbot@hai.ai');
    expect(status.status).toBe('active');
    expect(status.tier).toBe('new');
    expect(status.billingTier).toBe('free');
    expect(status.messagesSent24h).toBe(5);
    expect(status.dailyLimit).toBe(10);
    expect(status.dailyUsed).toBe(5);
    expect(status.resetsAt).toBe('2026-02-25T00:00:00Z');
    expect(status.messagesSentTotal).toBe(42);
    expect(status.externalEnabled).toBe(false);
    expect(status.externalSendsToday).toBe(0);
    expect(status.lastTierChange).toBeNull();
  });
});

describe('contract: deserialize key registry response', () => {
  it('maps all snake_case API fields to camelCase KeyRegistryResponse', () => {
    const raw = loadEmailContract('key_registry_response.json');
    const resp = parseKeyRegistryResponse(raw);

    expect(resp.email).toBe('testbot@hai.ai');
    expect(resp.jacsId).toBe('test-agent-jacs-id');
    expect(resp.publicKey).toBe('MCowBQYDK2VwAyEAExampleBase64PublicKeyData1234567890ABCDEF');
    expect(resp.algorithm).toBe('ed25519');
    expect(resp.reputationTier).toBe('new');
    expect(resp.registeredAt).toBe('2026-01-15T00:00:00Z');
  });
});

describe('contract: deserialize verification result', () => {
  it('maps all fields correctly', () => {
    const raw = loadEmailContract('verification_result.json');
    const result = parseEmailVerificationResult(raw);

    expect(result.valid).toBe(true);
    expect(result.jacsId).toBe('test-agent-jacs-id');
    expect(result.reputationTier).toBe('established');
    expect(result.error).toBeNull();
  });
});

describe('contract: content hash computation', () => {
  it('computes the same sha256 hash as the contract fixture', () => {
    const fixture = loadEmailContract('content_hash_example.json');
    const subject = fixture.subject as string;
    const body = fixture.body as string;
    const expectedHash = fixture.expected_hash as string;

    const computed = 'sha256:' + createHash('sha256')
      .update(subject + '\n' + body, 'utf8')
      .digest('hex');

    expect(computed).toBe(expectedHash);
  });
});

describe('contract: sign input format', () => {
  it('produces the correct sign_input from content hash and timestamp', () => {
    const fixture = loadEmailContract('content_hash_example.json');
    const expectedHash = fixture.expected_hash as string;
    const timestamp = fixture.timestamp as number;
    const expectedSignInput = fixture.sign_input_example as string;

    const signInput = `${expectedHash}:${timestamp}`;

    expect(signInput).toBe(expectedSignInput);
  });
});
