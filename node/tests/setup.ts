import { JacsAgent, createAgentSync } from '@hai.ai/jacs';
import { mkdtempSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

/** Strong test password that meets JACS security requirements. */
export const TEST_PASSWORD = 'Xk9#mP2vL7qR4!nB8wZ';

/** A test JACS ID. */
export const TEST_JACS_ID = 'test-agent-001';

/** The test JACS agent (ephemeral, in-memory). Can sign and verify. */
export const TEST_AGENT = new JacsAgent();
const _ephResult = JSON.parse(TEST_AGENT.ephemeralSync('ring-Ed25519'));

/**
 * A PEM-encoded public key from a generated keypair (NOT matching TEST_AGENT).
 * Used only for contract/deserialization tests that need a PEM string.
 * For sign/verify tests, use TEST_AGENT directly.
 */
export const TEST_PUBLIC_KEY_PEM = (() => {
  const tempDir = mkdtempSync(join(tmpdir(), 'haisdk-test-pubkey-'));
  const kd = join(tempDir, 'keys');
  const dd = join(tempDir, 'data');
  const cp = join(tempDir, 'jacs.config.json');
  const prevPw = process.env.JACS_PRIVATE_KEY_PASSWORD;
  process.env.JACS_PRIVATE_KEY_PASSWORD = TEST_PASSWORD;
  createAgentSync(
    'pubkey-helper',
    TEST_PASSWORD,
    'ring-Ed25519',
    dd, kd, cp,
    null, null, null, null,
  );
  const pem = readFileSync(join(kd, 'jacs.public.pem'), 'utf-8').trim();
  if (prevPw !== undefined) {
    process.env.JACS_PRIVATE_KEY_PASSWORD = prevPw;
  } else {
    delete process.env.JACS_PRIVATE_KEY_PASSWORD;
  }
  return pem;
})();

/**
 * Generate a keypair using JACS core (replaces local generateKeypair).
 * Creates a temporary JACS agent and extracts the key material from disk.
 * Note: the returned keys are from a different agent than TEST_AGENT.
 */
export function generateTestKeypair(): { publicKeyPem: string; privateKeyPem: string } {
  const tempDir = mkdtempSync(join(tmpdir(), 'haisdk-keygen-'));
  const kd = join(tempDir, 'keys');
  const dd = join(tempDir, 'data');
  const cp = join(tempDir, 'jacs.config.json');

  const prevPw = process.env.JACS_PRIVATE_KEY_PASSWORD;
  process.env.JACS_PRIVATE_KEY_PASSWORD = TEST_PASSWORD;

  createAgentSync(
    `test-${Date.now()}`,
    TEST_PASSWORD,
    'ring-Ed25519',
    dd, kd, cp,
    null, null, null, null,
  );

  if (prevPw !== undefined) {
    process.env.JACS_PRIVATE_KEY_PASSWORD = prevPw;
  } else {
    delete process.env.JACS_PRIVATE_KEY_PASSWORD;
  }

  const publicKeyPem = readFileSync(join(kd, 'jacs.public.pem'), 'utf-8').trim();
  const privateKeyPem = readFileSync(join(kd, 'jacs.private.pem.enc'), 'utf-8').trim();

  return { publicKeyPem, privateKeyPem };
}

/**
 * Create a mock SSE response body from a list of events.
 */
export function createSseBody(events: Array<{ event?: string; data: string; id?: string }>): ReadableStream<Uint8Array> {
  const encoder = new TextEncoder();
  let sent = false;

  return new ReadableStream({
    pull(controller) {
      if (sent) {
        controller.close();
        return;
      }
      sent = true;

      let text = '';
      for (const evt of events) {
        if (evt.event) text += `event: ${evt.event}\n`;
        if (evt.id) text += `id: ${evt.id}\n`;
        text += `data: ${evt.data}\n\n`;
      }
      controller.enqueue(encoder.encode(text));
    },
  });
}
