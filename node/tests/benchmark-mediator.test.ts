import { describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { HaiClient } from '../src/client.js';
import type { BenchmarkJob, HaiEvent } from '../src/types.js';
import { createMockFFI } from './ffi-mock.js';

const fixture = JSON.parse(readFileSync(new URL('../../fixtures/benchmark_mediator_contract.json', import.meta.url), 'utf8'));

describe('SDK benchmark mediator contract', () => {
  it('dispatches the response job ID separately from the campaign run ID', async () => {
    const client = Object.create(HaiClient.prototype) as HaiClient;
    vi.spyOn(client, 'connect').mockImplementation(async function* () {
      yield { eventType: 'benchmark_job', data: fixture.event, raw: '' } as HaiEvent;
    });
    const jobs: BenchmarkJob[] = [];
    await client.onBenchmarkJob(async (job) => { jobs.push(job); });
    expect(jobs).toHaveLength(1);
    expect(jobs[0].jobId).toBe(fixture.event.job_id);
    expect(jobs[0].runId).toBe(fixture.event.config.run_id);
    expect(jobs[0].data).toEqual(fixture.event);
  });

  it('registers the identity explicitly as a mediator', async () => {
    const client = Object.create(HaiClient.prototype) as HaiClient;
    const register = vi.fn(async (_params: Record<string, unknown>) => ({ success: true, jacs_id: "synthetic-sdk-agent" }));
    client._setFFIAdapter(createMockFFI({ register }));
    await client.register({ agentJson: fixture.registration.agent_json, isMediator: true });
    expect(register).toHaveBeenCalledWith(fixture.registration);
  });

  it('passes the unchanged response and usage to Rust signing', async () => {
    const client = Object.create(HaiClient.prototype) as HaiClient;
    const submitResponse = vi.fn(async (_params: Record<string, unknown>) => ({ success: true }));
    client._setFFIAdapter(createMockFFI({ submitResponse }));
    await client.submitResponse(fixture.event.job_id, fixture.response.message, {
      metadata: fixture.response.metadata, processingTimeMs: fixture.response.processing_time_ms,
    });
    expect(submitResponse).toHaveBeenCalledWith(fixture.ffi_submit_response);
  });
});
