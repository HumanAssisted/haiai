import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { mergeAgentJsonWithAgentCard } from '../src/a2a.js';

const __dirname = dirname(fileURLToPath(import.meta.url));

function loadFixture(name: string): Record<string, unknown> {
  const file = join(__dirname, '..', '..', 'fixtures', 'a2a', name);
  return JSON.parse(readFileSync(file, 'utf-8')) as Record<string, unknown>;
}

describe('a2a fixtures', () => {
  it('loads v0.4 and v1.0 agent-card fixtures', () => {
    const cardV04 = loadFixture('agent_card.v04.json');
    const cardV10 = loadFixture('agent_card.v10.json');

    expect(cardV04.name).toBe('HAIAI Demo Agent');
    expect(cardV10.name).toBe('HAIAI Demo Agent');
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

  it('loads golden profile and chain fixtures', () => {
    const profiles = loadFixture('golden_profile_normalization.json');
    const chain = loadFixture('golden_chain_of_custody.json');

    const profileCases = profiles.cases as Array<Record<string, unknown>>;
    expect(Array.isArray(profileCases)).toBe(true);
    expect(profileCases.length).toBeGreaterThan(0);
    expect(profileCases[0].expected).toBeTruthy();

    const expected = chain.expected as Record<string, unknown>;
    const entries = expected.entries as Array<Record<string, unknown>>;
    expect(expected.totalArtifacts).toBe(2);
    expect(entries.length).toBe(2);
    expect(entries[0].artifactType).toBe('a2a-task');
  });

  it('applies golden profile normalization cases', () => {
    const profiles = loadFixture('golden_profile_normalization.json');
    const cases = profiles.cases as Array<Record<string, unknown>>;

    for (const testCase of cases) {
      const mergedJson = mergeAgentJsonWithAgentCard(
        testCase.agentJson as Record<string, unknown>,
        testCase.card as Record<string, unknown>,
      );
      const merged = JSON.parse(mergedJson) as Record<string, unknown>;
      const expected = testCase.expected as Record<string, unknown>;
      const metadata = merged.metadata as Record<string, unknown>;
      expect(metadata.a2aProfile).toBe(expected.a2aProfile);
    }
  });
});
