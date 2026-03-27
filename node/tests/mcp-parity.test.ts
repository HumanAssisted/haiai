/**
 * MCP tool contract parity tests -- verify the Node FFI adapter can
 * support every tool category defined in the shared MCP tool contract.
 *
 * The Rust MCP server (hai-mcp) is the canonical MCP implementation.
 * Python and Node do not have their own MCP servers (deleted per
 * CLI_PARITY_AUDIT.md). However, the FFI adapter must expose the
 * underlying methods needed to serve each MCP tool category, so that
 * any future MCP reimplementation is backed by the same FFI layer.
 *
 * This test loads fixtures/mcp_tool_contract.json and validates that:
 * 1. The fixture is structurally valid (has required fields, counts match).
 * 2. Every MCP tool category maps to at least one FFI adapter method.
 * 3. The total_tool_count field matches the actual number of tools.
 */

import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { FFIClientAdapter } from '../src/ffi-client.js';

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

interface MCPTool {
  name: string;
  properties: Record<string, string>;
  required: string[];
}

interface MCPToolContract {
  description: string;
  version: string;
  total_tool_count: number;
  required_tools: MCPTool[];
}

function loadMCPContract(): MCPToolContract {
  const fixturePath = resolve(__dirname, '../../fixtures/mcp_tool_contract.json');
  return JSON.parse(readFileSync(fixturePath, 'utf-8')) as MCPToolContract;
}

/**
 * Mapping from MCP tool names to the FFI adapter methods (camelCase)
 * that would back them. Each MCP tool maps to one or more FFI methods.
 */
const MCP_TOOL_TO_FFI_METHODS: Record<string, string[]> = {
  hai_hello: ['hello'],
  hai_check_username: ['checkUsername'],
  hai_claim_username: ['claimUsername'],
  hai_register_agent: ['register'],
  hai_agent_status: ['verifyStatus'],
  hai_verify_status: ['verifyStatus'],
  hai_generate_verify_link: [], // client-side only, no FFI method needed
  hai_send_email: ['sendEmail'],
  hai_list_messages: ['listMessages'],
  hai_get_message: ['getMessage'],
  hai_delete_message: ['deleteMessage'],
  hai_mark_read: ['markRead'],
  hai_mark_unread: ['markUnread'],
  hai_search_messages: ['searchMessages'],
  hai_get_unread_count: ['getUnreadCount'],
  hai_get_email_status: ['getEmailStatus'],
  hai_reply_email: ['replyWithOptions'],
  hai_forward_email: ['forward'],
  hai_archive_message: ['archive'],
  hai_unarchive_message: ['unarchive'],
  hai_list_contacts: ['contacts'],
  hai_self_knowledge: [], // embedded docs, no FFI method needed
  hai_create_email_template: ['createEmailTemplate'],
  hai_list_email_templates: ['listEmailTemplates'],
  hai_search_email_templates: ['listEmailTemplates'],
  hai_get_email_template: ['getEmailTemplate'],
  hai_update_email_template: ['updateEmailTemplate'],
  hai_delete_email_template: ['deleteEmailTemplate'],
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('MCP tool contract parity (Node)', () => {
  it('fixture has required fields', () => {
    const contract = loadMCPContract();
    expect(contract.required_tools).toBeDefined();
    expect(contract.total_tool_count).toBeDefined();
    expect(contract.version).toBeDefined();
  });

  it('total_tool_count matches actual tool count', () => {
    const contract = loadMCPContract();
    expect(contract.required_tools.length).toBe(contract.total_tool_count);
  });

  it('every tool entry has name, properties, and required fields', () => {
    const contract = loadMCPContract();
    for (const tool of contract.required_tools) {
      expect(tool.name).toBeDefined();
      expect(tool.properties).toBeDefined();
      expect(tool.required).toBeDefined();
    }
  });

  it('FFIClientAdapter has methods to back every MCP tool category', () => {
    const contract = loadMCPContract();
    const adapterProto = FFIClientAdapter.prototype;
    const missing: string[] = [];

    for (const tool of contract.required_tools) {
      const ffiMethods = MCP_TOOL_TO_FFI_METHODS[tool.name];
      if (ffiMethods === undefined) {
        missing.push(`${tool.name}: no mapping in MCP_TOOL_TO_FFI_METHODS`);
        continue;
      }
      for (const method of ffiMethods) {
        if (typeof (adapterProto as Record<string, unknown>)[method] !== 'function') {
          missing.push(`${tool.name} -> FFI method '${method}' not in FFIClientAdapter`);
        }
      }
    }

    expect(missing).toEqual([]);
  });

  it('mapping covers all fixture tools', () => {
    const contract = loadMCPContract();
    const fixtureNames = new Set(contract.required_tools.map((t) => t.name));
    const mappingNames = new Set(Object.keys(MCP_TOOL_TO_FFI_METHODS));

    const unmapped = [...fixtureNames].filter((n) => !mappingNames.has(n));
    expect(unmapped).toEqual([]);
  });
});
