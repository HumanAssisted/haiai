import { inspect } from 'node:util';

type AnyFunction = (...args: any[]) => any;
type AnyModule = Record<string, unknown>;

export class JacsModuleError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'JacsModuleError';
  }
}

function isMissingModuleError(error: unknown, moduleName: string): boolean {
  if (!error || typeof error !== 'object') {
    return false;
  }

  const code = (error as { code?: string }).code;
  if (code === 'ERR_MODULE_NOT_FOUND' || code === 'MODULE_NOT_FOUND') {
    return true;
  }

  const message = (error as { message?: string }).message ?? '';
  if (message.includes(`Cannot find package '${moduleName}'`)) {
    return true;
  }
  if (message.includes(`Cannot find module '${moduleName}'`)) {
    return true;
  }
  if (message.includes(`Failed to load url ${moduleName}`)) {
    return true;
  }
  return false;
}

async function loadOptionalModule(
  moduleName: string,
  feature: string,
  installHint: string,
): Promise<AnyModule> {
  try {
    return await import(moduleName) as AnyModule;
  } catch (error) {
    if (isMissingModuleError(error, moduleName)) {
      throw new JacsModuleError(
        `Optional dependency '${moduleName}' is required for ${feature}. ${installHint}`,
      );
    }
    throw error;
  }
}

function getRequiredFunction(moduleName: string, moduleObj: AnyModule, name: string): AnyFunction {
  const value = moduleObj[name];
  if (typeof value !== 'function') {
    throw new JacsModuleError(
      `Module '${moduleName}' does not export function '${name}'. ` +
      `Received: ${inspect(value, { depth: 1 })}`,
    );
  }
  return value as AnyFunction;
}

const LANGCHAIN_HINT =
  "Install with: npm install @hai.ai/jacs @langchain/core @langchain/langgraph";
const MCP_HINT =
  "Install with: npm install @hai.ai/jacs @modelcontextprotocol/sdk";

// ---------------------------------------------------------------------------
// LangChain / LangGraph wrappers (delegates to @hai.ai/jacs/langchain)
// ---------------------------------------------------------------------------

export async function langchainSignedTool(tool: any, options: unknown): Promise<any> {
  const moduleName = '@hai.ai/jacs/langchain';
  const mod = await loadOptionalModule(moduleName, 'LangChain/LangGraph integration', LANGCHAIN_HINT);
  return getRequiredFunction(moduleName, mod, 'signedTool')(tool, options);
}

export async function langgraphWrapToolCall(options: unknown): Promise<any> {
  const moduleName = '@hai.ai/jacs/langchain';
  const mod = await loadOptionalModule(moduleName, 'LangGraph tool wrapping', LANGCHAIN_HINT);
  return getRequiredFunction(moduleName, mod, 'jacsWrapToolCall')(options);
}

export async function langgraphToolNode(tools: any[], options: unknown): Promise<any> {
  const moduleName = '@hai.ai/jacs/langchain';
  const mod = await loadOptionalModule(moduleName, 'LangGraph ToolNode integration', LANGCHAIN_HINT);
  return getRequiredFunction(moduleName, mod, 'jacsToolNode')(tools, options);
}

export async function createJacsLangchainTools(options: unknown): Promise<any[]> {
  const moduleName = '@hai.ai/jacs/langchain';
  const mod = await loadOptionalModule(moduleName, 'LangChain JACS tools', LANGCHAIN_HINT);
  return getRequiredFunction(moduleName, mod, 'createJacsTools')(options) as any[];
}

// ---------------------------------------------------------------------------
// MCP wrappers (delegates to @hai.ai/jacs/mcp)
// ---------------------------------------------------------------------------

export async function createJacsMcpTransportProxy(
  transport: unknown,
  clientOrAgent: unknown,
  role: 'client' | 'server' = 'server',
): Promise<any> {
  const moduleName = '@hai.ai/jacs/mcp';
  const mod = await loadOptionalModule(moduleName, 'MCP transport signing', MCP_HINT);
  return getRequiredFunction(moduleName, mod, 'createJACSTransportProxy')(transport, clientOrAgent, role);
}

export async function getJacsMcpToolDefinitions(): Promise<any[]> {
  const moduleName = '@hai.ai/jacs/mcp';
  const mod = await loadOptionalModule(moduleName, 'MCP tool definitions', MCP_HINT);
  return getRequiredFunction(moduleName, mod, 'getJacsMcpToolDefinitions')() as any[];
}

export async function registerJacsMcpTools(server: unknown, client: unknown): Promise<void> {
  const moduleName = '@hai.ai/jacs/mcp';
  const mod = await loadOptionalModule(moduleName, 'MCP server tool registration', MCP_HINT);
  getRequiredFunction(moduleName, mod, 'registerJacsTools')(server, client);
}

// ---------------------------------------------------------------------------
// Agent SDK wrappers (framework-neutral; works with OpenAI Agents tool funcs)
// ---------------------------------------------------------------------------

export interface AgentSdkSigningClient {
  signMessage(payload: unknown): unknown | Promise<unknown>;
  verify?(signedPayload: string): unknown | Promise<unknown>;
}

export interface AgentSdkToolWrapperOptions {
  signer: AgentSdkSigningClient;
  strict?: boolean;
  defaultToolName?: string;
}

function normalizeSignedPayload(value: unknown): string {
  if (typeof value === 'string') {
    return value;
  }
  if (value && typeof value === 'object') {
    const v = value as Record<string, unknown>;
    const direct = v.raw ?? v.raw_json ?? v.rawJson;
    if (typeof direct === 'string') {
      return direct;
    }
  }
  return JSON.stringify(value);
}

function passthroughPayload(value: unknown): string {
  return typeof value === 'string' ? value : JSON.stringify(value);
}

export function createAgentSdkToolWrapper(
  options: AgentSdkToolWrapperOptions,
): <T extends AnyFunction>(tool: T, toolName?: string) => (...args: Parameters<T>) => Promise<string> {
  const strict = options.strict ?? false;

  return function wrapTool<T extends AnyFunction>(
    tool: T,
    toolName?: string,
  ): (...args: Parameters<T>) => Promise<string> {
    const resolvedToolName = toolName ?? options.defaultToolName ?? tool.name ?? 'tool';

    return async (...args: Parameters<T>): Promise<string> => {
      const result = await tool(...args);
      const payload = {
        tool: resolvedToolName,
        result,
      };

      try {
        const signed = await options.signer.signMessage(payload);
        return normalizeSignedPayload(signed);
      } catch (error) {
        if (strict) {
          throw error;
        }
        return passthroughPayload(result);
      }
    };
  };
}

export async function verifyAgentSdkPayload(
  signer: AgentSdkSigningClient,
  signedPayload: string,
  options: { strict?: boolean } = {},
): Promise<unknown> {
  const strict = options.strict ?? false;
  if (typeof signer.verify !== 'function') {
    if (strict) {
      throw new JacsModuleError(
        'Agent SDK verification requires signer.verify(signedPayload).',
      );
    }
    return signedPayload;
  }

  try {
    return await signer.verify(signedPayload);
  } catch (error) {
    if (strict) {
      throw error;
    }
    return signedPayload;
  }
}
