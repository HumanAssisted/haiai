/**
 * Live integration tests for HAI email CRUD operations.
 *
 * Gated behind HAI_LIVE_TEST=1. Requires a running HAI API at
 * HAI_URL (defaults to http://localhost:3000) backed by Stalwart.
 *
 * Run:
 *   HAI_LIVE_TEST=1 HAI_URL=http://localhost:3000 npx vitest run tests/email-integration.test.ts
 */

import { describe, it, expect, beforeAll } from 'vitest';
import { HaiClient, generateKeypair } from '../src/index.js';

const LIVE = process.env.HAI_LIVE_TEST === '1';
const API_URL = process.env.HAI_URL || 'http://localhost:3000';

describe.skipIf(!LIVE)('Email integration (live API)', () => {
  let client: HaiClient;
  const agentName = `node-integ-${Date.now()}`;
  let sentMessageId: string;
  let rfcMessageId: string;
  const subject = `node-integ-test-${Date.now()}`;
  const body = 'Hello from Node integration test!';

  beforeAll(async () => {
    // Generate keypair and build client from credentials.
    const keypair = generateKeypair();
    client = HaiClient.fromCredentials(agentName, keypair.privateKeyPem, {
      url: API_URL,
    });

    // Register the agent with the local API.
    const result = await client.register({
      ownerEmail: 'test@example.com',
      description: 'Node integration test agent',
    });

    expect(result.success).toBe(true);
    console.log(`Registered agent: jacsId=${result.jacsId}`);
  }, 30_000);

  it('should send an email', async () => {
    const result = await client.sendEmail({
      to: `${agentName}@hai.ai`,
      subject,
      body,
    });

    sentMessageId = result.messageId;
    expect(sentMessageId).toBeTruthy();
    console.log(`Sent email: messageId=${sentMessageId}`);

    // Small delay for async delivery
    await new Promise((r) => setTimeout(r, 2000));
  }, 15_000);

  it('should list messages', async () => {
    const messages = await client.listMessages({ limit: 10 });
    expect(messages.length).toBeGreaterThan(0);
    console.log(`Listed ${messages.length} messages`);
  });

  it('should get message by id', async () => {
    const msg = await client.getMessage(sentMessageId);
    expect(msg.subject).toBe(subject);
    expect(msg.bodyText).toContain(body);

    rfcMessageId = msg.messageId || sentMessageId;
    console.log(`Got message: subject=${msg.subject}`);
  });

  it('should mark read', async () => {
    await client.markRead(sentMessageId);
    console.log('Marked read');
  });

  it('should mark unread', async () => {
    await client.markUnread(sentMessageId);
    console.log('Marked unread');
  });

  it('should search messages', async () => {
    const results = await client.searchMessages({ query: subject });
    expect(results.length).toBeGreaterThan(0);
    console.log(`Search found ${results.length} results`);
  });

  it('should get unread count', async () => {
    const count = await client.getUnreadCount();
    expect(typeof count).toBe('number');
    console.log(`Unread count: ${count}`);
  });

  it('should get email status', async () => {
    const status = await client.getEmailStatus();
    expect(status.email).toBeTruthy();
    console.log(`Email status: email=${status.email}, tier=${status.tier}`);
  });

  it('should reply to an email', async () => {
    const result = await client.reply(
      rfcMessageId,
      'Reply from Node integration test!',
    );
    expect(result.messageId).toBeTruthy();
    console.log(`Reply sent: messageId=${result.messageId}`);
  });

  it('should delete a message', async () => {
    await client.deleteMessage(sentMessageId);
    console.log('Deleted message');
  });

  it('should error when getting deleted message', async () => {
    await expect(client.getMessage(sentMessageId)).rejects.toThrow();
    console.log('Verified deleted message returns error');
  });
});
