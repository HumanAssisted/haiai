import { describe, it, expect } from 'vitest';
import type {
  HaiClientOptions,
  AgentConfig,
  HaiEvent,
  BenchmarkJob,
  BenchmarkTier,
  ConnectionMode,
  EventType,
  HelloWorldResult,
  RegistrationResult,
  FreeChaoticResult,
  BaselineResult,
  CertifiedResult,
  BenchmarkResult,
  JobResponseResult,
  StatusResult,
  TranscriptMessage,
  ConversationTurn,
  AgentCapability,
  BenchmarkJobConfig,
  ConnectOptions,
  OnBenchmarkJobOptions,
  BaselineRunOptions,
  FreeChaoticRunOptions,
  JobResponse,
} from '../src/types.js';

describe('type definitions', () => {
  it('HaiClientOptions accepts all fields', () => {
    const opts: HaiClientOptions = {
      configPath: './test.json',
      url: 'https://localhost:3000',
      timeout: 5000,
      maxRetries: 2,
    };
    expect(opts.timeout).toBe(5000);
  });

  it('HaiClientOptions works with no fields', () => {
    const opts: HaiClientOptions = {};
    expect(opts.configPath).toBeUndefined();
  });

  it('AgentConfig has required fields', () => {
    const config: AgentConfig = {
      jacsAgentName: 'test-agent',
      jacsAgentVersion: '1.0.0',
      jacsKeyDir: './keys',
    };
    expect(config.jacsAgentName).toBe('test-agent');
    expect(config.jacsId).toBeUndefined();
  });

  it('HaiEvent has correct shape', () => {
    const event: HaiEvent = {
      eventType: 'benchmark_job',
      data: { run_id: 'r1' },
      id: 'evt-1',
      raw: '{"run_id":"r1"}',
    };
    expect(event.eventType).toBe('benchmark_job');
  });

  it('BenchmarkTier accepts all three tiers', () => {
    const tiers: BenchmarkTier[] = ['free_chaotic', 'baseline', 'certified'];
    expect(tiers).toHaveLength(3);
  });

  it('ConnectionMode accepts sse and ws', () => {
    const modes: ConnectionMode[] = ['sse', 'ws'];
    expect(modes).toHaveLength(2);
  });

  it('EventType accepts known and custom strings', () => {
    const types: EventType[] = ['connected', 'benchmark_job', 'heartbeat', 'custom_type'];
    expect(types).toHaveLength(4);
  });

  it('BenchmarkJob has correct shape', () => {
    const job: BenchmarkJob = {
      runId: 'run-1',
      scenario: { prompt: 'test' },
      data: { run_id: 'run-1' },
    };
    expect(job.runId).toBe('run-1');
  });

  it('TranscriptMessage has all fields', () => {
    const msg: TranscriptMessage = {
      role: 'mediator',
      content: 'I suggest a compromise.',
      timestamp: '2024-01-01T00:00:00Z',
      annotations: ['resolution_proposed'],
    };
    expect(msg.role).toBe('mediator');
  });

  it('ConversationTurn is alias for TranscriptMessage', () => {
    const turn: ConversationTurn = {
      role: 'party_a',
      content: 'I disagree.',
      timestamp: '2024-01-01T00:00:00Z',
      annotations: [],
    };
    expect(turn.role).toBe('party_a');
  });

  it('HelloWorldResult has correct shape', () => {
    const result: HelloWorldResult = {
      success: true,
      timestamp: '2024-01-01T00:00:00Z',
      clientIp: '127.0.0.1',
      haiPublicKeyFingerprint: 'abc123',
      message: 'Hello!',
      haiSignatureValid: true,
      rawResponse: {},
    };
    expect(result.success).toBe(true);
  });

  it('RegistrationResult has correct shape', () => {
    const result: RegistrationResult = {
      success: true,
      agentId: 'agent-1',
      haiSignature: 'sig',
      registrationId: 'reg-1',
      registeredAt: '2024-01-01T00:00:00Z',
      rawResponse: {},
    };
    expect(result.agentId).toBe('agent-1');
  });

  it('FreeChaoticResult has correct shape', () => {
    const result: FreeChaoticResult = {
      success: true,
      runId: 'run-1',
      transcript: [],
      upsellMessage: 'Upgrade!',
      rawResponse: {},
    };
    expect(result.upsellMessage).toBe('Upgrade!');
  });

  it('BaselineResult has score', () => {
    const result: BaselineResult = {
      success: true,
      runId: 'run-1',
      score: 85,
      transcript: [],
      paymentId: 'pay-1',
      rawResponse: {},
    };
    expect(result.score).toBe(85);
  });

  it('CertifiedResult has categories', () => {
    const result: CertifiedResult = {
      success: true,
      runId: 'run-1',
      score: 92,
      categories: { empathy: 95, clarity: 89 },
      transcript: [],
      paymentId: 'pay-1',
      rawResponse: {},
    };
    expect(result.categories.empathy).toBe(95);
  });

  it('BenchmarkResult is union type', () => {
    const results: BenchmarkResult[] = [
      { success: true, runId: 'r1', transcript: [], upsellMessage: '', rawResponse: {} },
      { success: true, runId: 'r2', score: 80, transcript: [], paymentId: 'p1', rawResponse: {} },
    ];
    expect(results).toHaveLength(2);
  });

  it('JobResponseResult has correct shape', () => {
    const result: JobResponseResult = {
      success: true,
      jobId: 'job-1',
      message: 'Accepted',
      rawResponse: {},
    };
    expect(result.success).toBe(true);
  });

  it('StatusResult has correct shape', () => {
    const result: StatusResult = {
      active: true,
      agentId: 'agent-1',
      registeredAt: '2024-01-01T00:00:00Z',
      benchmarkCount: 5,
      rawResponse: {},
    };
    expect(result.benchmarkCount).toBe(5);
  });

  it('AgentCapability accepts known and custom strings', () => {
    const caps: AgentCapability[] = ['mediation', 'arbitration', 'custom_skill'];
    expect(caps).toHaveLength(3);
  });

  it('BenchmarkJobConfig has correct shape', () => {
    const config: BenchmarkJobConfig = {
      tier: 'baseline',
      name: 'Test Run',
      transport: 'sse',
      paymentId: 'pay-1',
    };
    expect(config.tier).toBe('baseline');
  });

  it('ConnectOptions has correct shape', () => {
    const opts: ConnectOptions = {
      transport: 'ws',
      onEvent: () => {},
    };
    expect(opts.transport).toBe('ws');
  });

  it('OnBenchmarkJobOptions has correct shape', () => {
    const opts: OnBenchmarkJobOptions = { transport: 'sse' };
    expect(opts.transport).toBe('sse');
  });

  it('BaselineRunOptions has all fields', () => {
    const opts: BaselineRunOptions = {
      transport: 'sse',
      pollIntervalMs: 3000,
      pollTimeoutMs: 600000,
      onCheckoutUrl: () => {},
    };
    expect(opts.pollIntervalMs).toBe(3000);
  });

  it('FreeChaoticRunOptions has transport', () => {
    const opts: FreeChaoticRunOptions = { transport: 'ws' };
    expect(opts.transport).toBe('ws');
  });

  it('JobResponse has correct shape', () => {
    const resp: JobResponse = {
      response: {
        message: 'test',
        metadata: null,
        processing_time_ms: 100,
      },
    };
    expect(resp.response.processing_time_ms).toBe(100);
  });
});
