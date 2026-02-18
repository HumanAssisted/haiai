#!/usr/bin/env node
/**
 * HAI SDK MCP Server - Expose HAI SDK methods as MCP tools.
 *
 * Start: npx haisdk-mcp
 * Compatible with Claude Desktop, Cursor, and other MCP clients.
 */
import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from '@modelcontextprotocol/sdk/types.js';
import { HaiClient } from './client.js';

const TOOLS = [
  {
    name: 'hai_register_agent',
    description: 'Register a new JACS agent with HAI.AI',
    inputSchema: {
      type: 'object' as const,
      properties: {
        name: { type: 'string', description: 'Agent name' },
        owner_email: { type: 'string', description: 'Owner email address' },
        domain: { type: 'string', description: 'Domain for DNS verification' },
      },
      required: ['name', 'owner_email'],
    },
  },
  {
    name: 'hai_hello',
    description: 'Perform a hello-world handshake with HAI',
    inputSchema: {
      type: 'object' as const,
      properties: {
        include_test: { type: 'boolean', description: 'Request a test scenario preview' },
      },
    },
  },
  {
    name: 'hai_verify_agent',
    description: 'Check agent verification and registration status',
    inputSchema: {
      type: 'object' as const,
      properties: {
        jacs_id: { type: 'string', description: 'JACS ID of the agent to verify' },
      },
    },
  },
  {
    name: 'hai_check_username',
    description: 'Check if a username is available on HAI',
    inputSchema: {
      type: 'object' as const,
      properties: {
        username: { type: 'string', description: 'Username to check' },
      },
      required: ['username'],
    },
  },
  {
    name: 'hai_claim_username',
    description: 'Claim a username for an agent',
    inputSchema: {
      type: 'object' as const,
      properties: {
        agent_id: { type: 'string', description: 'JACS ID of the agent' },
        username: { type: 'string', description: 'Username to claim' },
      },
      required: ['agent_id', 'username'],
    },
  },
  {
    name: 'hai_run_benchmark',
    description: 'Run a benchmark on HAI',
    inputSchema: {
      type: 'object' as const,
      properties: {
        tier: { type: 'string', description: 'Benchmark tier: free, dns_certified, or fully_certified', default: 'free' },
        name: { type: 'string', description: 'Benchmark run name' },
      },
    },
  },
  {
    name: 'hai_send_email',
    description: 'Send an email from the agent\'s @hai.ai address',
    inputSchema: {
      type: 'object' as const,
      properties: {
        to: { type: 'string', description: 'Recipient email address' },
        subject: { type: 'string', description: 'Email subject' },
        body: { type: 'string', description: 'Email body' },
        in_reply_to: { type: 'string', description: 'Message ID to reply to' },
      },
      required: ['to', 'subject', 'body'],
    },
  },
  {
    name: 'hai_list_messages',
    description: 'List email messages for the agent',
    inputSchema: {
      type: 'object' as const,
      properties: {
        limit: { type: 'number', description: 'Max messages to return' },
        folder: { type: 'string', description: 'Folder: inbox, outbox, or all' },
      },
    },
  },
  {
    name: 'hai_fetch_key',
    description: 'Look up an agent\'s public key from the HAI directory',
    inputSchema: {
      type: 'object' as const,
      properties: {
        jacs_id: { type: 'string', description: 'JACS ID of the agent' },
        version: { type: 'string', description: 'Key version (default: latest)' },
      },
      required: ['jacs_id'],
    },
  },
];

async function getClient(): Promise<HaiClient> {
  const url = process.env.HAI_API_URL;
  const configPath = process.env.HAI_CONFIG_PATH || process.env.JACS_CONFIG_PATH;
  return HaiClient.create({ url, configPath });
}

async function handleToolCall(
  name: string,
  args: Record<string, unknown>,
): Promise<string> {
  const client = await getClient();

  switch (name) {
    case 'hai_register_agent': {
      const result = await client.registerNewAgent(
        args.name as string,
        {
          ownerEmail: args.owner_email as string,
          domain: args.domain as string | undefined,
        },
      );
      return JSON.stringify(result, null, 2);
    }
    case 'hai_hello': {
      const result = await client.hello((args.include_test as boolean) ?? false);
      return JSON.stringify(result, null, 2);
    }
    case 'hai_verify_agent': {
      const jacsId = args.jacs_id as string | undefined;
      if (jacsId) {
        const result = await client.getAgentAttestation(jacsId);
        return JSON.stringify(result, null, 2);
      }
      const result = await client.verify();
      return JSON.stringify(result, null, 2);
    }
    case 'hai_check_username': {
      const result = await client.checkUsername(args.username as string);
      return JSON.stringify(result, null, 2);
    }
    case 'hai_claim_username': {
      const result = await client.claimUsername(
        args.agent_id as string,
        args.username as string,
      );
      return JSON.stringify(result, null, 2);
    }
    case 'hai_run_benchmark': {
      const tier = (args.tier as string) || 'free';
      const benchName = (args.name as string) || 'mediation_basic';
      const result = await client.benchmark(benchName, tier);
      return JSON.stringify(result, null, 2);
    }
    case 'hai_send_email': {
      const result = await client.sendEmail({
        to: args.to as string,
        subject: args.subject as string,
        body: args.body as string,
        inReplyTo: args.in_reply_to as string | undefined,
      });
      return JSON.stringify(result, null, 2);
    }
    case 'hai_list_messages': {
      const result = await client.listMessages({
        limit: args.limit as number | undefined,
        folder: args.folder as 'inbox' | 'outbox' | 'all' | undefined,
      });
      return JSON.stringify(result, null, 2);
    }
    case 'hai_fetch_key': {
      const result = await client.fetchRemoteKey(
        args.jacs_id as string,
        (args.version as string) || 'latest',
      );
      return JSON.stringify(result, null, 2);
    }
    default:
      throw new Error(`Unknown tool: ${name}`);
  }
}

export async function startServer() {
  const server = new Server(
    { name: 'hai-sdk', version: '0.1.0' },
    { capabilities: { tools: {} } },
  );

  server.setRequestHandler(ListToolsRequestSchema, async () => ({
    tools: TOOLS,
  }));

  server.setRequestHandler(CallToolRequestSchema, async (request) => {
    const { name, arguments: args } = request.params;
    try {
      const text = await handleToolCall(name, (args ?? {}) as Record<string, unknown>);
      return { content: [{ type: 'text' as const, text }] };
    } catch (e) {
      return {
        content: [{ type: 'text' as const, text: `Error: ${(e as Error).message}` }],
        isError: true,
      };
    }
  });

  const transport = new StdioServerTransport();
  await server.connect(transport);
}

// Run if executed directly
startServer();
