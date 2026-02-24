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

    await expect(mod.quickstartA2A({ algorithm: 'pq2025' })).resolves.toEqual({
      quickstart: true,
      options: { algorithm: 'pq2025' },
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
  });
});
