#!/usr/bin/env node
/**
 * Standalone MCP server exposing HAI SDK operations as tools.
 *
 * Usage:
 *   haisdk-mcp                           # stdio transport
 *   npx tsx src/mcp-server.ts            # dev mode
 *   HAI_URL=https://hai.ai haisdk-mcp    # override API endpoint
 */

import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from '@modelcontextprotocol/sdk/types.js';
import { fileURLToPath } from 'node:url';
import { HaiClient } from './client.js';
import { generateVerifyLink } from './verify.js';

export async function getClient(args: Record<string, unknown>): Promise<HaiClient> {
  const options: Record<string, unknown> = {};
  if (typeof args.hai_url === 'string' && args.hai_url) {
    options.url = args.hai_url;
  }
  if (typeof args.config_path === 'string' && args.config_path) {
    options.configPath = args.config_path;
  }
  return HaiClient.create(options as any);
}

function toJSON(obj: unknown): string {
  return JSON.stringify(obj);
}

export const TOOLS = [
  // ----- Identity tools -----
  {
    name: 'hai_hello',
    description: 'Run authenticated hello handshake with HAI',
    inputSchema: {
      type: 'object' as const,
      properties: {
        config_path: { type: 'string', description: 'Path to jacs.config.json' },
        hai_url: { type: 'string', description: 'HAI API URL override' },
      },
    },
  },
  {
    name: 'hai_check_username',
    description: 'Check if a hai.ai username is available',
    inputSchema: {
      type: 'object' as const,
      properties: {
        username: { type: 'string', description: 'Username to check' },
        hai_url: { type: 'string', description: 'HAI API URL override' },
      },
      required: ['username'],
    },
  },
  {
    name: 'hai_claim_username',
    description: 'Claim a hai.ai username for an agent',
    inputSchema: {
      type: 'object' as const,
      properties: {
        agent_id: { type: 'string', description: 'Agent UUID' },
        username: { type: 'string', description: 'Username to claim' },
        config_path: { type: 'string', description: 'Path to jacs.config.json' },
        hai_url: { type: 'string', description: 'HAI API URL override' },
      },
      required: ['agent_id', 'username'],
    },
  },
  {
    name: 'hai_register_agent',
    description: 'Register the local JACS agent with HAI',
    inputSchema: {
      type: 'object' as const,
      properties: {
        owner_email: { type: 'string', description: 'Owner email address' },
        config_path: { type: 'string', description: 'Path to jacs.config.json' },
        hai_url: { type: 'string', description: 'HAI API URL override' },
      },
    },
  },
  {
    name: 'hai_agent_status',
    description: "Get the current agent's verification status",
    inputSchema: {
      type: 'object' as const,
      properties: {
        config_path: { type: 'string', description: 'Path to jacs.config.json' },
        hai_url: { type: 'string', description: 'HAI API URL override' },
      },
    },
  },
  {
    name: 'hai_verify_agent',
    description: "Verify another agent's JACS document",
    inputSchema: {
      type: 'object' as const,
      properties: {
        agent_document: { type: 'string', description: 'JACS document (JSON string)' },
        config_path: { type: 'string', description: 'Path to jacs.config.json' },
        hai_url: { type: 'string', description: 'HAI API URL override' },
      },
      required: ['agent_document'],
    },
  },
  {
    name: 'hai_generate_verify_link',
    description: 'Generate a HAI verify link from a signed JACS document',
    inputSchema: {
      type: 'object' as const,
      properties: {
        document: { type: 'string', description: 'Signed JACS document JSON string' },
        base_url: { type: 'string', description: 'Verifier base URL override' },
        hosted: { type: 'boolean', description: 'Use hosted verify URL mode' },
      },
      required: ['document'],
    },
  },
  // ----- Email tools -----
  {
    name: 'hai_send_email',
    description: "Send an email from the agent's @hai.ai address",
    inputSchema: {
      type: 'object' as const,
      properties: {
        to: { type: 'string', description: 'Recipient email address' },
        subject: { type: 'string', description: 'Email subject line' },
        body: { type: 'string', description: 'Plain text email body' },
        in_reply_to: { type: 'string', description: 'Message-ID to reply to (for threading)' },
        config_path: { type: 'string', description: 'Path to jacs.config.json' },
        hai_url: { type: 'string', description: 'HAI API URL override' },
      },
      required: ['to', 'subject', 'body'],
    },
  },
  {
    name: 'hai_list_messages',
    description: "List email messages in the agent's inbox/outbox",
    inputSchema: {
      type: 'object' as const,
      properties: {
        limit: { type: 'integer', description: 'Max messages to return (default 20)' },
        offset: { type: 'integer', description: 'Pagination offset' },
        direction: { type: 'string', description: "Filter: 'inbound' or 'outbound'" },
        config_path: { type: 'string', description: 'Path to jacs.config.json' },
        hai_url: { type: 'string', description: 'HAI API URL override' },
      },
    },
  },
  {
    name: 'hai_get_message',
    description: 'Get a single email message by ID',
    inputSchema: {
      type: 'object' as const,
      properties: {
        message_id: { type: 'string', description: 'Message UUID' },
        config_path: { type: 'string', description: 'Path to jacs.config.json' },
        hai_url: { type: 'string', description: 'HAI API URL override' },
      },
      required: ['message_id'],
    },
  },
  {
    name: 'hai_delete_message',
    description: 'Delete an email message',
    inputSchema: {
      type: 'object' as const,
      properties: {
        message_id: { type: 'string', description: 'Message UUID' },
        config_path: { type: 'string', description: 'Path to jacs.config.json' },
        hai_url: { type: 'string', description: 'HAI API URL override' },
      },
      required: ['message_id'],
    },
  },
  {
    name: 'hai_mark_read',
    description: 'Mark an email message as read',
    inputSchema: {
      type: 'object' as const,
      properties: {
        message_id: { type: 'string', description: 'Message UUID' },
        config_path: { type: 'string', description: 'Path to jacs.config.json' },
        hai_url: { type: 'string', description: 'HAI API URL override' },
      },
      required: ['message_id'],
    },
  },
  {
    name: 'hai_mark_unread',
    description: 'Mark an email message as unread',
    inputSchema: {
      type: 'object' as const,
      properties: {
        message_id: { type: 'string', description: 'Message UUID' },
        config_path: { type: 'string', description: 'Path to jacs.config.json' },
        hai_url: { type: 'string', description: 'HAI API URL override' },
      },
      required: ['message_id'],
    },
  },
  {
    name: 'hai_search_messages',
    description: 'Search email messages by query, sender, recipient, or date range',
    inputSchema: {
      type: 'object' as const,
      properties: {
        q: { type: 'string', description: 'Search query text' },
        direction: { type: 'string', description: "Filter: 'inbound' or 'outbound'" },
        from_address: { type: 'string', description: 'Filter by sender address' },
        to_address: { type: 'string', description: 'Filter by recipient address' },
        since: { type: 'string', description: 'Filter: messages after this ISO date' },
        until: { type: 'string', description: 'Filter: messages before this ISO date' },
        limit: { type: 'integer', description: 'Max results (default 20)' },
        offset: { type: 'integer', description: 'Pagination offset' },
        config_path: { type: 'string', description: 'Path to jacs.config.json' },
        hai_url: { type: 'string', description: 'HAI API URL override' },
      },
    },
  },
  {
    name: 'hai_get_unread_count',
    description: 'Get the count of unread email messages',
    inputSchema: {
      type: 'object' as const,
      properties: {
        config_path: { type: 'string', description: 'Path to jacs.config.json' },
        hai_url: { type: 'string', description: 'HAI API URL override' },
      },
    },
  },
  {
    name: 'hai_get_email_status',
    description: 'Get email account status including usage limits and daily stats',
    inputSchema: {
      type: 'object' as const,
      properties: {
        config_path: { type: 'string', description: 'Path to jacs.config.json' },
        hai_url: { type: 'string', description: 'HAI API URL override' },
      },
    },
  },
  {
    name: 'hai_reply_email',
    description: 'Reply to an email message (fetches original, sends reply with threading)',
    inputSchema: {
      type: 'object' as const,
      properties: {
        message_id: { type: 'string', description: 'ID of the message to reply to' },
        body: { type: 'string', description: 'Reply body text' },
        subject_override: { type: 'string', description: 'Override the Re: subject line' },
        config_path: { type: 'string', description: 'Path to jacs.config.json' },
        hai_url: { type: 'string', description: 'HAI API URL override' },
      },
      required: ['message_id', 'body'],
    },
  },
];

function textResult(text: string) {
  return { content: [{ type: 'text' as const, text }] };
}

function errorResult(message: string) {
  return { content: [{ type: 'text' as const, text: message }], isError: true as const };
}

export async function handleToolCall(
  name: string,
  args: Record<string, unknown>,
): Promise<{ content: { type: 'text'; text: string }[]; isError?: true }> {
  try {
    const client = await getClient(args);

    switch (name) {
      // Identity
      case 'hai_hello': {
        const result = await client.hello();
        return textResult(toJSON(result));
      }
      case 'hai_check_username': {
        const result = await client.checkUsername(args.username as string);
        return textResult(toJSON(result));
      }
      case 'hai_claim_username': {
        const result = await client.claimUsername(args.agent_id as string, args.username as string);
        return textResult(toJSON(result));
      }
      case 'hai_register_agent': {
        const opts: Record<string, unknown> = {};
        if (args.owner_email) opts.ownerEmail = args.owner_email;
        const result = await client.register(opts as any);
        return textResult(toJSON(result));
      }
      case 'hai_agent_status': {
        const result = await client.verify();
        return textResult(toJSON(result));
      }
      case 'hai_verify_agent': {
        const result = await client.verifyAgent(args.agent_document as string);
        return textResult(toJSON(result));
      }
      case 'hai_generate_verify_link': {
        const result = generateVerifyLink(
          args.document as string,
          typeof args.base_url === 'string' ? args.base_url : undefined,
          Boolean(args.hosted),
        );
        return textResult(toJSON({ verify_url: result }));
      }

      // Email
      case 'hai_send_email': {
        const result = await client.sendEmail({
          to: args.to as string,
          subject: args.subject as string,
          body: args.body as string,
          inReplyTo: (args.in_reply_to as string) || undefined,
        });
        return textResult(toJSON(result));
      }
      case 'hai_list_messages': {
        const result = await client.listMessages({
          limit: args.limit as number | undefined,
          offset: args.offset as number | undefined,
          direction: args.direction as 'inbound' | 'outbound' | undefined,
        });
        return textResult(toJSON(result));
      }
      case 'hai_get_message': {
        const result = await client.getMessage(args.message_id as string);
        return textResult(toJSON(result));
      }
      case 'hai_delete_message': {
        await client.deleteMessage(args.message_id as string);
        return textResult(JSON.stringify({ deleted: true, message_id: args.message_id }));
      }
      case 'hai_mark_read': {
        await client.markRead(args.message_id as string);
        return textResult(JSON.stringify({ message_id: args.message_id, is_read: true }));
      }
      case 'hai_mark_unread': {
        await client.markUnread(args.message_id as string);
        return textResult(JSON.stringify({ message_id: args.message_id, is_read: false }));
      }
      case 'hai_search_messages': {
        const result = await client.searchMessages({
          query: (args.q as string) || '',
          limit: args.limit as number | undefined,
          offset: args.offset as number | undefined,
          direction: args.direction as 'inbound' | 'outbound' | undefined,
          fromAddress: args.from_address as string | undefined,
          toAddress: args.to_address as string | undefined,
        });
        return textResult(toJSON(result));
      }
      case 'hai_get_unread_count': {
        const count = await client.getUnreadCount();
        return textResult(JSON.stringify({ count }));
      }
      case 'hai_get_email_status': {
        const result = await client.getEmailStatus();
        return textResult(toJSON(result));
      }
      case 'hai_reply_email': {
        const result = await client.reply(
          args.message_id as string,
          args.body as string,
          (args.subject_override as string) || undefined,
        );
        return textResult(toJSON(result));
      }

      default:
        return errorResult(`unknown tool: ${name}`);
    }
  } catch (err) {
    return errorResult(err instanceof Error ? err.message : String(err));
  }
}

export async function main() {
  const server = new Server(
    { name: 'hai-sdk', version: '0.1.0' },
    { capabilities: { tools: {} } },
  );

  server.setRequestHandler(ListToolsRequestSchema, async () => ({
    tools: TOOLS,
  }));

  server.setRequestHandler(CallToolRequestSchema, async (request) => {
    const { name, arguments: args } = request.params;
    return handleToolCall(name, (args ?? {}) as Record<string, unknown>);
  });

  const transport = new StdioServerTransport();
  await server.connect(transport);
}

const currentFile = fileURLToPath(import.meta.url);
const invokedFile = process.argv[1];

if (invokedFile && currentFile === invokedFile) {
  main().catch((err) => {
    process.stderr.write(`server error: ${err}\n`);
    process.exit(1);
  });
}
