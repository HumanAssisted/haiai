import { generateKeyPairSync } from 'node:crypto';
import { JacsAgent } from '@hai.ai/jacs';

/** Strong test password that meets JACS security requirements. */
export const TEST_PASSWORD = 'Xk9#mP2vL7qR4!nB8wZ';

/** A test JACS ID. */
export const TEST_JACS_ID = 'test-agent-001';

/** The test JACS agent (ephemeral, in-memory). Can sign and verify. */
export const TEST_AGENT = new JacsAgent();
const _ephResult = JSON.parse(TEST_AGENT.ephemeralSync('ring-Ed25519'));

/**
 * A PEM-encoded public key from a generated Ed25519 keypair.
 * Used for tests that need deterministic PEM-shaped key material without
 * depending on JACS on-disk agent layout.
 */
export const TEST_PUBLIC_KEY_PEM = (() => {
  const { publicKey } = generateKeyPairSync('ed25519');
  return publicKey.export({ format: 'pem', type: 'spki' }).toString().trim();
})();

/**
 * Generate an Ed25519 keypair as plaintext PEM for tests that only need
 * stable key fixtures or a public/private pair written to disk.
 */
export function generateTestKeypair(): { publicKeyPem: string; privateKeyPem: string } {
  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  return {
    publicKeyPem: publicKey.export({ format: 'pem', type: 'spki' }).toString().trim(),
    privateKeyPem: privateKey.export({ format: 'pem', type: 'pkcs8' }).toString().trim(),
  };
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
