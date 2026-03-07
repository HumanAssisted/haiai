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
 * Enforce local MCP execution policy when invoking `jacs mcp run`.
 *
 * We only allow `--bin` passthrough here and reject all forwarded runtime args
 * so transport cannot be switched away from stdio.
 */
export function enforceMcpRunStdioArgs(args: string[]): string[] {
  if (args.length < 2 || args[0] !== 'mcp' || args[1] !== 'run') {
    return args;
  }

  const normalized: string[] = ['mcp', 'run'];
  for (let i = 2; i < args.length; i += 1) {
    const arg = args[i];

    if (arg === '--bin') {
      const bin = args[i + 1];
      if (!bin) {
        throw new Error('Missing value for --bin');
      }
      normalized.push('--bin', bin);
      i += 1;
      continue;
    }

    if (arg.startsWith('--bin=')) {
      normalized.push(arg);
      continue;
    }

    throw new Error(
      '`jacs mcp run` is stdio-only in haiai. ' +
      'Only optional `--bin <path>` is allowed; transport/runtime overrides are blocked.',
    );
  }

  return normalized;
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
  const normalizedArgs = enforceMcpRunStdioArgs(args);
  const binary = options.jacsBin?.trim() || resolveJacsCliBin(options.env ?? process.env);
  const result = spawnSync(binary, normalizedArgs, {
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
