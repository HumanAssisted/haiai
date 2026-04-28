import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { loadConfig, loadPrivateKey, loadPrivateKeyPassphrase } from '../src/config.js';
import { generateTestKeypair as generateKeypair } from './setup.js';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import * as os from 'node:os';

describe('config', () => {
  let tmpDir: string;

  beforeEach(async () => {
    tmpDir = await fs.mkdtemp(path.join(os.tmpdir(), 'haiai-test-'));
  });

  afterEach(async () => {
    delete process.env.JACS_CONFIG_PATH;
    delete process.env.JACS_PRIVATE_KEY_PASSWORD;
    delete process.env.JACS_PASSWORD_FILE;
    delete process.env.JACS_DISABLE_PASSWORD_ENV;
    delete process.env.JACS_DISABLE_PASSWORD_FILE;
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
      expect(config.jacsKeyDir).toBe(path.resolve(tmpDir, 'keys'));
      expect(config.jacsId).toBe('jacs-id-1');
    });

    it('loads config with canonical snake_case fields', async () => {
      const configPath = path.join(tmpDir, 'jacs.config.json');
      await fs.writeFile(configPath, JSON.stringify({
        jacs_agent_id_and_version: 'snake-id:1.0.0',
        jacs_key_directory: './mykeys',
      }));

      const config = await loadConfig(configPath);
      expect(config.jacsAgentVersion).toBe('1.0.0');
      expect(config.jacsKeyDir).toBe(path.resolve(tmpDir, 'mykeys'));
      expect(config.jacsId).toBe('snake-id');
    });

    it('throws when required fields are missing', async () => {
      const configPath = path.join(tmpDir, 'jacs.config.json');
      await fs.writeFile(configPath, JSON.stringify({}));

      await expect(loadConfig(configPath)).rejects.toThrow(
        /JACS config has neither canonical nor legacy fields/,
      );
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

      process.env.JACS_CONFIG_PATH = configPath;
      const config = await loadConfig();
      expect(config.jacsAgentName).toBe('env-bot');
      expect(config.jacsKeyDir).toBe(path.resolve(tmpDir));
    });

    it('resolves relative jacsPrivateKeyPath from config directory', async () => {
      const configPath = path.join(tmpDir, 'jacs.config.json');
      await fs.writeFile(configPath, JSON.stringify({
        jacsAgentName: 'test-bot',
        jacsAgentVersion: '1.0.0',
        jacsKeyDir: './keys',
        jacsPrivateKeyPath: './custom/agent_private_key.pem',
      }));

      const config = await loadConfig(configPath);
      expect(config.jacsPrivateKeyPath).toBe(
        path.resolve(tmpDir, 'custom/agent_private_key.pem'),
      );
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

    it('throws when jacsPrivateKeyPath is configured but missing', async () => {
      const keyPath = path.join(tmpDir, 'missing_key.pem');
      const kp = generateKeypair();
      await fs.writeFile(path.join(tmpDir, 'agent_private_key.pem'), kp.privateKeyPem);

      const config = {
        jacsAgentName: 'test',
        jacsAgentVersion: '1.0',
        jacsKeyDir: tmpDir,
        jacsPrivateKeyPath: keyPath,
      };

      await expect(loadPrivateKey(config)).rejects.toThrow(
        'Configured jacsPrivateKeyPath does not exist',
      );
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

  describe('loadPrivateKeyPassphrase', () => {
    it('uses env password by default', async () => {
      process.env.JACS_PRIVATE_KEY_PASSWORD = 'dev-password';
      const passphrase = await loadPrivateKeyPassphrase();
      expect(passphrase).toBe('dev-password');
    });

    it('uses password file when env source is disabled', async () => {
      const passwordFile = path.join(tmpDir, 'password.txt');
      await fs.writeFile(passwordFile, 'file-password\n', 'utf-8');
      if (process.platform !== 'win32') {
        await fs.chmod(passwordFile, 0o600);
      }

      process.env.JACS_PRIVATE_KEY_PASSWORD = 'dev-password';
      process.env.JACS_PASSWORD_FILE = passwordFile;
      process.env.JACS_DISABLE_PASSWORD_ENV = '1';

      const passphrase = await loadPrivateKeyPassphrase();
      expect(passphrase).toBe('file-password');
    });

    it('throws when multiple password sources are configured', async () => {
      const passwordFile = path.join(tmpDir, 'password.txt');
      await fs.writeFile(passwordFile, 'file-password\n', 'utf-8');

      process.env.JACS_PRIVATE_KEY_PASSWORD = 'dev-password';
      process.env.JACS_PASSWORD_FILE = passwordFile;

      await expect(loadPrivateKeyPassphrase()).rejects.toThrow('Multiple password sources configured');
    });

    it('throws when no password source is configured', async () => {
      await expect(loadPrivateKeyPassphrase()).rejects.toThrow('Private key password required');
    });

    it('skips disabled file source and uses env', async () => {
      process.env.JACS_PRIVATE_KEY_PASSWORD = 'dev-password';
      process.env.JACS_PASSWORD_FILE = '/tmp/unused-password-file.txt';
      process.env.JACS_DISABLE_PASSWORD_FILE = 'true';

      const passphrase = await loadPrivateKeyPassphrase();
      expect(passphrase).toBe('dev-password');
    });

    it('rejects insecure password file permissions on unix', async () => {
      if (process.platform === 'win32') {
        return;
      }

      const passwordFile = path.join(tmpDir, 'password-insecure.txt');
      await fs.writeFile(passwordFile, 'file-password\n', 'utf-8');
      await fs.chmod(passwordFile, 0o644);

      process.env.JACS_PASSWORD_FILE = passwordFile;
      process.env.JACS_DISABLE_PASSWORD_ENV = '1';

      await expect(loadPrivateKeyPassphrase()).rejects.toThrow('insecure permissions');
    });

    it('rejects symlink password file sources', async () => {
      if (process.platform === 'win32') {
        return;
      }

      const targetFile = path.join(tmpDir, 'password-target.txt');
      const linkFile = path.join(tmpDir, 'password-link.txt');
      await fs.writeFile(targetFile, 'file-password\n', 'utf-8');
      await fs.chmod(targetFile, 0o600);
      await fs.symlink(targetFile, linkFile);

      process.env.JACS_PASSWORD_FILE = linkFile;
      process.env.JACS_DISABLE_PASSWORD_ENV = '1';

      await expect(loadPrivateKeyPassphrase()).rejects.toThrow('must not be a symlink');
    });
  });
});
