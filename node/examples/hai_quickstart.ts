/**
 * HAI SDK Quickstart (TypeScript) -- register an agent, say hello, run a benchmark.
 *
 * Prerequisites:
 *     npm install @haiai/haiai @hai.ai/jacs
 *
 * Usage (new agent):
 *     export JACS_PRIVATE_KEY_PASSWORD=dev-password
 *     npx tsx examples/hai_quickstart.ts
 *   or:
 *     export JACS_PASSWORD_FILE=/secure/path/password.txt
 *     npx tsx examples/hai_quickstart.ts
 *
 * Usage (existing agent with jacs.config.json):
 *     export JACS_PRIVATE_KEY_PASSWORD=dev-password
 *     npx tsx examples/hai_quickstart.ts --existing
 *   or:
 *     export JACS_PASSWORD_FILE=/secure/path/password.txt
 *     npx tsx examples/hai_quickstart.ts --existing
 *
 * Configure exactly one password source.
 */

import { HaiClient } from '../src/client.js';
import { JacsClient } from '@hai.ai/jacs/client';

const HAI_URL = 'https://hai.ai';
const CONFIG_PATH = './jacs.config.json';

async function quickstartNewAgent(): Promise<void> {
  // 1. Create/load a local JACS identity (mandatory identity fields).
  await JacsClient.quickstart({
    name: 'my-quickstart-agent',
    domain: 'agent.example.com',
    description: 'HAIAI quickstart agent',
    algorithm: 'pq2025',
    configPath: CONFIG_PATH,
  });
  const client = await HaiClient.create({ url: HAI_URL, configPath: CONFIG_PATH });

  // Register this JACS identity with HAI.
  console.log('=== Step 1: Register a new JACS agent with HAI ===');
  const reg = await client.register({
    ownerEmail: 'you@example.com',
    domain: 'agent.example.com',
    description: 'HAIAI quickstart agent',
  });
  console.log(`Agent ID: ${reg.agentId}`);
  console.log(`Registered at: ${reg.registeredAt}`);

  // 2. Hello world -- verify signed connectivity
  console.log('\n=== Step 2: Hello world ===');
  const hello = await client.hello();
  console.log(`Message:   ${hello.message}`);
  console.log(`Timestamp: ${hello.timestamp}`);
  console.log(`Hello ID:  ${hello.helloId}`);

  // 3. Check registration status
  console.log('\n=== Step 3: Check status ===');
  const status = await client.verify();
  console.log(`Registered: ${status.registered}`);
  console.log(`JACS ID:    ${status.jacsId}`);

  // 4. Run a free benchmark
  console.log('\n=== Step 4: Free benchmark run ===');
  const run = await client.freeChaoticRun();
  console.log(`Run ID:    ${run.runId}`);
  console.log(`Transcript turns: ${run.transcript.length}`);
  if (run.upsellMessage) {
    console.log(`Upsell: ${run.upsellMessage}`);
  }

  console.log('\nQuickstart complete!');
}

async function quickstartExistingAgent(): Promise<void> {
  // 1. Load existing config
  console.log('=== Loading existing config (configure exactly one password source) ===');
  const client = await HaiClient.create({ url: HAI_URL, configPath: CONFIG_PATH });

  // 2. Test connection
  console.log('\n=== Test connection ===');
  const connected = await client.testConnection();
  console.log(`Connected: ${connected}`);
  if (!connected) {
    console.error('Cannot reach HAI server. Check your network.');
    process.exit(1);
  }

  // 3. Hello world
  console.log('\n=== Hello world ===');
  const hello = await client.hello();
  console.log(`Message:   ${hello.message}`);
  console.log(`Hello ID:  ${hello.helloId}`);

  // 4. Free benchmark
  console.log('\n=== Free benchmark run ===');
  const run = await client.freeChaoticRun();
  console.log(`Run ID:    ${run.runId}`);
  console.log(`Transcript turns: ${run.transcript.length}`);

  console.log('\nDone!');
}

// Parse CLI args
const isExisting = process.argv.includes('--existing');
if (isExisting) {
  quickstartExistingAgent().catch(console.error);
} else {
  quickstartNewAgent().catch(console.error);
}
