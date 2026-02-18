import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { loadConfig, loadPrivateKey } from '../src/config.js';
import { generateKeypair } from '../src/crypt.js';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import * as os from 'node:os';

describe('config', () => {
  let tmpDir: string;

  beforeEach(async () => {
    tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'haisdk-test-'));
  });

  afterEach(async () => {
    await fs.rm(tmpDir, { recursive: true, force: true });
  });

  describe('loadConfig', () => {
    it('loads config from explicit path', async () => {
      const configPath = path.join(tmpDir, 'jacs.config.json');
      await fs.writeFile(configPath, JSON.stringify({
        jacsAgentName: 'test-bot',
        jacsAgentVersion: '2.0.0',
        jacsKeyDir: './keys',
        jacsId: 'jacs-id-1',
      }));

      const config = await loadConfig(configPath);
      expect(config.jacsAgentName).toBe('test-bot');
      expect(config.jacsAgentVersion).toBe('2.0.0');
      expect(config.jacsKeyDir).toBe('./keys');
      expect(config.jacsId).toBe('jacs-id-1');
    });

    it('loads config with snake_case fields', async () => {
      const configPath = path.join(tmpDir, 'jacs.config.json');
      await fs.writeFile(configPath, JSON.stringify({
        agent_name: 'snake-bot',
        agent_version: '1.0.0',
        key_dir: './mykeys',
        jacs_id: 'snake-id',
      }));

      const config = await loadConfig(configPath);
      expect(config.jacsAgentName).toBe('snake-bot');
      expect(config.jacsAgentVersion).toBe('1.0.0');
      expect(config.jacsKeyDir).toBe('./mykeys');
      expect(config.jacsId).toBe('snake-id');
    });

    it('defaults missing fields', async () => {
      const configPath = path.join(tmpDir, 'jacs.config.json');
      await fs.writeFile(configPath, JSON.stringify({}));

      const config = await loadConfig(configPath);
      expect(config.jacsAgentName).toBe('unnamed-agent');
      expect(config.jacsAgentVersion).toBe('1.0.0');
      expect(config.jacsKeyDir).toBe('.');
    });

    it('throws for non-existent file', async () => {
      await expect(loadConfig('/non/existent/path.json')).rejects.toThrow();
    });

    it('throws for invalid JSON', async () => {
      const configPath = path.join(tmpDir, 'bad.json');
      await fs.writeFile(configPath, 'not-json');
      await expect(loadConfig(configPath)).rejects.toThrow();
    });

    it('uses JACS_CONFIG_PATH env var', async () => {
      const configPath = path.join(tmpDir, 'jacs.config.json');
      await fs.writeFile(configPath, JSON.stringify({
        jacsAgentName: 'env-bot',
        jacsAgentVersion: '1.0.0',
        jacsKeyDir: '.',
      }));

      const originalEnv = process.env.JACS_CONFIG_PATH;
      try {
        process.env.JACS_CONFIG_PATH = configPath;
        const config = await loadConfig();
        expect(config.jacsAgentName).toBe('env-bot');
      } finally {
        if (originalEnv !== undefined) {
          process.env.JACS_CONFIG_PATH = originalEnv;
        } else {
          delete process.env.JACS_CONFIG_PATH;
        }
      }
    });
  });

  describe('loadPrivateKey', () => {
    it('loads private key from key directory', async () => {
      const keyDir = path.join(tmpDir, 'keys');
      await fs.mkdir(keyDir);

      const kp = generateKeypair();
      await fs.writeFile(path.join(keyDir, 'private_key.pem'), kp.privateKeyPem);

      const config = {
        jacsAgentName: 'test',
        jacsAgentVersion: '1.0',
        jacsKeyDir: keyDir,
      };

      const pem = await loadPrivateKey(config);
      expect(pem).toContain('-----BEGIN PRIVATE KEY-----');
    });

    it('loads from jacsPrivateKeyPath if specified', async () => {
      const keyPath = path.join(tmpDir, 'custom_key.pem');
      const kp = generateKeypair();
      await fs.writeFile(keyPath, kp.privateKeyPem);

      const config = {
        jacsAgentName: 'test',
        jacsAgentVersion: '1.0',
        jacsKeyDir: tmpDir,
        jacsPrivateKeyPath: keyPath,
      };

      const pem = await loadPrivateKey(config);
      expect(pem).toContain('-----BEGIN PRIVATE KEY-----');
    });

    it('strips comment lines from PEM', async () => {
      const keyDir = path.join(tmpDir, 'keys');
      await fs.mkdir(keyDir);

      const kp = generateKeypair();
      const commentedPem = '# WARNING: TEST ONLY\n' + kp.privateKeyPem;
      await fs.writeFile(path.join(keyDir, 'private_key.pem'), commentedPem);

      const config = {
        jacsAgentName: 'test',
        jacsAgentVersion: '1.0',
        jacsKeyDir: keyDir,
      };

      const pem = await loadPrivateKey(config);
      expect(pem).not.toContain('# WARNING');
      expect(pem).toContain('-----BEGIN PRIVATE KEY-----');
    });

    it('throws when no key found', async () => {
      const config = {
        jacsAgentName: 'test',
        jacsAgentVersion: '1.0',
        jacsKeyDir: tmpDir, // empty dir
      };

      await expect(loadPrivateKey(config)).rejects.toThrow('No private key found');
    });

    it('tries agent_private_key.pem as candidate', async () => {
      const keyDir = path.join(tmpDir, 'keys');
      await fs.mkdir(keyDir);

      const kp = generateKeypair();
      await fs.writeFile(path.join(keyDir, 'agent_private_key.pem'), kp.privateKeyPem);

      const config = {
        jacsAgentName: 'test',
        jacsAgentVersion: '1.0',
        jacsKeyDir: keyDir,
      };

      const pem = await loadPrivateKey(config);
      expect(pem).toContain('-----BEGIN PRIVATE KEY-----');
    });
  });
});
