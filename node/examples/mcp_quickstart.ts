/**
 * MCP quickstart using HAIAI integration wrappers.
 *
 * Demonstrates:
 *   1. JACS quickstart with required identity fields
 *   2. MCP tool registration via HAIAI -> JACS adapters
 *   3. Expanded toolsets (share/trust helpers)
 *
 * Prerequisites:
 *   npm install @haiai/haiai @hai.ai/jacs @modelcontextprotocol/sdk
 *
 * Usage:
 *   npx tsx examples/mcp_quickstart.ts
 */

import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { JacsClient } from '@hai.ai/jacs/client';
import { getJacsMcpToolDefinitions, registerJacsMcpTools } from '../src/index.js';

async function main(): Promise<void> {
  const jacs = await JacsClient.quickstart({
    name: 'hai-agent',
    domain: 'agent.example.com',
    description: 'HAIAI MCP agent',
    algorithm: 'pq2025',
  });

  const server = new Server(
    { name: 'haiai-example-mcp', version: '1.0.0' },
    { capabilities: { tools: {} } },
  );

  await registerJacsMcpTools(server, jacs);

  const defs = await getJacsMcpToolDefinitions();
  const names = defs
    .map((entry) => {
      const item = entry as Record<string, unknown>;
      return typeof item.name === 'string' ? item.name : '';
    })
    .filter(Boolean);

  console.log('Registered MCP tool definitions:');
  for (const name of names) {
    console.log(`- ${name}`);
  }
}

main().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});

