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

// Canonical Rust-format JACS config fields (written by `haiai init` and JACS
// itself). Detection: presence of `jacs_agent_id_and_version` +
// `jacs_key_directory` means the config is in Rust-canonical format. The
// Python SDK previously only understood the camelCase wrapper form and
// produced a false-green; this parity was added to the Node SDK as well.
const CANONICAL_FIELDS = ['jacs_agent_id_and_version', 'jacs_key_directory'] as const;
const LEGACY_FIELDS = ['jacsAgentName', 'jacsAgentVersion', 'jacsKeyDir'] as const;

function resolveAbsoluteDir(value: string, configDir: string): string {
  return isAbsolute(value) ? value : resolve(configDir, value);
}

function parseAgentIdAndVersion(value: string): { agentId: string; version: string } {
  const idx = value.indexOf(':');
  if (idx < 0) {
    throw new Error(
      `Invalid jacs_agent_id_and_version (expected 'id:version'): ${JSON.stringify(value)}`,
    );
  }
  const agentId = value.slice(0, idx);
  const version = value.slice(idx + 1);
  if (!agentId || !version) {
    throw new Error(
      `jacs_agent_id_and_version has empty component: ${JSON.stringify(value)}`,
    );
  }
  return { agentId, version };
}

/**
 * Read the agent name from the signed agent document on disk.
 *
 * JACS stores the agent document at `{data_dir}/agent/{id:version}.json`.
 * The name isn't in the canonical config file — it's a field of the signed
 * document. We only need it for `AgentConfig.jacsAgentName` (logging /
 * display); the FFI loads the real data from the config path we pass.
 *
 * Permissive by design: if the doc isn't readable, fall back to
 * `id_and_version` as the name rather than blocking config load.
 */
async function readAgentNameFromDoc(
  dataDir: string,
  idAndVersion: string,
): Promise<string | undefined> {
  const docPath = join(dataDir, 'agent', `${idAndVersion}.json`);
  try {
    const raw = await readFile(docPath, 'utf-8');
    const doc = JSON.parse(raw) as Record<string, unknown>;
    const candidate = (doc.jacsAgentName ?? doc.name) as unknown;
    if (typeof candidate === 'string' && candidate.length > 0) {
      return candidate;
    }
    return undefined;
  } catch {
    return undefined;
  }
}

async function loadCanonical(
  json: Record<string, unknown>,
  configDir: string,
): Promise<AgentConfig> {
  const idAndVersion = json.jacs_agent_id_and_version as string;
  const { agentId, version } = parseAgentIdAndVersion(idAndVersion);
  const keyDir = resolveAbsoluteDir(json.jacs_key_directory as string, configDir);
  const dataDirRaw = (json.jacs_data_directory as string | undefined) ?? 'data';
  const dataDir = resolveAbsoluteDir(dataDirRaw, configDir);
  const nameFromDoc = await readAgentNameFromDoc(dataDir, idAndVersion);

  const privateKeyPath = json.jacs_agent_private_key_filename as string | undefined;

  return {
    jacsAgentName: nameFromDoc ?? idAndVersion,
    jacsAgentVersion: version,
    jacsKeyDir: keyDir,
    jacsId: agentId,
    jacsPrivateKeyPath: privateKeyPath
      ? (isAbsolute(privateKeyPath) ? privateKeyPath : resolve(keyDir, privateKeyPath))
      : undefined,
  };
}

function loadLegacy(json: Record<string, unknown>, configDir: string): AgentConfig {
  const name = json.jacsAgentName as string;
  const version = json.jacsAgentVersion as string;
  const keyDir = resolveAbsoluteDir(json.jacsKeyDir as string, configDir);
  const privateKeyPath = (json.jacsPrivateKeyPath ?? json.private_key_path) as string | undefined;

  return {
    jacsAgentName: name,
    jacsAgentVersion: version,
    jacsKeyDir: keyDir,
    jacsId: (json.jacsId ?? json.jacs_id) as string | undefined,
    jacsPrivateKeyPath: privateKeyPath
      ? (isAbsolute(privateKeyPath) ? privateKeyPath : resolve(configDir, privateKeyPath))
      : undefined,
  };
}

/**
 * Load JACS agent configuration from a jacs.config.json file.
 *
 * Resolves config path in order:
 * 1. Explicit configPath argument
 * 2. JACS_CONFIG_PATH environment variable
 * 3. ./jacs.config.json (current directory)
 *
 * Accepts two config formats:
 * - Canonical Rust/JACS format (snake_case, written by `haiai init`): detected
 *   by presence of `jacs_agent_id_and_version` + `jacs_key_directory`. Agent
 *   name is read from the signed agent document at
 *   `{jacs_data_directory}/agent/{jacs_agent_id_and_version}.json`.
 * - Legacy camelCase format (older Node/Python wrapper output): detected by
 *   presence of `jacsAgentName` + `jacsAgentVersion` + `jacsKeyDir`.
 */
export async function loadConfig(configPath?: string): Promise<AgentConfig> {
  const resolvedPath = configPath
    ?? process.env.JACS_CONFIG_PATH
    ?? './jacs.config.json';
  const absoluteConfigPath = resolve(resolvedPath);
  const configDir = dirname(absoluteConfigPath);

  const raw = await readFile(absoluteConfigPath, 'utf-8');
  const json = JSON.parse(raw) as Record<string, unknown>;

  const isCanonical = CANONICAL_FIELDS.every((k) => typeof json[k] === 'string' && (json[k] as string).length > 0);
  const isLegacy = LEGACY_FIELDS.every((k) => typeof json[k] === 'string' && (json[k] as string).length > 0);

  if (isCanonical) {
    return loadCanonical(json, configDir);
  }
  if (isLegacy) {
    return loadLegacy(json, configDir);
  }

  const missingCanonical = CANONICAL_FIELDS.filter((k) => !(typeof json[k] === 'string' && (json[k] as string).length > 0));
  const missingLegacy = LEGACY_FIELDS.filter((k) => !(typeof json[k] === 'string' && (json[k] as string).length > 0));
  throw new Error(
    'JACS config has neither canonical nor legacy fields. ' +
      `Canonical missing: ${missingCanonical.join(', ')}; ` +
      `Legacy missing: ${missingLegacy.join(', ')}`,
  );
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
    try {
      await readFile(config.jacsPrivateKeyPath, 'utf-8');
    } catch {
      throw new Error(
        `Configured jacsPrivateKeyPath does not exist: ${config.jacsPrivateKeyPath}`,
      );
    }
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
