/**
 * HAI SDK Quickstart (TypeScript) -- register an agent, say hello, run a benchmark.
 *
 * Prerequisites:
 *     npm install @humanassisted/haisdk
 *
 * Usage (new agent):
 *     npx tsx examples/hai_quickstart.ts
 *
 * Usage (existing agent with jacs.config.json):
 *     npx tsx examples/hai_quickstart.ts --existing
 */

import { HaiClient } from '../src/client.js';
import { generateKeypair } from '../src/crypt.js';

const HAI_URL = 'https://hai.ai';

async function quickstartNewAgent(): Promise<void> {
  // 1. Bootstrap an in-memory client for first-time registration.
  //    Persist keys/config with the CLI for real usage.
  const keypair = generateKeypair();
  const client = HaiClient.fromCredentials(
    'my-quickstart-agent',
    keypair.privateKeyPem,
    { url: HAI_URL },
  );

  // Register this JACS identity with HAI.
  console.log('=== Step 1: Register a new JACS agent with HAI ===');
  const reg = await client.register({
    ownerEmail: 'you@example.com',
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
  console.log('=== Loading existing config ===');
  const client = await HaiClient.create({ url: HAI_URL });

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
