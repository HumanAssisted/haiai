import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { TOOLS } from '../src/mcp-server.js';

interface RequiredTool {
  name: string;
  properties: Record<string, string>;
  required: string[];
}

interface MCPContractFixture {
  required_tools: RequiredTool[];
}

function loadFixture(): MCPContractFixture {
  const here = dirname(fileURLToPath(import.meta.url));
  const fixturePath = resolve(here, '../../fixtures/mcp_tool_contract.json');
  return JSON.parse(readFileSync(fixturePath, 'utf-8')) as MCPContractFixture;
}

function normalizeType(type: string | undefined): string {
  if (type === 'integer') {
    return 'number';
  }
  return type ?? 'string';
}

describe('mcp tool contract (node)', () => {
  it('matches the shared required MCP tool surface', () => {
    const fixture = loadFixture();
    const actual = new Map(
      TOOLS.map((tool) => [
        tool.name,
        {
          name: tool.name,
          properties: Object.fromEntries(
            Object.entries(tool.inputSchema.properties ?? {}).map(([name, schema]) => [
              name,
              normalizeType((schema as { type?: string }).type),
            ]),
          ),
          required: [...(tool.inputSchema.required ?? [])].sort(),
        },
      ]),
    );

    for (const expected of fixture.required_tools) {
      const tool = actual.get(expected.name);
      expect(tool).toBeDefined();
      expect(tool?.required).toEqual([...expected.required].sort());
      for (const [name, type] of Object.entries(expected.properties)) {
        expect(tool?.properties[name]).toBe(type);
      }
    }
  });
});
