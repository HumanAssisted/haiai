import { createPrivateKey, createPublicKey, sign, verify, generateKeyPairSync } from 'node:crypto';

/**
 * CRYPTO POLICY:
 * This module is transitional. Cryptographic operations in haisdk must
 * delegate to JACS functions. Do not add new local cryptographic implementations.
 */

/**
 * Sign a UTF-8 message with an Ed25519 private key (PEM-encoded).
 * Returns the signature as a base64 string.
 */
export function signString(privateKeyPem: string, message: string): string {
  const key = createPrivateKey(privateKeyPem);
  const signature = sign(null, Buffer.from(message, 'utf-8'), key);
  return signature.toString('base64');
}

/**
 * Verify an Ed25519 signature over a UTF-8 message.
 * `publicKeyPem` is PEM-encoded, `signatureB64` is base64-encoded.
 */
export function verifyString(publicKeyPem: string, message: string, signatureB64: string): boolean {
  try {
    let key;
    if (publicKeyPem.startsWith('-----')) {
      key = createPublicKey(publicKeyPem);
    } else {
      // Assume raw base64 Ed25519 public key
      const keyBuffer = Buffer.from(publicKeyPem, 'base64');
      key = createPublicKey({
        key: keyBuffer,
        format: 'der',
        type: 'spki',
      });
    }
    const signature = Buffer.from(signatureB64, 'base64');
    return verify(null, Buffer.from(message, 'utf-8'), key, signature);
  } catch {
    return false;
  }
}

/**
 * Generate a new Ed25519 keypair.
 * Returns { publicKeyPem, privateKeyPem }.
 */
export function generateKeypair(): { publicKeyPem: string; privateKeyPem: string } {
  const { publicKey, privateKey } = generateKeyPairSync('ed25519');

  const publicKeyPem = publicKey.export({ type: 'spki', format: 'pem' }) as string;
  const privateKeyPem = privateKey.export({ type: 'pkcs8', format: 'pem' }) as string;

  return { publicKeyPem, privateKeyPem };
}
