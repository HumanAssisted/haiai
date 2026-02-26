import { inspect } from 'node:util';

import { JacsModuleError } from './integrations.js';

type AnyFunction = (...args: any[]) => any;
type AnyModule = Record<string, unknown>;

export type A2ATrustPolicy = 'open' | 'verified' | 'strict';

export interface GetA2AIntegrationOptions {
  trustPolicy?: A2ATrustPolicy;
}

export interface QuickstartA2AOptions {
  name: string;
  domain: string;
  description: string;
  algorithm?: string;
  configPath?: string;
  url?: string;
}

export interface RegisterWithAgentCardOptions extends GetA2AIntegrationOptions {
  ownerEmail?: string;
  domain?: string;
  description?: string;
  agentJson?: string | Record<string, unknown>;
  publicKeyPem?: string;
}

export interface RegisterWithAgentCardResult {
  registration: unknown;
  agentCard: Record<string, unknown>;
  agentJson: string;
}

export interface A2AMediatedJobOptions extends GetA2AIntegrationOptions {
  transport?: 'sse' | 'ws';
  verifyInboundArtifact?: boolean;
  enforceTrustPolicy?: boolean;
  maxReconnectAttempts?: number;
  notifyEmail?: string;
  emailSubject?: string;
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

function asRecord(value: unknown, label: string): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new JacsModuleError(`${label} must be an object`);
  }
  return value as Record<string, unknown>;
}

function stringifyAgentJson(agentJson: string | Record<string, unknown>): string {
  if (typeof agentJson === 'string') {
    return agentJson;
  }
  return JSON.stringify(agentJson);
}

function resolveCardProfile(agentCard: Record<string, unknown>): string {
  const metadata = agentCard.metadata;
  if (metadata && typeof metadata === 'object') {
    const profile = (metadata as Record<string, unknown>).a2aProfile;
    if (typeof profile === 'string' && profile.trim() !== '') {
      return profile;
    }
  }

  const protocolVersions = agentCard.protocolVersions;
  if (Array.isArray(protocolVersions) && typeof protocolVersions[0] === 'string' && protocolVersions[0].trim() !== '') {
    return protocolVersions[0];
  }

  const supportedInterfaces = agentCard.supportedInterfaces;
  if (Array.isArray(supportedInterfaces)) {
    for (const entry of supportedInterfaces) {
      if (!entry || typeof entry !== 'object') continue;
      const version = (entry as Record<string, unknown>).protocolVersion;
      if (typeof version === 'string' && version.trim() !== '') {
        return version;
      }
    }
  }

  return '0.4.0';
}

function valueAsString(record: Record<string, unknown>, key: string): string {
  const value = record[key];
  return typeof value === 'string' ? value : '';
}

function mergeOptions<T extends Record<string, unknown>>(value: T): T {
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(value)) {
    if (v !== undefined) {
      out[k] = v;
    }
  }
  return out as T;
}

function assertRequiredIdentityField(value: unknown, fieldName: string): asserts value is string {
  if (typeof value !== 'string' || value.trim().length === 0) {
    throw new JacsModuleError(`quickstartA2A requires non-empty '${fieldName}'`);
  }
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
export async function quickstartA2A(options: QuickstartA2AOptions): Promise<unknown> {
  assertRequiredIdentityField(options.name, 'name');
  assertRequiredIdentityField(options.domain, 'domain');
  assertRequiredIdentityField(options.description, 'description');

  const mod = await loadOptionalModule(A2A_MODULE, 'A2A quickstart', A2A_HINT);
  const IntegrationClass = getRequiredClass(A2A_MODULE, mod, 'JACSA2AIntegration');
  const quickstart = getRequiredFunction(A2A_MODULE, IntegrationClass as unknown as AnyModule, 'quickstart');
  return quickstart(mergeOptions({
    ...options,
    algorithm: options.algorithm ?? 'pq2025',
  }));
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

export function mergeAgentJsonWithAgentCard(
  agentJson: string | Record<string, unknown>,
  agentCard: Record<string, unknown>,
): string {
  const base = typeof agentJson === 'string'
    ? JSON.parse(agentJson) as Record<string, unknown>
    : { ...agentJson };
  const card = asRecord(agentCard, 'agentCard');

  base.a2aAgentCard = card;
  if (base.skills === undefined && Array.isArray(card.skills)) {
    base.skills = card.skills;
  }
  if (base.capabilities === undefined && card.capabilities && typeof card.capabilities === 'object') {
    base.capabilities = card.capabilities;
  }

  const metadata = (base.metadata && typeof base.metadata === 'object')
    ? { ...(base.metadata as Record<string, unknown>) }
    : {};
  metadata.a2aProfile = resolveCardProfile(card);
  metadata.a2aSkillsCount = Array.isArray(card.skills) ? card.skills.length : 0;
  base.metadata = metadata;

  return JSON.stringify(base);
}

export async function registerWithAgentCard(
  haiClient: unknown,
  jacsClient: unknown,
  agentData: Record<string, unknown>,
  options: RegisterWithAgentCardOptions = {},
): Promise<RegisterWithAgentCardResult> {
  const clientRecord = asRecord(haiClient, 'haiClient');
  const register = getRequiredFunction('HaiClient', clientRecord, 'register');
  const exportKeys = clientRecord.exportKeys;

  const cardRaw = await exportAgentCard(jacsClient, agentData, options);
  const agentCard = asRecord(cardRaw, 'agentCard');

  let baseAgentJson: string | Record<string, unknown>;
  if (options.agentJson !== undefined) {
    baseAgentJson = options.agentJson;
  } else {
    baseAgentJson = {
      jacsId: valueAsString(agentData, 'jacsId') || valueAsString(clientRecord, 'jacsId'),
      name: valueAsString(agentData, 'jacsName') || valueAsString(clientRecord, 'agentName'),
      jacsVersion: valueAsString(agentData, 'jacsVersion') || '1.0.0',
    };
  }
  const mergedAgentJson = mergeAgentJsonWithAgentCard(baseAgentJson, agentCard);

  let publicKeyPem = options.publicKeyPem;
  if (!publicKeyPem && typeof exportKeys === 'function') {
    const exported = exportKeys.apply(clientRecord);
    if (exported && typeof exported === 'object') {
      const candidate = (exported as Record<string, unknown>).publicKeyPem;
      if (typeof candidate === 'string' && candidate.length > 0) {
        publicKeyPem = candidate;
      }
    }
  }

  const registration = await register.apply(clientRecord, [mergeOptions({
    ownerEmail: options.ownerEmail,
    domain: options.domain,
    description: options.description,
    agentJson: mergedAgentJson,
    publicKeyPem,
  })]);

  return {
    registration,
    agentCard,
    agentJson: mergedAgentJson,
  };
}

export async function onMediatedBenchmarkJob(
  haiClient: unknown,
  jacsClient: unknown,
  handler: (taskArtifact: Record<string, unknown>) => Promise<Record<string, unknown>>,
  options: A2AMediatedJobOptions = {},
): Promise<void> {
  const clientRecord = asRecord(haiClient, 'haiClient');
  const onBenchmarkJob = getRequiredFunction('HaiClient', clientRecord, 'onBenchmarkJob');
  const submitResponse = getRequiredFunction('HaiClient', clientRecord, 'submitResponse');
  const sendEmail = clientRecord.sendEmail;

  const verifyInboundArtifact = options.verifyInboundArtifact ?? false;
  const enforceTrustPolicy = options.enforceTrustPolicy ?? false;
  const maxReconnectAttempts = options.maxReconnectAttempts ?? 0;
  const transport = options.transport ?? 'sse';

  let attempts = 0;
  while (true) {
    try {
      await onBenchmarkJob.apply(clientRecord, [async (job: Record<string, unknown>) => {
        const data = asRecord(job.data ?? {}, 'benchmark job data');
        const jobId = valueAsString(data, 'job_id') || valueAsString(data, 'run_id') || valueAsString(job, 'runId');
        if (!jobId) {
          throw new JacsModuleError('benchmark job missing job_id/run_id');
        }

        if (enforceTrustPolicy) {
          const remoteCard = data.remoteAgentCard ?? data.remote_agent_card ?? data.agentCard;
          if (!remoteCard) {
            throw new JacsModuleError('remote agent card required when trust enforcement is enabled');
          }
          const trust = await assessRemoteAgent(jacsClient, remoteCard as string | Record<string, unknown>, options) as Record<string, unknown>;
          if (trust.allowed !== true) {
            const reason = typeof trust.reason === 'string' ? trust.reason : 'unknown reason';
            throw new JacsModuleError(`trust policy rejected remote agent: ${reason}`);
          }
        }

        if (verifyInboundArtifact) {
          const inbound = data.a2aTask ?? data.a2a_task;
          if (!inbound) {
            throw new JacsModuleError('inbound a2a task required when signature verification is enabled');
          }
          const verification = await verifyArtifact(jacsClient, inbound as string | Record<string, unknown>, options) as Record<string, unknown>;
          if (verification.valid !== true) {
            const err = typeof verification.error === 'string' ? verification.error : 'unknown verification failure';
            throw new JacsModuleError(`inbound a2a task signature invalid: ${err}`);
          }
        }

        const taskPayload: Record<string, unknown> = {
          type: 'benchmark_job',
          jobId,
          scenarioId: data.scenario_id ?? data.scenarioId ?? null,
          config: data.config ?? null,
        };
        const taskArtifact = await signArtifact(jacsClient, taskPayload, 'task', null, options) as Record<string, unknown>;
        const resultPayload = await handler(taskArtifact);
        const resultArtifact = await signArtifact(
          jacsClient,
          resultPayload,
          'task-result',
          [taskArtifact],
          options,
        ) as Record<string, unknown>;

        const messageValue = resultPayload.message;
        const message = typeof messageValue === 'string' ? messageValue : JSON.stringify(resultPayload);

        await submitResponse.apply(clientRecord, [jobId, message, {
          metadata: {
            a2aTask: taskArtifact,
            a2aResult: resultArtifact,
          },
          processingTimeMs: 0,
        }]);

        if (options.notifyEmail && typeof sendEmail === 'function') {
          const subject = options.emailSubject ?? `A2A mediated result for job ${jobId}`;
          await (sendEmail as AnyFunction).apply(clientRecord, [{
            to: options.notifyEmail,
            subject,
            body: `Signed A2A artifact:\n\n${JSON.stringify(resultArtifact, null, 2)}`,
          }]);
        }
      }, { transport }]);
      return;
    } catch (error) {
      if (attempts >= maxReconnectAttempts) {
        throw error;
      }
      attempts += 1;
    }
  }
}
