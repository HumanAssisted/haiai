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
import type { SendEmailResult, EmailMessage, EmailStatus } from '../src/types.js';

const LIVE = process.env.HAI_LIVE_TEST === '1';
const API_URL = process.env.HAI_URL || 'http://localhost:3000';

/** Small helper to wait for async email delivery to settle. */
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

describe.skipIf(!LIVE)('Email integration (live API)', () => {
  let client: HaiClient;
  const agentName = `node-integ-${Date.now()}`;
  let sentMessageId: string;
  let replyMessageId: string;
  const subject = `node-integ-test-${Date.now()}`;
  const body = 'Hello from Node integration test!';

  // -------------------------------------------------------------------------
  // Setup: register agent + claim username to provision @hai.ai email
  // -------------------------------------------------------------------------

  beforeAll(async () => {
    // 1. Generate fresh Ed25519 keypair.
    const keypair = generateKeypair();

    // 2. Build client from credentials (no jacs.config.json needed).
    client = HaiClient.fromCredentials(agentName, keypair.privateKeyPem, {
      url: API_URL,
    });

    // 3. Register the agent with the local API (bootstrap registration).
    const ownerEmail = process.env.HAI_OWNER_EMAIL || 'jonathan@hai.io';
    const result = await client.register({
      description: 'Node SDK email integration test agent',
      ownerEmail,
    });

    expect(result.success).toBe(true);
    expect(result.jacsId).toBeTruthy();
    expect(result.agentId).toBeTruthy();
    console.log(`Registered agent: jacsId=${result.jacsId}, agentId=${result.agentId}`);

    // 4. Claim a username to provision the @hai.ai email address.
    const claim = await client.claimUsername(client.haiAgentId, agentName);
    expect(claim.email).toContain('@hai.ai');
    console.log(`Claimed username: ${claim.username}, email=${claim.email}`);
  }, 30_000);

  // -------------------------------------------------------------------------
  // 1. Send email -> assert message_id
  // -------------------------------------------------------------------------

  it('should send an email and return a message_id', async () => {
    const result: SendEmailResult = await client.sendEmail({
      to: `${agentName}@hai.ai`,
      subject,
      body,
    });

    sentMessageId = result.messageId;
    expect(sentMessageId).toBeTruthy();
    expect(typeof sentMessageId).toBe('string');
    expect(result.status).toBeTruthy();
    console.log(`Sent email: messageId=${sentMessageId}, status=${result.status}`);

    // Allow time for async delivery before subsequent tests.
    await sleep(2000);
  }, 15_000);

  // -------------------------------------------------------------------------
  // 2. List messages -> assert sent message appears
  // -------------------------------------------------------------------------

  it('should list messages including the sent message', async () => {
    const messages: EmailMessage[] = await client.listMessages({ limit: 50 });
    expect(messages.length).toBeGreaterThan(0);

    // The sent message should appear in the list (either as outbound or
    // delivered back to self as inbound).
    const found = messages.some(
      (m) => m.id === sentMessageId || m.subject === subject,
    );
    expect(found).toBe(true);
    console.log(`Listed ${messages.length} messages; sent message found=${found}`);
  });

  // -------------------------------------------------------------------------
  // 3. Get message -> assert subject/body match
  // -------------------------------------------------------------------------

  it('should get the message by id with matching subject and body', async () => {
    const msg: EmailMessage = await client.getMessage(sentMessageId);
    expect(msg.id).toBe(sentMessageId);
    expect(msg.subject).toBe(subject);
    expect(msg.bodyText).toContain(body);
    expect(msg.direction).toBeTruthy();
    expect(msg.createdAt).toBeTruthy();
    console.log(`Got message: id=${msg.id}, subject=${msg.subject}`);
  });

  // -------------------------------------------------------------------------
  // 4. Mark read / unread -> no error
  // -------------------------------------------------------------------------

  it('should mark a message as read without error', async () => {
    await client.markRead(sentMessageId);

    // Verify the read state persisted.
    const msg = await client.getMessage(sentMessageId);
    expect(msg.isRead).toBe(true);
    console.log('Marked read; isRead=' + msg.isRead);
  });

  it('should mark a message as unread without error', async () => {
    await client.markUnread(sentMessageId);

    // Verify the unread state persisted.
    const msg = await client.getMessage(sentMessageId);
    expect(msg.isRead).toBe(false);
    console.log('Marked unread; isRead=' + msg.isRead);
  });

  // -------------------------------------------------------------------------
  // 5. Search -> assert message found
  // -------------------------------------------------------------------------

  it('should search messages and find the sent message', async () => {
    const results: EmailMessage[] = await client.searchMessages({ query: subject });
    expect(results.length).toBeGreaterThan(0);

    const found = results.some(
      (m) => m.id === sentMessageId || m.subject === subject,
    );
    expect(found).toBe(true);
    console.log(`Search found ${results.length} results; target found=${found}`);
  });

  // -------------------------------------------------------------------------
  // 6. Unread count -> assert returns number
  // -------------------------------------------------------------------------

  it('should return unread count as a number', async () => {
    const count: number = await client.getUnreadCount();
    expect(typeof count).toBe('number');
    expect(count).toBeGreaterThanOrEqual(0);
    console.log(`Unread count: ${count}`);
  });

  // -------------------------------------------------------------------------
  // 7. Email status -> assert returns status
  // -------------------------------------------------------------------------

  it('should return email status with expected fields', async () => {
    const status: EmailStatus = await client.getEmailStatus();
    expect(status.email).toBeTruthy();
    expect(status.email).toContain('@hai.ai');
    expect(status.status).toBeTruthy();
    expect(typeof status.dailyLimit).toBe('number');
    expect(typeof status.dailyUsed).toBe('number');
    expect(typeof status.messagesSentTotal).toBe('number');
    console.log(
      `Email status: email=${status.email}, status=${status.status}, tier=${status.tier}`,
    );
  });

  // -------------------------------------------------------------------------
  // 8. Reply -> assert reply message_id
  // -------------------------------------------------------------------------

  it('should reply to the sent message and return a reply message_id', async () => {
    // reply() expects the internal message ID (used in URL path), not the
    // RFC Message-ID. It fetches the original message, derives the sender
    // and subject, then calls sendEmail with inReplyTo threading.
    const result: SendEmailResult = await client.reply(
      sentMessageId,
      'Reply from Node integration test!',
    );

    replyMessageId = result.messageId;
    expect(replyMessageId).toBeTruthy();
    expect(typeof replyMessageId).toBe('string');
    expect(result.status).toBeTruthy();
    console.log(`Reply sent: messageId=${replyMessageId}, status=${result.status}`);
  }, 15_000);

  // -------------------------------------------------------------------------
  // 9. Delete -> assert no error
  // -------------------------------------------------------------------------

  it('should delete the sent message without error', async () => {
    // deleteMessage resolves void on success; any throw means failure.
    await client.deleteMessage(sentMessageId);
    console.log(`Deleted message: ${sentMessageId}`);
  });

  // -------------------------------------------------------------------------
  // 10. Get deleted -> assert error/404
  // -------------------------------------------------------------------------

  it('should throw when getting a deleted message', async () => {
    await expect(client.getMessage(sentMessageId)).rejects.toThrow();
    console.log('Confirmed deleted message returns error');
  });
});
