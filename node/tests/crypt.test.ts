import { describe, it, expect } from 'vitest';
import { signString, verifyString, generateKeypair } from '../src/crypt.js';
import { TEST_KEYPAIR } from './setup.js';

describe('crypt', () => {
  describe('generateKeypair', () => {
    it('generates valid Ed25519 keypair', () => {
      const kp = generateKeypair();
      expect(kp.publicKeyPem).toContain('-----BEGIN PUBLIC KEY-----');
      expect(kp.privateKeyPem).toContain('-----BEGIN PRIVATE KEY-----');
    });

    it('generates different keypairs each time', () => {
      const kp1 = generateKeypair();
      const kp2 = generateKeypair();
      expect(kp1.privateKeyPem).not.toBe(kp2.privateKeyPem);
      expect(kp1.publicKeyPem).not.toBe(kp2.publicKeyPem);
    });
  });

  describe('signString', () => {
    it('produces a base64 signature', () => {
      const sig = signString(TEST_KEYPAIR.privateKeyPem, 'hello world');
      expect(typeof sig).toBe('string');
      // Ed25519 signatures are 64 bytes
      expect(Buffer.from(sig, 'base64').length).toBe(64);
    });

    it('produces consistent signatures for the same message', () => {
      const sig1 = signString(TEST_KEYPAIR.privateKeyPem, 'deterministic');
      const sig2 = signString(TEST_KEYPAIR.privateKeyPem, 'deterministic');
      expect(sig1).toBe(sig2);
    });

    it('produces different signatures for different messages', () => {
      const sig1 = signString(TEST_KEYPAIR.privateKeyPem, 'message1');
      const sig2 = signString(TEST_KEYPAIR.privateKeyPem, 'message2');
      expect(sig1).not.toBe(sig2);
    });

    it('signs empty string without error', () => {
      const sig = signString(TEST_KEYPAIR.privateKeyPem, '');
      expect(Buffer.from(sig, 'base64').length).toBe(64);
    });

    it('signs UTF-8 content', () => {
      const sig = signString(TEST_KEYPAIR.privateKeyPem, 'Hello 世界 🌍');
      expect(Buffer.from(sig, 'base64').length).toBe(64);
    });
  });

  describe('verifyString', () => {
    it('verifies a valid signature', () => {
      const message = 'test message';
      const sig = signString(TEST_KEYPAIR.privateKeyPem, message);
      const valid = verifyString(TEST_KEYPAIR.publicKeyPem, message, sig);
      expect(valid).toBe(true);
    });

    it('rejects tampered message', () => {
      const sig = signString(TEST_KEYPAIR.privateKeyPem, 'original');
      const valid = verifyString(TEST_KEYPAIR.publicKeyPem, 'tampered', sig);
      expect(valid).toBe(false);
    });

    it('rejects tampered signature', () => {
      const message = 'test';
      const sig = signString(TEST_KEYPAIR.privateKeyPem, message);
      // Flip a byte in the signature
      const buf = Buffer.from(sig, 'base64');
      buf[0] = buf[0] ^ 0xff;
      const valid = verifyString(TEST_KEYPAIR.publicKeyPem, message, buf.toString('base64'));
      expect(valid).toBe(false);
    });

    it('rejects wrong key', () => {
      const otherKp = generateKeypair();
      const sig = signString(TEST_KEYPAIR.privateKeyPem, 'test');
      const valid = verifyString(otherKp.publicKeyPem, 'test', sig);
      expect(valid).toBe(false);
    });

    it('returns false for invalid PEM', () => {
      const valid = verifyString('not-a-pem', 'test', 'bad-sig');
      expect(valid).toBe(false);
    });

    it('returns false for empty inputs', () => {
      expect(verifyString('', 'msg', 'sig')).toBe(false);
      expect(verifyString(TEST_KEYPAIR.publicKeyPem, '', '')).toBe(false);
    });

    it('roundtrips with sign', () => {
      const kp = generateKeypair();
      const message = 'roundtrip test ' + Date.now();
      const sig = signString(kp.privateKeyPem, message);
      expect(verifyString(kp.publicKeyPem, message, sig)).toBe(true);
    });
  });
});
