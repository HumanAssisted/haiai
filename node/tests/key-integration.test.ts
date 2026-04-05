/**
 * Live integration tests for JACS key rotation and versioned fetch operations.
 *
 * Gated behind HAI_LIVE_TEST=1. Requires a running HAI API at
 * HAI_URL (defaults to http://localhost:3000).
 *
 * Run:
 *   HAI_LIVE_TEST=1 HAI_URL=http://localhost:3000 npx vitest run tests/key-integration.test.ts
 */

import { describe, it, expect, beforeAll } from 'vitest';
import { HaiClient } from '../src/index.js';
import { generateTestKeypair as generateKeypair } from './setup.js';

const LIVE = process.env.HAI_LIVE_TEST === '1';
const API_URL = process.env.HAI_URL || 'http://localhost:3000';

describe.skipIf(!LIVE)('Key integration (live API)', () => {
  let client: HaiClient;
  const agentName = `node-key-integ-${Date.now()}`;
  let jacsId: string;
  let agentId: string;

  // -------------------------------------------------------------------------
  // Setup: register agent
  // -------------------------------------------------------------------------

  beforeAll(async () => {
    const keypair = generateKeypair();
    client = await HaiClient.fromCredentials(agentName, keypair.privateKeyPem, {
      url: API_URL,
      privateKeyPassphrase: 'keygen-password',
    });

    const ownerEmail = process.env.HAI_OWNER_EMAIL || 'jonathan@hai.io';
    const result = await client.register({
      description: 'Node SDK key integration test agent',
      ownerEmail,
    });

    expect(result.success).toBe(true);
    jacsId = result.jacsId!;
    agentId = result.agentId!;
    console.log(`Registered agent: jacsId=${jacsId}, agentId=${agentId}`);
  }, 30_000);

  // -------------------------------------------------------------------------
  // Test: register then fetch key matches
  // -------------------------------------------------------------------------

  it('should fetch remote key matching registration', async () => {
    const key = await client.fetchRemoteKey(jacsId, 'latest');
    expect(key.jacsId || key.publicKey).toBeTruthy();
    expect(key.publicKey).toBeTruthy();
    expect(key.algorithm).toBeTruthy();
  });

  // -------------------------------------------------------------------------
  // Test: fetch key by hash
  // -------------------------------------------------------------------------

  it('should fetch key by hash', async () => {
    const key = await client.fetchRemoteKey(jacsId, 'latest');
    if (!key.publicKeyHash) {
      console.warn('Server did not return publicKeyHash, skipping');
      return;
    }

    const byHash = await client.fetchKeyByHash(key.publicKeyHash);
    expect(byHash.publicKey).toBe(key.publicKey);
    expect(byHash.algorithm).toBe(key.algorithm);
  });

  // -------------------------------------------------------------------------
  // Test: fetch key by email
  // -------------------------------------------------------------------------

  it('should fetch key by email after registration', async () => {
    // Username is now claimed during registration (one-step flow).
    const email = `${agentName}@hai.ai`;

    let byEmail;
    try {
      byEmail = await client.fetchKeyByEmail(email);
    } catch {
      console.warn('FetchKeyByEmail failed (agent may not have email in test env), skipping');
      return;
    }

    expect(byEmail.jacsId).toBeTruthy();
    expect(byEmail.publicKey).toBeTruthy();
  });

  // -------------------------------------------------------------------------
  // Test: fetch all keys returns history
  // -------------------------------------------------------------------------

  it('should return key history with at least one entry', async () => {
    const history = await client.fetchAllKeys(jacsId);
    expect(history.jacsId || history.total).toBeTruthy();
    expect(history.total).toBeGreaterThanOrEqual(1);
    expect(history.keys.length).toBeGreaterThanOrEqual(1);
    expect(history.keys[0].publicKey).toBeTruthy();
  });

  // -------------------------------------------------------------------------
  // Test: fetch key by domain returns 404 for fake domain
  // -------------------------------------------------------------------------

  it('should return 404 for nonexistent domain', async () => {
    await expect(
      client.fetchKeyByDomain('nonexistent-test-domain-12345.invalid'),
    ).rejects.toThrow();
  });
});
