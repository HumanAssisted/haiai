import { lstat, readFile } from 'node:fs/promises';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import type { AgentConfig } from './types.js';

const ENV_PASSWORD = 'JACS_PRIVATE_KEY_PASSWORD';
const ENV_PASSWORD_FILE = 'JACS_PASSWORD_FILE';
const ENV_DISABLE_PASSWORD_ENV = 'JACS_DISABLE_PASSWORD_ENV';
const ENV_DISABLE_PASSWORD_FILE = 'JACS_DISABLE_PASSWORD_FILE';

function isDisabled(flagName: string): boolean {
  const raw = process.env[flagName]?.trim().toLowerCase();
  return raw === '1' || raw === 'true' || raw === 'yes' || raw === 'on';
}

function trimTrailingNewlines(value: string): string {
  return value.replace(/[\r\n]+$/, '');
}

async function readPasswordFileStrict(filePath: string): Promise<string> {
  let stats;
  try {
    stats = await lstat(filePath);
  } catch (error) {
    const message = (error as Error).message;
    throw new Error(`Failed to read JACS_PASSWORD_FILE (${filePath}): ${message}`);
  }

  if (stats.isSymbolicLink()) {
    throw new Error(`JACS_PASSWORD_FILE must not be a symlink: ${filePath}`);
  }

  if (!stats.isFile()) {
    throw new Error(`JACS_PASSWORD_FILE must be a regular file: ${filePath}`);
  }

  if (process.platform !== 'win32') {
    const mode = stats.mode & 0o777;
    if ((mode & 0o077) !== 0) {
      throw new Error(
        `JACS_PASSWORD_FILE has insecure permissions (${mode.toString(8)}): ${filePath}. ` +
          "Restrict to owner-only (for example: chmod 600 '/path/to/password.txt').",
      );
    }
  }

  let content: string;
  try {
    content = await readFile(filePath, 'utf-8');
  } catch (error) {
    const message = (error as Error).message;
    throw new Error(`Failed to read JACS_PASSWORD_FILE (${filePath}): ${message}`);
  }

  const passphrase = trimTrailingNewlines(content);
  if (!passphrase) {
    throw new Error(`JACS_PASSWORD_FILE is empty: ${filePath}`);
  }

  return passphrase;
}

/**
 * Resolve private-key passphrase from configured secret sources.
 *
 * Exactly one source must be configured after source filters are applied.
 *
 * Sources:
 * - JACS_PRIVATE_KEY_PASSWORD (developer default)
 * - JACS_PASSWORD_FILE
 *
 * Optional source disable flags:
 * - JACS_DISABLE_PASSWORD_ENV=1
 * - JACS_DISABLE_PASSWORD_FILE=1
 */
export async function loadPrivateKeyPassphrase(): Promise<string> {
  const envEnabled = !isDisabled(ENV_DISABLE_PASSWORD_ENV);
  const fileEnabled = !isDisabled(ENV_DISABLE_PASSWORD_FILE);

  const envPassword = process.env[ENV_PASSWORD];
  const passwordFile = process.env[ENV_PASSWORD_FILE];

  const configured: string[] = [];
  if (envEnabled && envPassword) {
    configured.push(ENV_PASSWORD);
  }
  if (fileEnabled && passwordFile) {
    configured.push(ENV_PASSWORD_FILE);
  }

  if (configured.length > 1) {
    throw new Error(
      `Multiple password sources configured: ${configured.join(', ')}. Configure exactly one.`,
    );
  }

  if (configured.length === 0) {
    throw new Error(
      'Private key password required. Configure exactly one of JACS_PRIVATE_KEY_PASSWORD or JACS_PASSWORD_FILE.',
    );
  }

  if (configured[0] === ENV_PASSWORD) {
    return envPassword as string;
  }

  const filePath = passwordFile as string;
  return readPasswordFileStrict(filePath);
}

/**
 * Load JACS agent configuration from a jacs.config.json file.
 *
 * Resolves config path in order:
 * 1. Explicit configPath argument
 * 2. JACS_CONFIG_PATH environment variable
 * 3. ./jacs.config.json (current directory)
 */
export async function loadConfig(configPath?: string): Promise<AgentConfig> {
  const resolvedPath = configPath
    ?? process.env.JACS_CONFIG_PATH
    ?? './jacs.config.json';
  const absoluteConfigPath = resolve(resolvedPath);
  const configDir = dirname(absoluteConfigPath);

  const raw = await readFile(absoluteConfigPath, 'utf-8');
  const json = JSON.parse(raw) as Record<string, unknown>;
  const keyDir = (json.jacsKeyDir ?? json.key_dir ?? '.') as string;
  const privateKeyPath = (json.jacsPrivateKeyPath ?? json.private_key_path) as string | undefined;

  return {
    jacsAgentName: (json.jacsAgentName ?? json.agent_name ?? 'unnamed-agent') as string,
    jacsAgentVersion: (json.jacsAgentVersion ?? json.agent_version ?? '1.0.0') as string,
    // Resolve relative paths from the config location, not the caller CWD.
    jacsKeyDir: isAbsolute(keyDir) ? keyDir : resolve(configDir, keyDir),
    jacsId: (json.jacsId ?? json.jacs_id) as string | undefined,
    jacsPrivateKeyPath: privateKeyPath
      ? (isAbsolute(privateKeyPath) ? privateKeyPath : resolve(configDir, privateKeyPath))
      : undefined,
  };
}

/**
 * Load the Ed25519 private key PEM from the configured key directory.
 *
 * Searches common file names in the key directory, or uses an explicit path
 * from the config.
 */
export async function loadPrivateKey(config: AgentConfig): Promise<string> {
  const keyDir = config.jacsKeyDir;

  const candidates: string[] = [];

  // Explicit path takes priority
  if (config.jacsPrivateKeyPath) {
    candidates.push(config.jacsPrivateKeyPath);
  }

  // Search in standardized order for cross-SDK compatibility
  candidates.push(
    join(keyDir, 'agent_private_key.pem'),
    join(keyDir, `${config.jacsAgentName}.private.pem`),
    join(keyDir, 'private_key.pem'),
  );

  for (const candidate of candidates) {
    try {
      const content = await readFile(candidate, 'utf-8');
      // Strip comment lines (e.g. "# WARNING: TEST-ONLY KEY")
      const pem = content
        .split('\n')
        .filter((line) => !line.startsWith('#'))
        .join('\n')
        .trim();
      if (pem) return pem;
    } catch {
      // Try next candidate
    }
  }

  throw new Error(
    `No private key found. Searched: ${candidates.join(', ')}. ` +
      'Set jacsKeyDir or jacsPrivateKeyPath in your config.',
  );
}
