import { describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { HaiClient } from '../src/client.js';
import { canonicalJson } from '../src/signing.js';
import { TEST_AGENT } from './setup.js';

interface CrossLangFixture {
  auth_header: {
    scheme: string;
    parts: string[];
    signed_message_template: string;
    example: {
      jacs_id: string;
      timestamp: number;
      stub_signature_base64: string;
      expected_header: string;
    };
  };
  canonical_json_cases: Array<{
    name: string;
    input: unknown;
    expected: string;
  }>;
}

function loadFixture(): CrossLangFixture {
  const here = dirname(fileURLToPath(import.meta.url));
  const fixturePath = resolve(here, '../../fixtures/cross_lang_test.json');
  return JSON.parse(readFileSync(fixturePath, 'utf-8')) as CrossLangFixture;
}

describe('cross-language wrapper contract (node)', () => {
  it('matches the shared canonical JSON cases', () => {
    const fixture = loadFixture();
    for (const testCase of fixture.canonical_json_cases) {
      expect(canonicalJson(testCase.input, TEST_AGENT), testCase.name).toBe(testCase.expected);
    }
  });

  it('matches the shared auth header example', () => {
    const fixture = loadFixture();
    const client = Object.create(HaiClient.prototype) as HaiClient & {
      agent: { signStringSync: (message: string) => string };
      config: { jacsId: string; jacsAgentName: string };
    };
    const signStringSync = vi.fn(() => fixture.auth_header.example.stub_signature_base64);

    client.agent = { signStringSync };
    client.config = {
      jacsId: fixture.auth_header.example.jacs_id,
      jacsAgentName: fixture.auth_header.example.jacs_id,
    };

    vi.useFakeTimers();
    vi.setSystemTime(fixture.auth_header.example.timestamp * 1000);

    expect(client.buildAuthHeader()).toBe(fixture.auth_header.example.expected_header);
    expect(signStringSync).toHaveBeenCalledWith(
      fixture.auth_header.signed_message_template
        .replace('{jacs_id}', fixture.auth_header.example.jacs_id)
        .replace('{timestamp}', String(fixture.auth_header.example.timestamp)),
    );

    vi.useRealTimers();
  });
});
