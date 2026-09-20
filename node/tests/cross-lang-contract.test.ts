import { describe, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { HaiClient } from '../src/client.js';
import { canonicalJson } from '../src/signing.js';
import { TEST_AGENT } from './setup.js';
import { createMockFFI } from './ffi-mock.js';

interface CrossLangFixture {
  request_auth: {
    scheme_prefix: string;
    input_fields: string[];
    example: {
      method: string;
      url: string;
      body_base64: string;
      stub_header: string;
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

  it('rejects the retired no-context helper actionably', () => {
    const client = Object.create(HaiClient.prototype) as HaiClient;
    expect(() => client.buildAuthHeader()).toThrow('buildRequestAuthHeader');
  });

  it('delegates the exact request bytes using the shared encoding contract', async () => {
    const { request_auth: fixture } = loadFixture();
    const { method, url, body_base64, stub_header } = fixture.example;
    const client = Object.create(HaiClient.prototype) as HaiClient;
    const buildRequestAuthHeader = vi.fn(async (_requestJson: string) => stub_header);
    client._setFFIAdapter(createMockFFI({ buildRequestAuthHeader }));
    const body = Buffer.from(body_base64, 'base64');
    expect(await client.buildRequestAuthHeader(method, url, body)).toBe(stub_header);
    const input = JSON.parse(buildRequestAuthHeader.mock.calls[0][0]);
    expect(input).toEqual({ method, url, body_base64 });
    expect(Object.keys(input)).toEqual(fixture.input_fields);
  });

  it('explicitly encodes an empty request body', async () => {
    const client = Object.create(HaiClient.prototype) as HaiClient;
    const buildRequestAuthHeader = vi.fn(async (_requestJson: string) => 'JACS v2.fixture');
    client._setFFIAdapter(createMockFFI({ buildRequestAuthHeader }));
    await client.buildRequestAuthHeader('GET', 'https://hai.ai/');
    expect(JSON.parse(buildRequestAuthHeader.mock.calls[0][0]).body_base64).toBe('');
  });

  it('rejects implicit body encoding before asking the signer', async () => {
    const client = Object.create(HaiClient.prototype) as HaiClient;
    await expect(client.buildRequestAuthHeader('POST', 'https://hai.ai/', 'body' as unknown as Uint8Array))
      .rejects.toThrow('exact transmitted request body');
  });
});
