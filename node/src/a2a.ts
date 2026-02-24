import { inspect } from 'node:util';

import { JacsModuleError } from './integrations.js';

type AnyFunction = (...args: any[]) => any;
type AnyModule = Record<string, unknown>;

export type A2ATrustPolicy = 'open' | 'verified' | 'strict';

export interface GetA2AIntegrationOptions {
  trustPolicy?: A2ATrustPolicy;
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

function getRequiredClass(moduleName: string, moduleObj: AnyModule, name: string): new (...args: any[]) => any {
  const value = moduleObj[name];
  if (typeof value !== 'function') {
    throw new JacsModuleError(
      `Module '${moduleName}' does not export class '${name}'. ` +
      `Received: ${inspect(value, { depth: 1 })}`,
    );
  }
  return value as new (...args: any[]) => any;
}

const A2A_HINT = "Install with: npm install @hai.ai/jacs";
const A2A_MODULE = '@hai.ai/jacs/a2a';

async function callIntegrationMethod(
  integration: Record<string, unknown>,
  methodName: string,
  args: unknown[],
): Promise<unknown> {
  const method = integration[methodName];
  if (typeof method !== 'function') {
    throw new JacsModuleError(
      `A2A integration missing method '${methodName}'. Received: ${inspect(method, { depth: 1 })}`,
    );
  }
  return (method as AnyFunction).apply(integration, args);
}

/**
 * Build a JACS A2A integration instance from a caller-provided JACS client.
 *
 * The JACS client is typically an instance of `JacsClient` from `@hai.ai/jacs/client`.
 */
export async function getA2AIntegration(
  jacsClient: unknown,
  options: GetA2AIntegrationOptions = {},
): Promise<unknown> {
  const mod = await loadOptionalModule(A2A_MODULE, 'A2A integration', A2A_HINT);
  const IntegrationClass = getRequiredClass(A2A_MODULE, mod, 'JACSA2AIntegration');
  return new IntegrationClass(jacsClient, options.trustPolicy);
}

/**
 * Delegate to `JACSA2AIntegration.quickstart(...)` from `@hai.ai/jacs/a2a`.
 */
export async function quickstartA2A(options: Record<string, unknown> = {}): Promise<unknown> {
  const mod = await loadOptionalModule(A2A_MODULE, 'A2A quickstart', A2A_HINT);
  const IntegrationClass = getRequiredClass(A2A_MODULE, mod, 'JACSA2AIntegration');
  const quickstart = getRequiredFunction(A2A_MODULE, IntegrationClass as unknown as AnyModule, 'quickstart');
  return quickstart(options);
}

export async function exportAgentCard(
  jacsClient: unknown,
  agentData: Record<string, unknown>,
  options: GetA2AIntegrationOptions = {},
): Promise<unknown> {
  const integration = await getA2AIntegration(jacsClient, options) as Record<string, unknown>;
  return callIntegrationMethod(integration, 'exportAgentCard', [agentData]);
}

export async function signArtifact(
  jacsClient: unknown,
  artifact: Record<string, unknown>,
  artifactType: string,
  parentSignatures: Record<string, unknown>[] | null = null,
  options: GetA2AIntegrationOptions = {},
): Promise<unknown> {
  const integration = await getA2AIntegration(jacsClient, options) as Record<string, unknown>;
  return callIntegrationMethod(integration, 'signArtifact', [artifact, artifactType, parentSignatures]);
}

export async function verifyArtifact(
  jacsClient: unknown,
  wrappedArtifact: string | Record<string, unknown>,
  options: GetA2AIntegrationOptions = {},
): Promise<unknown> {
  const integration = await getA2AIntegration(jacsClient, options) as Record<string, unknown>;
  return callIntegrationMethod(integration, 'verifyWrappedArtifact', [wrappedArtifact]);
}

export async function createChainOfCustody(
  jacsClient: unknown,
  artifacts: Record<string, unknown>[],
  options: GetA2AIntegrationOptions = {},
): Promise<unknown> {
  const integration = await getA2AIntegration(jacsClient, options) as Record<string, unknown>;
  return callIntegrationMethod(integration, 'createChainOfCustody', [artifacts]);
}

export async function generateWellKnownDocuments(
  jacsClient: unknown,
  agentCard: unknown,
  jwsSignature: string,
  publicKeyB64: string,
  agentData: Record<string, unknown>,
  options: GetA2AIntegrationOptions = {},
): Promise<unknown> {
  const integration = await getA2AIntegration(jacsClient, options) as Record<string, unknown>;
  return callIntegrationMethod(
    integration,
    'generateWellKnownDocuments',
    [agentCard, jwsSignature, publicKeyB64, agentData],
  );
}

export async function assessRemoteAgent(
  jacsClient: unknown,
  agentCardJson: string | Record<string, unknown>,
  options: GetA2AIntegrationOptions = {},
): Promise<unknown> {
  const integration = await getA2AIntegration(jacsClient, options) as Record<string, unknown>;
  return callIntegrationMethod(integration, 'assessRemoteAgent', [agentCardJson]);
}

export async function trustA2AAgent(
  jacsClient: unknown,
  agentCardJson: string | Record<string, unknown>,
  options: GetA2AIntegrationOptions = {},
): Promise<unknown> {
  const integration = await getA2AIntegration(jacsClient, options) as Record<string, unknown>;
  return callIntegrationMethod(integration, 'trustA2AAgent', [agentCardJson]);
}
