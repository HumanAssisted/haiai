#!/usr/bin/env node
/**
 * HAI SDK CLI - Register, test, and manage AI agents.
 *
 * Usage: npx haisdk <command> [options]
 */
import { HaiClient } from './client.js';
import { createAgentSync } from '@hai.ai/jacs';
import { loadPrivateKeyPassphrase } from './config.js';
import { runJacsCli } from './jacs.js';
import { chmod, mkdir, writeFile, readFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';

const USAGE = `Usage: haisdk <command> [options]

Commands:
  register         Register a new agent with HAI
  hello            Perform a hello-world handshake
  benchmark        Run a benchmark
  status           Check agent verification status
  check-username   Check username availability
  claim-username   Claim a username for an agent
  send-email       Send an email from the agent
  list-messages    List email messages
  email-status     Get email rate limit status
  fetch-key        Look up an agent's public key

Options:
  --help, -h       Show help
  --version, -v    Show version
`;

function fail(message: string): never {
  throw new Error(message);
}

function getArg(args: string[], flag: string): string | undefined {
  const idx = args.indexOf(flag);
  if (idx === -1 || idx + 1 >= args.length) return undefined;
  return args[idx + 1];
}

function requireArg(args: string[], flag: string, label: string): string {
  const val = getArg(args, flag);
  if (!val) fail(`${label} is required (${flag})`);
  return val;
}

const HAI_COMMANDS = new Set([
  'register',
  'hello',
  'benchmark',
  'status',
  'check-username',
  'claim-username',
  'send-email',
  'list-messages',
  'email-status',
  'fetch-key',
]);

/**
 * Determine whether CLI args should be forwarded to `jacs`.
 * - `haisdk jacs ...` forwards explicitly.
 * - unknown top-level commands forward transparently.
 */
export function resolveJacsPassthroughArgs(args: string[]): string[] | null {
  if (args.length === 0) return null;
  const command = args[0];

  if (command === 'jacs') {
    return args.slice(1);
  }

  if (!command.startsWith('-') && !HAI_COMMANDS.has(command)) {
    return args;
  }

  return null;
}

export async function main(argv: string[] = process.argv.slice(2)): Promise<number> {
  const args = argv;

  try {
    const passthroughArgs = resolveJacsPassthroughArgs(args);
    if (passthroughArgs) {
      const result = runJacsCli(passthroughArgs, { stdio: 'inherit' });
      return result.status ?? 1;
    }

    if (args.length === 0 || args.includes('--help') || args.includes('-h')) {
      process.stdout.write(USAGE);
      return 0;
    }

    if (args.includes('--version') || args.includes('-v')) {
      process.stdout.write('haisdk 0.1.0\n');
      return 0;
    }

    const command = args[0];
    const cmdArgs = args.slice(1);

    if (cmdArgs.includes('--help') || cmdArgs.includes('-h')) {
      showCommandHelp(command);
      return 0;
    }

    switch (command) {
      case 'register':
        await cmdRegister(cmdArgs);
        return 0;
      case 'hello':
        await cmdHello(cmdArgs);
        return 0;
      case 'benchmark':
        await cmdBenchmark(cmdArgs);
        return 0;
      case 'status':
        await cmdStatus(cmdArgs);
        return 0;
      case 'check-username':
        await cmdCheckUsername(cmdArgs);
        return 0;
      case 'claim-username':
        await cmdClaimUsername(cmdArgs);
        return 0;
      case 'send-email':
        await cmdSendEmail(cmdArgs);
        return 0;
      case 'list-messages':
        await cmdListMessages(cmdArgs);
        return 0;
      case 'email-status':
        await cmdEmailStatus(cmdArgs);
        return 0;
      case 'fetch-key':
        await cmdFetchKey(cmdArgs);
        return 0;
      default:
        fail(`Unknown command: ${command}\n\n${USAGE}`);
    }
  } catch (e) {
    process.stderr.write(`Error: ${(e as Error).message}\n`);
    return 1;
  }
}

function showCommandHelp(command: string) {
  const help: Record<string, string> = {
    register: 'Usage: haisdk register --name <name> --description <text> --dns <domain> --owner-email <email> [--key-dir <path>] [--config-path <path>] [--url <api-url>]',
    hello: 'Usage: haisdk hello [--include-test] [--url <api-url>]',
    benchmark: 'Usage: haisdk benchmark [--tier free|dns_certified|fully_certified] [--url <api-url>]',
    status: 'Usage: haisdk status [--jacs-id <id>] [--url <api-url>]',
    'check-username': 'Usage: haisdk check-username --username <name> [--url <api-url>]',
    'claim-username': 'Usage: haisdk claim-username --username <name> --agent-id <id> [--url <api-url>]',
    'send-email': 'Usage: haisdk send-email --to <addr> --subject <subj> --body <body> [--url <api-url>]',
    'list-messages': 'Usage: haisdk list-messages [--limit <n>] [--direction inbound|outbound] [--url <api-url>]',
    'email-status': 'Usage: haisdk email-status [--url <api-url>]',
    'fetch-key': 'Usage: haisdk fetch-key --jacs-id <id> [--version <ver>] [--url <api-url>]',
  };
  process.stdout.write((help[command] || `Unknown command: ${command}`) + '\n');
}

async function createClient(args: string[]): Promise<HaiClient> {
  const url = getArg(args, '--url');
  return HaiClient.create(url ? { url } : undefined);
}

async function cmdRegister(args: string[]) {
  const name = requireArg(args, '--name', 'Agent name');
  const description = requireArg(args, '--description', 'Description');
  const dns = getArg(args, '--dns') ?? getArg(args, '--domain');
  if (!dns) fail('DNS domain is required (--dns)');
  const ownerEmail = requireArg(args, '--owner-email', 'Owner email');
  const url = getArg(args, '--url');
  const keyDir = resolve(getArg(args, '--key-dir') ?? join(homedir(), '.jacs', 'keys'));
  const configPath = resolve(getArg(args, '--config-path') ?? './jacs.config.json');

  // Generate JACS agent (keys + config) via JACS core
  const keyPassphrase = await loadPrivateKeyPassphrase();
  const dataDir = join(dirname(configPath), 'jacs_data');

  const resultJson = createAgentSync(
    name,
    keyPassphrase,
    'pq2025',
    dataDir,
    keyDir,
    configPath,
    null,
    description,
    dns,
    null,
  );
  const createResult = JSON.parse(resultJson);

  // Read the generated public key
  const pubKeyPath = createResult.public_key_path || join(keyDir, 'jacs.public.pem');
  const privKeyPath = createResult.private_key_path || join(keyDir, 'jacs.private.pem.enc');
  const publicKeyPem = await readFile(pubKeyPath, 'utf-8');
  const privateKeyPem = await readFile(privKeyPath, 'utf-8');

  const client = await HaiClient.fromCredentials(
    name,
    privateKeyPem,
    url ? { url, privateKeyPassphrase: keyPassphrase } : { privateKeyPassphrase: keyPassphrase },
  );

  const result = await client.register({
    ownerEmail,
    description,
    domain: dns,
    publicKeyPem,
  });

  // Ensure key dir permissions
  try {
    await chmod(keyDir, 0o700);
  } catch {
    // Best effort on non-POSIX filesystems.
  }
  try {
    await chmod(privKeyPath, 0o600);
  } catch {
    // Best effort on non-POSIX filesystems.
  }

  // Update config for haisdk format
  const configData = {
    jacsAgentName: name,
    jacsAgentVersion: '1.0.0',
    jacsKeyDir: keyDir,
    jacsId: result.jacsId || name,
  };
  await mkdir(dirname(configPath), { recursive: true });
  await writeFile(configPath, JSON.stringify(configData, null, 2) + '\n', { mode: 0o600 });

  process.stdout.write(JSON.stringify({
    ...result,
    configPath,
    keyDir,
  }, null, 2) + '\n');
}

async function cmdHello(args: string[]) {
  const client = await createClient(args);
  const includeTest = args.includes('--include-test');
  const result = await client.hello(includeTest);
  process.stdout.write(JSON.stringify(result, null, 2) + '\n');
}

async function cmdBenchmark(args: string[]) {
  const client = await createClient(args);
  const tier = getArg(args, '--tier') || 'free';
  const result = await client.benchmark('mediation_basic', tier);
  process.stdout.write(JSON.stringify(result, null, 2) + '\n');
}

async function cmdStatus(args: string[]) {
  const client = await createClient(args);
  const jacsId = getArg(args, '--jacs-id');
  if (jacsId) {
    const result = await client.getAgentAttestation(jacsId);
    process.stdout.write(JSON.stringify(result, null, 2) + '\n');
  } else {
    const result = await client.verify();
    process.stdout.write(JSON.stringify(result, null, 2) + '\n');
  }
}

async function cmdCheckUsername(args: string[]) {
  const username = requireArg(args, '--username', 'Username');
  const client = await createClient(args);
  const result = await client.checkUsername(username);
  process.stdout.write(JSON.stringify(result, null, 2) + '\n');
}

async function cmdClaimUsername(args: string[]) {
  const username = requireArg(args, '--username', 'Username');
  const agentId = requireArg(args, '--agent-id', 'Agent ID');
  const client = await createClient(args);
  const result = await client.claimUsername(agentId, username);
  process.stdout.write(JSON.stringify(result, null, 2) + '\n');
}

async function cmdSendEmail(args: string[]) {
  const to = requireArg(args, '--to', 'Recipient');
  const subject = requireArg(args, '--subject', 'Subject');
  const body = requireArg(args, '--body', 'Body');
  const client = await createClient(args);
  const result = await client.sendEmail({ to, subject, body });
  process.stdout.write(JSON.stringify(result, null, 2) + '\n');
}

async function cmdListMessages(args: string[]) {
  const client = await createClient(args);
  const limit = getArg(args, '--limit');
  const direction = getArg(args, '--direction') as 'inbound' | 'outbound' | undefined;
  const result = await client.listMessages({
    limit: limit ? parseInt(limit, 10) : undefined,
    direction,
  });
  process.stdout.write(JSON.stringify(result, null, 2) + '\n');
}

async function cmdEmailStatus(args: string[]) {
  const client = await createClient(args);
  const result = await client.getEmailStatus();
  process.stdout.write(JSON.stringify(result, null, 2) + '\n');
}

async function cmdFetchKey(args: string[]) {
  const jacsId = requireArg(args, '--jacs-id', 'JACS ID');
  const version = getArg(args, '--version');
  const client = await createClient(args);
  const result = await client.fetchRemoteKey(jacsId, version);
  process.stdout.write(JSON.stringify(result, null, 2) + '\n');
}

const isMainModule = Boolean(process.argv[1] && /(^|[\\/])cli(\.js)?$/.test(process.argv[1]));

if (isMainModule) {
  main()
    .then((code) => process.exit(code))
    .catch((e) => {
      process.stderr.write(`Error: ${(e as Error).message}\n`);
      process.exit(1);
    });
}
