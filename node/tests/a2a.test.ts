import { afterEach, describe, expect, it, vi } from 'vitest';

describe('a2a facade wrappers', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.resetModules();
    vi.unmock('@hai.ai/jacs/a2a');
  });

  it('returns a clear error when optional A2A module is missing', async () => {
    const mod = await import('../src/a2a.js');

    await expect(mod.getA2AIntegration({})).rejects.toThrow(
      "Optional dependency '@hai.ai/jacs/a2a' is required",
    );
  });

  it('delegates A2A calls to JACSA2AIntegration', async () => {
    const calls: Record<string, unknown[]> = {};
    vi.doMock('@hai.ai/jacs/a2a', () => {
      class FakeA2AIntegration {
        private readonly client: unknown;
        private readonly trustPolicy: unknown;

        constructor(client: unknown, trustPolicy?: unknown) {
          this.client = client;
          this.trustPolicy = trustPolicy;
          calls.ctor = [client, trustPolicy as unknown];
        }

        static quickstart(options: Record<string, unknown>): Record<string, unknown> {
          calls.quickstart = [options];
          return { quickstart: true, options };
        }

        exportAgentCard(agentData: Record<string, unknown>): Record<string, unknown> {
          calls.exportAgentCard = [this.client, this.trustPolicy, agentData];
          return { op: 'exportAgentCard', agentData };
        }

        signArtifact(
          artifact: Record<string, unknown>,
          artifactType: string,
          parentSignatures: Record<string, unknown>[] | null,
        ): Record<string, unknown> {
          calls.signArtifact = [this.client, this.trustPolicy, artifact, artifactType, parentSignatures];
          return { op: 'signArtifact', artifactType };
        }

        verifyWrappedArtifact(
          wrappedArtifact: string | Record<string, unknown>,
        ): Record<string, unknown> {
          calls.verifyWrappedArtifact = [this.client, this.trustPolicy, wrappedArtifact];
          return { op: 'verifyWrappedArtifact' };
        }

        createChainOfCustody(artifacts: Record<string, unknown>[]): Record<string, unknown> {
          calls.createChainOfCustody = [this.client, this.trustPolicy, artifacts];
          return { op: 'createChainOfCustody', count: artifacts.length };
        }

        generateWellKnownDocuments(
          agentCard: unknown,
          jwsSignature: string,
          publicKeyB64: string,
          agentData: Record<string, unknown>,
        ): Record<string, unknown> {
          calls.generateWellKnownDocuments = [
            this.client,
            this.trustPolicy,
            agentCard,
            jwsSignature,
            publicKeyB64,
            agentData,
          ];
          return { op: 'generateWellKnownDocuments' };
        }

        assessRemoteAgent(agentCardJson: string | Record<string, unknown>): Record<string, unknown> {
          calls.assessRemoteAgent = [this.client, this.trustPolicy, agentCardJson];
          return { op: 'assessRemoteAgent', allowed: true };
        }

        trustA2AAgent(agentCardJson: string | Record<string, unknown>): string {
          calls.trustA2AAgent = [this.client, this.trustPolicy, agentCardJson];
          return 'trusted-agent';
        }
      }

      return {
        JACSA2AIntegration: FakeA2AIntegration,
      };
    });

    const mod = await import('../src/a2a.js');
    const fakeClient = { kind: 'jacs-client' };
    const options = { trustPolicy: 'strict' as const };

    const integration = await mod.getA2AIntegration(fakeClient, options);
    expect(calls.ctor).toEqual([fakeClient, 'strict']);

    await expect(mod.quickstartA2A({
      name: 'hai-agent',
      domain: 'agent.example.com',
      description: 'HAISDK agent',
      algorithm: 'pq2025',
    })).resolves.toEqual({
      quickstart: true,
      options: {
        name: 'hai-agent',
        domain: 'agent.example.com',
        description: 'HAISDK agent',
        algorithm: 'pq2025',
      },
    });

    await expect(mod.exportAgentCard(fakeClient, { jacsId: 'agent-1' }, options)).resolves.toEqual({
      op: 'exportAgentCard',
      agentData: { jacsId: 'agent-1' },
    });
    expect(calls.exportAgentCard).toEqual([fakeClient, 'strict', { jacsId: 'agent-1' }]);

    await expect(
      mod.signArtifact(fakeClient, { taskId: 't-1' }, 'task', [{ parent: true }], options),
    ).resolves.toEqual({ op: 'signArtifact', artifactType: 'task' });
    expect(calls.signArtifact).toEqual([
      fakeClient,
      'strict',
      { taskId: 't-1' },
      'task',
      [{ parent: true }],
    ]);

    await expect(mod.verifyArtifact(fakeClient, '{"wrapped":true}', options)).resolves.toEqual({
      op: 'verifyWrappedArtifact',
    });
    expect(calls.verifyWrappedArtifact).toEqual([fakeClient, 'strict', '{"wrapped":true}']);

    await expect(
      mod.createChainOfCustody(fakeClient, [{ one: 1 }, { two: 2 }], options),
    ).resolves.toEqual({
      op: 'createChainOfCustody',
      count: 2,
    });
    expect(calls.createChainOfCustody).toEqual([fakeClient, 'strict', [{ one: 1 }, { two: 2 }]]);

    await expect(
      mod.generateWellKnownDocuments(
        fakeClient,
        { name: 'Agent Card' },
        'jws-signature',
        'pubkey-b64',
        { jacsId: 'agent-1' },
        options,
      ),
    ).resolves.toEqual({
      op: 'generateWellKnownDocuments',
    });
    expect(calls.generateWellKnownDocuments).toEqual([
      fakeClient,
      'strict',
      { name: 'Agent Card' },
      'jws-signature',
      'pubkey-b64',
      { jacsId: 'agent-1' },
    ]);

    await expect(mod.assessRemoteAgent(fakeClient, '{"card":true}', options)).resolves.toEqual({
      op: 'assessRemoteAgent',
      allowed: true,
    });
    expect(calls.assessRemoteAgent).toEqual([fakeClient, 'strict', '{"card":true}']);

    await expect(mod.trustA2AAgent(fakeClient, '{"card":true}', options)).resolves.toBe('trusted-agent');
    expect(calls.trustA2AAgent).toEqual([fakeClient, 'strict', '{"card":true}']);
    expect(integration).toBeTruthy();
  });

  it('registerWithAgentCard merges card metadata and calls HaiClient.register', async () => {
    vi.doMock('@hai.ai/jacs/a2a', () => {
      class FakeA2AIntegration {
        exportAgentCard(): Record<string, unknown> {
          return {
            name: 'Demo Agent',
            supportedInterfaces: [{ url: 'https://agent.example.com', protocolBinding: 'jsonrpc', protocolVersion: '1.0' }],
            capabilities: {},
            skills: [{ id: 's1' }],
            metadata: {},
          };
        }
      }
      return { JACSA2AIntegration: FakeA2AIntegration };
    });

    const mod = await import('../src/a2a.js');
    const registerCalls: unknown[] = [];
    const fakeHaiClient = {
      jacsId: 'agent-1',
      agentName: 'Agent One',
      exportKeys: () => ({ publicKeyPem: '-----BEGIN PUBLIC KEY-----\nABC\n-----END PUBLIC KEY-----' }),
      register: async (opts: Record<string, unknown>) => {
        registerCalls.push(opts);
        return { agentId: 'agent-1' };
      },
    };

    const result = await mod.registerWithAgentCard(
      fakeHaiClient,
      {},
      { jacsId: 'agent-1', jacsName: 'Agent One' },
      { ownerEmail: 'owner@hai.ai', trustPolicy: 'verified' },
    );

    expect(registerCalls).toHaveLength(1);
    const sent = registerCalls[0] as Record<string, unknown>;
    expect(sent.ownerEmail).toBe('owner@hai.ai');
    expect(typeof sent.agentJson).toBe('string');
    const merged = JSON.parse(sent.agentJson as string) as Record<string, unknown>;
    expect(merged.a2aAgentCard).toBeTruthy();
    expect((merged.metadata as Record<string, unknown>).a2aProfile).toBe('1.0');
    expect(result.agentCard).toBeTruthy();
  });

  it('onMediatedBenchmarkJob retries and submits signed artifacts', async () => {
    const behavior = {
      trustAllowed: true,
      signatureValid: true,
    };

    vi.doMock('@hai.ai/jacs/a2a', () => {
      class FakeA2AIntegration {
        signArtifact(
          artifact: Record<string, unknown>,
          artifactType: string,
        ): Record<string, unknown> {
          return {
            jacsType: `a2a-${artifactType}`,
            a2aArtifact: artifact,
            jacsSignature: { agentID: 'agent-1', signature: 'sig' },
          };
        }

        verifyWrappedArtifact(): Record<string, unknown> {
          return { valid: behavior.signatureValid, error: behavior.signatureValid ? '' : 'invalid' };
        }

        assessRemoteAgent(): Record<string, unknown> {
          return { allowed: behavior.trustAllowed, reason: behavior.trustAllowed ? 'ok' : 'blocked' };
        }
      }
      return { JACSA2AIntegration: FakeA2AIntegration };
    });

    const mod = await import('../src/a2a.js');
    const submitCalls: unknown[] = [];
    const emailCalls: unknown[] = [];
    let onBenchmarkAttempts = 0;
    const fakeHaiClient = {
      onBenchmarkJob: async (handler: (job: Record<string, unknown>) => Promise<void>) => {
        onBenchmarkAttempts += 1;
        if (onBenchmarkAttempts === 1) {
          throw new Error('temporary transport failure');
        }
        await handler({
          runId: 'job-1',
          data: {
            job_id: 'job-1',
            remoteAgentCard: { metadata: { jacsId: 'trusted-agent' } },
            a2aTask: { wrapped: true },
          },
        });
      },
      submitResponse: async (...args: unknown[]) => {
        submitCalls.push(args);
        return { success: true };
      },
      sendEmail: async (opts: Record<string, unknown>) => {
        emailCalls.push(opts);
        return { status: 'sent' };
      },
    };

    await mod.onMediatedBenchmarkJob(
      fakeHaiClient,
      {},
      async () => ({ message: 'handled' }),
      {
        trustPolicy: 'strict',
        transport: 'ws',
        maxReconnectAttempts: 1,
        enforceTrustPolicy: true,
        verifyInboundArtifact: true,
        notifyEmail: 'ops@hai.ai',
      },
    );

    expect(onBenchmarkAttempts).toBe(2);
    expect(submitCalls).toHaveLength(1);
    expect(emailCalls).toHaveLength(1);
  });

  it('onMediatedBenchmarkJob rejects trust/signature failures', async () => {
    const state = {
      trustAllowed: false,
      signatureValid: true,
    };

    vi.doMock('@hai.ai/jacs/a2a', () => {
      class FakeA2AIntegration {
        signArtifact(
          artifact: Record<string, unknown>,
          artifactType: string,
        ): Record<string, unknown> {
          return {
            jacsType: `a2a-${artifactType}`,
            a2aArtifact: artifact,
            jacsSignature: { agentID: 'agent-1', signature: 'sig' },
          };
        }

        verifyWrappedArtifact(): Record<string, unknown> {
          return { valid: state.signatureValid, error: state.signatureValid ? '' : 'invalid' };
        }

        assessRemoteAgent(): Record<string, unknown> {
          return { allowed: state.trustAllowed, reason: 'blocked' };
        }
      }
      return { JACSA2AIntegration: FakeA2AIntegration };
    });

    const mod = await import('../src/a2a.js');
    const fakeHaiClient = {
      onBenchmarkJob: async (handler: (job: Record<string, unknown>) => Promise<void>) => {
        await handler({
          runId: 'job-1',
          data: {
            job_id: 'job-1',
            remoteAgentCard: { metadata: { jacsId: 'unknown' } },
            a2aTask: { wrapped: true },
          },
        });
      },
      submitResponse: async () => ({ success: true }),
    };

    await expect(
      mod.onMediatedBenchmarkJob(
        fakeHaiClient,
        {},
        async () => ({ message: 'handled' }),
        {
          transport: 'ws',
          enforceTrustPolicy: true,
          verifyInboundArtifact: true,
        },
      ),
    ).rejects.toThrow('trust policy rejected remote agent');

    state.trustAllowed = true;
    state.signatureValid = false;

    await expect(
      mod.onMediatedBenchmarkJob(
        fakeHaiClient,
        {},
        async () => ({ message: 'handled' }),
        {
          transport: 'ws',
          enforceTrustPolicy: true,
          verifyInboundArtifact: true,
        },
      ),
    ).rejects.toThrow('inbound a2a task signature invalid');
  });
});
