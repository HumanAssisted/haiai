import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import type { AgentConfig } from './types.js';

/**
 * Load JACS agent configuration from a jacs.config.json file.
 *
 * Resolves config path in order:
 * 1. Explicit configPath argument
 * 2. JACS_CONFIG_PATH environment variable
 * 3. ./jacs.config.json (current directory)
 */
export async function loadConfig(configPath?: string): Promise<AgentConfig> {
  const resolved = configPath
    ?? process.env.JACS_CONFIG_PATH
    ?? './jacs.config.json';

  const raw = await readFile(resolved, 'utf-8');
  const json = JSON.parse(raw) as Record<string, unknown>;

  return {
    jacsAgentName: (json.jacsAgentName ?? json.agent_name ?? 'unnamed-agent') as string,
    jacsAgentVersion: (json.jacsAgentVersion ?? json.agent_version ?? '1.0.0') as string,
    jacsKeyDir: (json.jacsKeyDir ?? json.key_dir ?? '.') as string,
    jacsId: (json.jacsId ?? json.jacs_id) as string | undefined,
    jacsPrivateKeyPath: (json.jacsPrivateKeyPath ?? json.private_key_path) as string | undefined,
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

  candidates.push(
    join(keyDir, 'private_key.pem'),
    join(keyDir, 'agent_private_key.pem'),
    join(keyDir, 'test_agent_private_key.pem'),
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
