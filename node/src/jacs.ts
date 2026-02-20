import { spawnSync, type SpawnSyncOptionsWithBufferEncoding, type SpawnSyncReturns } from 'node:child_process';

export interface RunJacsCliOptions {
  /** Override binary path (default: JACS_CLI_BIN or `jacs`). */
  jacsBin?: string;
  /** Working directory for the command. */
  cwd?: string;
  /** Environment variables for the command. */
  env?: NodeJS.ProcessEnv;
  /** stdio mode; defaults to `pipe` for library usage. */
  stdio?: SpawnSyncOptionsWithBufferEncoding['stdio'];
}

export function resolveJacsCliBin(env: NodeJS.ProcessEnv = process.env): string {
  const candidate = (env.JACS_CLI_BIN ?? '').trim();
  return candidate.length > 0 ? candidate : 'jacs';
}

/**
 * Execute a JACS CLI command from library code.
 *
 * Example:
 *   runJacsCli(["verify", "./signed.json"])
 */
export function runJacsCli(
  args: string[],
  options: RunJacsCliOptions = {},
): SpawnSyncReturns<Buffer> {
  const binary = options.jacsBin?.trim() || resolveJacsCliBin(options.env ?? process.env);
  const result = spawnSync(binary, args, {
    cwd: options.cwd,
    env: options.env,
    stdio: options.stdio ?? 'pipe',
    encoding: 'buffer',
  });

  if (result.error) {
    const err = result.error as NodeJS.ErrnoException;
    if (err.code === 'ENOENT') {
      throw new Error(`JACS CLI binary not found: ${binary}. Install jacs or set JACS_CLI_BIN.`);
    }
    throw new Error(`Failed to execute JACS CLI '${binary}': ${err.message}`);
  }

  return result;
}
