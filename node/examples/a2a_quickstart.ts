/**
 * A2A (Agent-to-Agent) quickstart using HAISDK facade APIs.
 *
 * Demonstrates:
 *   1. Initialize a JACS client + HAI client
 *   2. Export an A2A agent card via haisdk facade
 *   3. Sign and verify A2A task artifacts
 *   4. Build chain-of-custody output
 *   5. Generate .well-known discovery documents
 *
 * Prerequisites:
 *   npm install haisdk @hai.ai/jacs
 *
 * Usage:
 *   npx tsx examples/a2a_quickstart.ts
 */

import { JacsClient } from '@hai.ai/jacs/client';
import {
  createChainOfCustody,
  generateWellKnownDocuments,
  HaiClient,
  registerWithAgentCard,
  signArtifact,
  verifyArtifact,
} from '../src/index.js';

const HAI_URL = 'https://hai.ai';
const A2A_OPTIONS = { trustPolicy: 'verified' as const };

async function main(): Promise<void> {
  console.log('=== Step 1: Initialize JACS + HAI clients ===');
  const jacs = await JacsClient.quickstart({
    name: 'hai-agent',
    domain: 'agent.example.com',
    description: 'HAISDK agent',
    algorithm: 'pq2025',
  });
  const hai = await HaiClient.create({ url: HAI_URL });

  console.log('\n=== Step 2: Register with embedded A2A agent card metadata ===');
  const localJacsId = hai.jacsId;
  const agentData = {
    jacsId: localJacsId,
    jacsName: 'a2a-demo-agent',
    jacsVersion: '1.0.0',
    jacsAgentDomain: 'demo.example.com',
    a2aProfile: '1.0',
    jacsServices: [
      {
        name: 'conflict_mediation',
        serviceDescription: 'Mediate disputes with signed provenance artifacts.',
      },
    ],
  } as Record<string, unknown>;

  const registered = await registerWithAgentCard(
    hai,
    jacs,
    agentData,
    {
      ownerEmail: 'you@example.com',
      description: 'A2A facade quickstart agent',
      agentJson: {
        jacsId: localJacsId,
        name: 'a2a-demo-agent',
      },
      ...A2A_OPTIONS,
    },
  );
  const registration = registered.registration as Record<string, unknown>;
  const jacsId = (registration.jacsId as string) || (registration.jacs_id as string) || localJacsId;
  const agentCard = registered.agentCard;
  console.log(`Agent registered with ID: ${jacsId}`);
  console.log(JSON.stringify(agentCard, null, 2));

  console.log('\n=== Step 3: Sign and verify task artifact ===');
  const taskArtifact = {
    taskId: 'task-001',
    operation: 'mediate_conflict',
    input: {
      parties: ['Alice', 'Bob'],
      topic: 'Resource allocation disagreement',
    },
  } as Record<string, unknown>;

  const wrappedTask = await signArtifact(jacs, taskArtifact, 'task', null, A2A_OPTIONS) as Record<string, unknown>;
  const verification = await verifyArtifact(jacs, wrappedTask, A2A_OPTIONS) as Record<string, unknown>;
  console.log(`Valid: ${String(verification.valid)}`);
  console.log(`Signer: ${String(verification.signerId ?? '')}`);
  console.log(`Type: ${String(verification.artifactType ?? '')}`);

  console.log('\n=== Step 4: Chain of custody ===');
  const resultArtifact = {
    taskId: 'task-001',
    result: 'Mediation successful -- both parties agreed to a shared schedule.',
  } as Record<string, unknown>;

  const wrappedResult = await signArtifact(
    jacs,
    resultArtifact,
    'task-result',
    [wrappedTask],
    A2A_OPTIONS,
  ) as Record<string, unknown>;

  const chain = await createChainOfCustody(
    jacs,
    [wrappedTask, wrappedResult],
    A2A_OPTIONS,
  ) as Record<string, unknown>;
  console.log(JSON.stringify(chain, null, 2));

  console.log('\n=== Step 5: .well-known document bundle ===');
  const { publicKeyPem } = hai.exportKeys();
  const publicKeyB64 = Buffer.from(publicKeyPem, 'utf-8').toString('base64url');
  const wellKnown = await generateWellKnownDocuments(
    jacs,
    agentCard,
    '',
    publicKeyB64,
    agentData,
    A2A_OPTIONS,
  ) as Record<string, unknown>;

  for (const [path, doc] of Object.entries(wellKnown)) {
    const preview = JSON.stringify(doc, null, 2);
    console.log(`\n${path}:`);
    console.log(preview.length > 220 ? `${preview.slice(0, 220)}...` : preview);
  }

  console.log('\nA2A quickstart complete.');
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
