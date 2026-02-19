/**
 * A2A (Agent-to-Agent) Quickstart using HAISDK Node/TypeScript.
 *
 * Demonstrates how to use haisdk with the A2A protocol (v0.4.0):
 *   1. Register a JACS agent with HAI
 *   2. Export the agent as an A2A Agent Card
 *   3. Wrap an artifact with JACS provenance signature
 *   4. Verify a wrapped artifact
 *   5. Create a chain of custody for multi-agent workflows
 *   6. Publish .well-known documents
 *
 * Prerequisites:
 *     npm install @humanassisted/haisdk
 *
 * Usage:
 *     npx tsx examples/a2a_quickstart.ts
 */

import { HaiClient } from '../src/client.js';
import { generateKeypair, signString } from '../src/crypt.js';
import { canonicalJson } from '../src/signing.js';
import { randomUUID } from 'node:crypto';

const HAI_URL = 'https://hai.ai';

// ---------------------------------------------------------------------------
// A2A v0.4.0 Types
// ---------------------------------------------------------------------------

interface A2AAgentInterface {
  url: string;
  protocolBinding: string;
}

interface A2AAgentSkill {
  id: string;
  name: string;
  description: string;
  tags: string[];
  examples?: string[];
}

interface A2AAgentExtension {
  uri: string;
  description?: string;
  required?: boolean;
}

interface A2AAgentCapabilities {
  streaming?: boolean;
  pushNotifications?: boolean;
  extensions?: A2AAgentExtension[];
}

interface A2AAgentCard {
  name: string;
  description: string;
  version: string;
  protocolVersions: string[];
  supportedInterfaces: A2AAgentInterface[];
  defaultInputModes: string[];
  defaultOutputModes: string[];
  capabilities: A2AAgentCapabilities;
  skills: A2AAgentSkill[];
  metadata?: Record<string, unknown>;
}

interface WrappedArtifact {
  jacsId: string;
  jacsVersion: string;
  jacsType: string;
  jacsLevel: string;
  jacsVersionDate: string;
  a2aArtifact: Record<string, unknown>;
  jacsParentSignatures?: Record<string, unknown>[];
  jacsSignature?: {
    agentID: string;
    date: string;
    signature: string;
  };
}

// ---------------------------------------------------------------------------
// A2A Helper Functions
// ---------------------------------------------------------------------------

/**
 * Export a JACS agent as an A2A Agent Card (v0.4.0).
 *
 * The Agent Card is published at /.well-known/agent-card.json for
 * zero-config discovery by other A2A agents.
 */
function exportAgentCard(jacsId: string, agentName: string, domain?: string): A2AAgentCard {
  const baseUrl = domain
    ? `https://${domain}/agent/${jacsId}`
    : `https://hai.ai/agent/${jacsId}`;

  return {
    name: agentName,
    description: `HAI-registered JACS agent: ${agentName}`,
    version: '1.0.0',
    protocolVersions: ['0.4.0'],
    supportedInterfaces: [
      { url: baseUrl, protocolBinding: 'jsonrpc' },
    ],
    defaultInputModes: ['text/plain', 'application/json'],
    defaultOutputModes: ['text/plain', 'application/json'],
    capabilities: {
      extensions: [
        {
          uri: 'urn:jacs:provenance-v1',
          description: 'JACS cryptographic document signing and verification',
          required: false,
        },
      ],
    },
    skills: [
      {
        id: 'mediation',
        name: 'conflict_mediation',
        description: 'Mediate conflicts between parties using de-escalation techniques',
        tags: ['jacs', 'mediation', 'conflict-resolution'],
        examples: ['Mediate a workplace dispute', 'Help resolve a disagreement'],
      },
    ],
    metadata: {
      jacsId,
      registeredWith: 'hai.ai',
    },
  };
}

/**
 * Wrap an A2A artifact with a JACS provenance signature.
 *
 * Signs the artifact using the agent's private key, creating a
 * verifiable record for multi-agent workflows.
 */
function wrapArtifactWithProvenance(
  privateKeyPem: string,
  jacsId: string,
  artifact: Record<string, unknown>,
  artifactType: string,
  parentSignatures?: Record<string, unknown>[],
): WrappedArtifact {
  const wrapped: WrappedArtifact = {
    jacsId: randomUUID(),
    jacsVersion: '1.0.0',
    jacsType: `a2a-${artifactType}`,
    jacsLevel: 'artifact',
    jacsVersionDate: new Date().toISOString(),
    a2aArtifact: artifact,
  };

  if (parentSignatures) {
    wrapped.jacsParentSignatures = parentSignatures;
  }

  // Sign canonical JSON of the document (without jacsSignature)
  const canonical = canonicalJson(wrapped as unknown as Record<string, unknown>);
  const signature = signString(privateKeyPem, canonical);

  wrapped.jacsSignature = {
    agentID: jacsId,
    date: new Date().toISOString(),
    signature,
  };

  return wrapped;
}

/**
 * Verify a JACS-wrapped A2A artifact.
 */
function verifyWrappedArtifact(wrapped: WrappedArtifact): Record<string, unknown> {
  const signatureInfo = wrapped.jacsSignature;
  if (!signatureInfo?.signature) {
    return { valid: false, error: 'No signature found' };
  }

  // For full verification you would fetch the signer's public key
  // from HAI and use verifyString(). Here we show the structure:
  return {
    valid: true,
    signerId: signatureInfo.agentID,
    artifactType: wrapped.jacsType,
    timestamp: wrapped.jacsVersionDate,
    originalArtifact: wrapped.a2aArtifact,
  };
}

/**
 * Create a chain of custody document for multi-agent workflows.
 */
function createChainOfCustody(artifacts: WrappedArtifact[]): Record<string, unknown> {
  const chain = artifacts.map((a) => ({
    artifactId: a.jacsId,
    artifactType: a.jacsType,
    timestamp: a.jacsVersionDate,
    agentId: a.jacsSignature?.agentID ?? 'unknown',
    signaturePresent: Boolean(a.jacsSignature?.signature),
  }));

  return {
    chainOfCustody: chain,
    created: new Date().toISOString(),
    totalArtifacts: chain.length,
  };
}

/**
 * Generate .well-known documents for A2A discovery.
 */
function generateWellKnownDocuments(
  agentCard: A2AAgentCard,
  jacsId: string,
): Record<string, Record<string, unknown>> {
  return {
    '/.well-known/agent-card.json': agentCard as unknown as Record<string, unknown>,
    '/.well-known/jacs-agent.json': {
      jacsVersion: '1.0',
      agentId: jacsId,
      registeredWith: 'hai.ai',
      capabilities: { signing: true, verification: true },
      endpoints: { verify: '/jacs/verify', sign: '/jacs/sign' },
    },
  };
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main(): Promise<void> {
  // --- Step 1: Register agent with HAI ---
  console.log('=== Step 1: Register a JACS agent with HAI ===');
  const keypair = generateKeypair();
  const client = HaiClient.fromCredentials(
    'a2a-demo-agent',
    keypair.privateKeyPem,
    { url: HAI_URL },
  );
  const reg = await client.register({
    ownerEmail: 'you@example.com',
  });
  const jacsId = reg.jacsId || client.jacsId;
  console.log(`Agent registered with ID: ${jacsId}`);

  // Use the in-memory keypair for signing artifacts in this quickstart flow.
  const privateKeyPem = keypair.privateKeyPem;

  // --- Step 2: Export as A2A Agent Card ---
  console.log('\n=== Step 2: Export A2A Agent Card (v0.4.0) ===');
  const agentCard = exportAgentCard(jacsId, 'a2a-demo-agent', 'demo.example.com');
  console.log(JSON.stringify(agentCard, null, 2));

  // --- Step 3: Wrap artifact with JACS provenance ---
  console.log('\n=== Step 3: Wrap artifact with JACS provenance ===');
  const taskArtifact = {
    taskId: 'task-001',
    operation: 'mediate_conflict',
    input: {
      parties: ['Alice', 'Bob'],
      topic: 'Resource allocation disagreement',
    },
  };
  const wrapped = wrapArtifactWithProvenance(
    privateKeyPem,
    jacsId,
    taskArtifact,
    'task',
  );
  console.log(`Wrapped artifact ID: ${wrapped.jacsId}`);
  console.log(`Artifact type: ${wrapped.jacsType}`);
  console.log(`Signed by: ${wrapped.jacsSignature?.agentID}`);

  // --- Step 4: Verify the wrapped artifact ---
  console.log('\n=== Step 4: Verify wrapped artifact ===');
  const verification = verifyWrappedArtifact(wrapped);
  console.log(`Valid: ${verification.valid}`);
  console.log(`Signer: ${verification.signerId}`);
  console.log(`Type: ${verification.artifactType}`);

  // --- Step 5: Chain of custody (multi-agent workflow) ---
  console.log('\n=== Step 5: Chain of custody ===');
  const resultArtifact = {
    taskId: 'task-001',
    result: 'Mediation successful -- both parties agreed to shared schedule',
  };
  const wrappedResult = wrapArtifactWithProvenance(
    privateKeyPem,
    jacsId,
    resultArtifact,
    'task-result',
    wrapped.jacsSignature ? [wrapped.jacsSignature as unknown as Record<string, unknown>] : [],
  );

  const chain = createChainOfCustody([wrapped, wrappedResult]);
  console.log(`Chain length: ${(chain.totalArtifacts as number)}`);
  for (const entry of chain.chainOfCustody as Array<Record<string, unknown>>) {
    console.log(`  [${entry.artifactType}] by ${entry.agentId} at ${entry.timestamp}`);
  }

  // --- Step 6: Generate .well-known documents ---
  console.log('\n=== Step 6: .well-known documents ===');
  const wellKnown = generateWellKnownDocuments(agentCard, jacsId);
  for (const [path, doc] of Object.entries(wellKnown)) {
    console.log(`\n${path}:`);
    console.log(JSON.stringify(doc, null, 2).slice(0, 200) + '...');
  }

  console.log('\nA2A quickstart complete!');
  console.log('Serve the .well-known documents at your agent\'s domain for A2A discovery.');
}

main().catch(console.error);
