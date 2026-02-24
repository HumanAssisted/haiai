import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

function loadFixture(name: string): Record<string, unknown> {
  const file = join(process.cwd(), '..', 'fixtures', 'a2a', name);
  return JSON.parse(readFileSync(file, 'utf-8')) as Record<string, unknown>;
}

describe('a2a fixtures', () => {
  it('loads v0.4 and v1.0 agent-card fixtures', () => {
    const cardV04 = loadFixture('agent_card.v04.json');
    const cardV10 = loadFixture('agent_card.v10.json');

    expect(cardV04.name).toBe('HAISDK Demo Agent');
    expect(cardV10.name).toBe('HAISDK Demo Agent');
    expect(cardV04.protocolVersions).toEqual(['0.4.0']);

    const supported = cardV10.supportedInterfaces as Array<Record<string, unknown>>;
    expect(Array.isArray(supported)).toBe(true);
    expect(supported[0].protocolVersion).toBe('1.0');
  });

  it('loads wrapped-artifact and trust fixtures', () => {
    const wrapped = loadFixture('wrapped_task.with_parents.json');
    const trustCases = loadFixture('trust_assessment_cases.json');

    expect(wrapped.jacsType).toBe('a2a-task-result');
    const parents = wrapped.jacsParentSignatures as Array<unknown>;
    expect(Array.isArray(parents)).toBe(true);
    expect(parents.length).toBe(1);

    const cases = trustCases.cases as Array<Record<string, unknown>>;
    expect(Array.isArray(cases)).toBe(true);
    expect(cases.length).toBeGreaterThan(0);
  });
});
