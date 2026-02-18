#!/usr/bin/env node
/**
 * HAI SDK CLI - Register, test, and manage AI agents.
 *
 * Usage: npx haisdk <command> [options]
 */
import { HaiClient } from './client.js';
import { generateKeypair } from './crypt.js';
import { chmod, mkdir, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';

const USAGE = `Usage: haisdk <command> [options]

Commands:
  register         Register a new agent with HAI
  hello            Perform a hello-world handshake
  benchmark        Run a benchmark
  verify           Check agent verification status
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
  process.stderr.write(`Error: ${message}\n`);
  process.exit(1);
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

async function main() {
  const args = process.argv.slice(2);

  if (args.length === 0 || args.includes('--help') || args.includes('-h')) {
    process.stdout.write(USAGE);
    process.exit(0);
  }

  if (args.includes('--version') || args.includes('-v')) {
    process.stdout.write('haisdk 0.1.0\n');
    process.exit(0);
  }

  const command = args[0];
  const cmdArgs = args.slice(1);

  if (cmdArgs.includes('--help') || cmdArgs.includes('-h')) {
    showCommandHelp(command);
    process.exit(0);
  }

  try {
    switch (command) {
      case 'register':
        await cmdRegister(cmdArgs);
        break;
      case 'hello':
        await cmdHello(cmdArgs);
        break;
      case 'benchmark':
        await cmdBenchmark(cmdArgs);
        break;
      case 'verify':
        await cmdVerify(cmdArgs);
        break;
      case 'check-username':
        await cmdCheckUsername(cmdArgs);
        break;
      case 'claim-username':
        await cmdClaimUsername(cmdArgs);
        break;
      case 'send-email':
        await cmdSendEmail(cmdArgs);
        break;
      case 'list-messages':
        await cmdListMessages(cmdArgs);
        break;
      case 'email-status':
        await cmdEmailStatus(cmdArgs);
        break;
      case 'fetch-key':
        await cmdFetchKey(cmdArgs);
        break;
      default:
        fail(`Unknown command: ${command}\n\n${USAGE}`);
    }
  } catch (e) {
    fail((e as Error).message);
  }
}

function showCommandHelp(command: string) {
  const help: Record<string, string> = {
    register: 'Usage: haisdk register --name <name> --description <text> --dns <domain> --owner-email <email> [--key-dir <path>] [--config-path <path>] [--url <api-url>]',
    hello: 'Usage: haisdk hello [--include-test] [--url <api-url>]',
    benchmark: 'Usage: haisdk benchmark [--tier free|dns_certified|fully_certified] [--url <api-url>]',
    verify: 'Usage: haisdk verify [--jacs-id <id>] [--url <api-url>]',
    'check-username': 'Usage: haisdk check-username --username <name> [--url <api-url>]',
    'claim-username': 'Usage: haisdk claim-username --username <name> --agent-id <id> [--url <api-url>]',
    'send-email': 'Usage: haisdk send-email --to <addr> --subject <subj> --body <body> [--url <api-url>]',
    'list-messages': 'Usage: haisdk list-messages [--limit <n>] [--folder inbox|outbox|all] [--url <api-url>]',
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

  // Bootstrap registration must work before any local config exists.
  const keypair = generateKeypair();
  const client = HaiClient.fromCredentials(
    name,
    keypair.privateKeyPem,
    url ? { url } : undefined,
  );

  const result = await client.register({
    ownerEmail,
    description,
    domain: dns,
  });

  await mkdir(keyDir, { recursive: true, mode: 0o700 });
  try {
    await chmod(keyDir, 0o700);
  } catch {
    // Best effort on non-POSIX filesystems.
  }

  const privateKeyPath = join(keyDir, 'agent_private_key.pem');
  const publicKeyPath = join(keyDir, 'agent_public_key.pem');
  await writeFile(privateKeyPath, keypair.privateKeyPem, { mode: 0o600 });
  try {
    await chmod(privateKeyPath, 0o600);
  } catch {
    // Best effort on non-POSIX filesystems.
  }
  await writeFile(publicKeyPath, keypair.publicKeyPem, { mode: 0o644 });

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

async function cmdVerify(args: string[]) {
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
  const folder = getArg(args, '--folder') as 'inbox' | 'outbox' | 'all' | undefined;
  const result = await client.listMessages({
    limit: limit ? parseInt(limit, 10) : undefined,
    folder,
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

main();
