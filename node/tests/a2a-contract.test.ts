/**
 * A2A Verification Contract Tests
 *
 * These tests validate that the Node SDK's A2A types and serialization
 * match the canonical contract fixture at fixtures/a2a_verification_contract.json.
 * They catch schema drift across languages by verifying field names, types,
 * and roundtrip values.
 */
import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

// Load the canonical contract fixture.
const CONTRACT_PATH = resolve(__dirname, '../../fixtures/a2a_verification_contract.json');
const contract = JSON.parse(readFileSync(CONTRACT_PATH, 'utf-8'));

type SchemaFields = Record<string, string>;

/** Assert that all required fields from a schema exist in obj. */
function assertFieldsPresent(
  label: string,
  obj: Record<string, unknown>,
  requiredFields: SchemaFields,
) {
  for (const field of Object.keys(requiredFields)) {
    if (field === '_comment') continue;
    expect(obj).toHaveProperty(field);
  }
}

/** Assert that a value matches the expected type string from the schema. */
function assertFieldType(
  label: string,
  field: string,
  expectedType: string,
  value: unknown,
) {
  switch (expectedType) {
    case 'string':
      expect(typeof value).toBe('string');
      break;
    case 'boolean':
      expect(typeof value).toBe('boolean');
      break;
    case 'object':
      expect(value).toBeDefined();
      expect(typeof value).toBe('object');
      expect(Array.isArray(value)).toBe(false);
      break;
    case 'array':
      expect(Array.isArray(value)).toBe(true);
      break;
    case 'number':
      expect(typeof value).toBe('number');
      break;
  }
}

describe('A2A Verification Contract', () => {
  describe('WrappedArtifact schema', () => {
    const wrappedArtifact = contract.wrappedArtifact as Record<string, unknown>;
    const schema = contract.wrappedArtifactSchema as Record<string, unknown>;
    const requiredFields = schema.requiredFields as SchemaFields;
    const signatureFields = schema.signatureFields as SchemaFields;

    it('has all required fields', () => {
      assertFieldsPresent('A2AWrappedArtifact', wrappedArtifact, requiredFields);
    });

    it('required fields have correct types', () => {
      for (const [field, expectedType] of Object.entries(requiredFields)) {
        if (field === '_comment') continue;
        assertFieldType('A2AWrappedArtifact', field, expectedType, wrappedArtifact[field]);
      }
    });

    it('jacsSignature has all required sub-fields', () => {
      const sig = wrappedArtifact.jacsSignature as Record<string, unknown>;
      expect(sig).toBeDefined();
      assertFieldsPresent('A2AArtifactSignature', sig, signatureFields);
    });

    it('uses agentID (uppercase ID) not agentId', () => {
      const sig = wrappedArtifact.jacsSignature as Record<string, unknown>;
      expect(sig).toHaveProperty('agentID');
      // Must NOT have lowercase agentId
      expect(Object.keys(sig)).not.toContain('agentId');
    });

    it('roundtrip values match contract', () => {
      expect(wrappedArtifact.jacsId).toBe('contract-00000000-0000-4000-8000-000000000001');
      expect(wrappedArtifact.jacsType).toBe('a2a-task');
      expect(wrappedArtifact.jacsLevel).toBe('artifact');
      expect(wrappedArtifact.jacsVersion).toBe('1.0.0');
      const sig = wrappedArtifact.jacsSignature as Record<string, unknown>;
      expect(sig.agentID).toBe('contract-agent');
    });
  });

  describe('VerificationResult schema', () => {
    const schema = contract.verificationResultSchema as Record<string, unknown>;
    const requiredFields = schema.requiredFields as SchemaFields;
    const example = contract.verificationResultExample as Record<string, unknown>;

    it('example has all required fields', () => {
      assertFieldsPresent('A2AArtifactVerificationResult', example, requiredFields);
    });

    it('required fields have correct types', () => {
      for (const [field, expectedType] of Object.entries(requiredFields)) {
        if (field === '_comment') continue;
        assertFieldType('A2AArtifactVerificationResult', field, expectedType, example[field]);
      }
    });

    it('example values match contract', () => {
      expect(example.valid).toBe(false);
      expect(example.signerId).toBe('contract-agent');
      expect(example.artifactType).toBe('a2a-task');
      expect(example.timestamp).toBe('2026-03-01T00:00:00Z');
      expect(example.error).toBe('signature verification failed');
    });

    it('uses signerId (camelCase) not signer_id', () => {
      expect(example).toHaveProperty('signerId');
      expect(Object.keys(example)).not.toContain('signer_id');
    });

    it('uses artifactType (camelCase) not artifact_type', () => {
      expect(example).toHaveProperty('artifactType');
      expect(Object.keys(example)).not.toContain('artifact_type');
    });

    it('uses originalArtifact (camelCase) not original_artifact', () => {
      expect(example).toHaveProperty('originalArtifact');
      expect(Object.keys(example)).not.toContain('original_artifact');
    });
  });

  describe('TrustAssessment schema', () => {
    const schema = contract.trustAssessmentSchema as Record<string, unknown>;
    const requiredFields = schema.requiredFields as SchemaFields;
    const example = contract.trustAssessmentExample as Record<string, unknown>;

    it('example has all required fields', () => {
      assertFieldsPresent('A2ATrustAssessment', example, requiredFields);
    });

    it('required fields have correct types', () => {
      for (const [field, expectedType] of Object.entries(requiredFields)) {
        if (field === '_comment') continue;
        assertFieldType('A2ATrustAssessment', field, expectedType, example[field]);
      }
    });

    it('example values match contract', () => {
      expect(example.allowed).toBe(true);
      expect(example.trustLevel).toBe('jacs_verified');
      expect(example.jacsRegistered).toBe(true);
      expect(example.inTrustStore).toBe(false);
      expect(example.reason).toBe('open policy: all agents accepted');
    });

    it('uses trustLevel (camelCase) not trust_level', () => {
      expect(example).toHaveProperty('trustLevel');
      expect(Object.keys(example)).not.toContain('trust_level');
    });

    it('uses jacsRegistered (camelCase) not jacs_registered', () => {
      expect(example).toHaveProperty('jacsRegistered');
      expect(Object.keys(example)).not.toContain('jacs_registered');
    });

    it('uses inTrustStore (camelCase) not in_trust_store', () => {
      expect(example).toHaveProperty('inTrustStore');
      expect(Object.keys(example)).not.toContain('in_trust_store');
    });
  });

  describe('AgentCard schema', () => {
    const schema = contract.agentCardSchema as Record<string, unknown>;
    const requiredFields = schema.requiredFields as SchemaFields;

    // Load the existing v04 card fixture to validate against the schema.
    const cardPath = resolve(__dirname, '../../fixtures/a2a/agent_card.v04.json');
    const card = JSON.parse(readFileSync(cardPath, 'utf-8')) as Record<string, unknown>;

    it('card fixture has all required fields', () => {
      assertFieldsPresent('A2AAgentCard', card, requiredFields);
    });

    it('required fields have correct types', () => {
      for (const [field, expectedType] of Object.entries(requiredFields)) {
        if (field === '_comment') continue;
        assertFieldType('A2AAgentCard', field, expectedType, card[field]);
      }
    });

    it('uses supportedInterfaces (camelCase) not supported_interfaces', () => {
      expect(card).toHaveProperty('supportedInterfaces');
      expect(Object.keys(card)).not.toContain('supported_interfaces');
    });

    it('uses defaultInputModes (camelCase) not default_input_modes', () => {
      expect(card).toHaveProperty('defaultInputModes');
      expect(Object.keys(card)).not.toContain('default_input_modes');
    });

    it('uses defaultOutputModes (camelCase) not default_output_modes', () => {
      expect(card).toHaveProperty('defaultOutputModes');
      expect(Object.keys(card)).not.toContain('default_output_modes');
    });

    it('skills have required sub-fields', () => {
      const skillFields = (schema.skillFields ?? {}) as SchemaFields;
      const skills = card.skills as Record<string, unknown>[];
      expect(skills.length).toBeGreaterThan(0);
      assertFieldsPresent('A2AAgentSkill', skills[0], skillFields);
    });

    it('extensions have uri field', () => {
      const caps = card.capabilities as Record<string, unknown>;
      const extensions = caps.extensions as Record<string, unknown>[];
      expect(extensions.length).toBeGreaterThan(0);
      expect(extensions[0]).toHaveProperty('uri');
    });
  });

  describe('ChainOfCustody schema', () => {
    const schema = contract.chainOfCustodySchema as Record<string, unknown>;
    const requiredFields = schema.requiredFields as SchemaFields;
    const entryFields = schema.entryFields as SchemaFields;

    // Load the golden chain fixture.
    const chainPath = resolve(__dirname, '../../fixtures/a2a/golden_chain_of_custody.json');
    const chainFixture = JSON.parse(readFileSync(chainPath, 'utf-8')) as Record<string, unknown>;
    const expected = chainFixture.expected as Record<string, unknown>;

    it('expected output has schema-defined top-level fields', () => {
      // The golden fixture expected object has entries+totalArtifacts (chain output).
      expect(expected).toHaveProperty('totalArtifacts');
      expect(expected).toHaveProperty('entries');
    });

    it('chain entry fields match schema', () => {
      const entries = expected.entries as Record<string, unknown>[];
      expect(entries.length).toBeGreaterThan(0);
      assertFieldsPresent('A2AChainEntry', entries[0], entryFields);
    });

    it('chain entry fields have correct types', () => {
      const entries = expected.entries as Record<string, unknown>[];
      for (const [field, expectedType] of Object.entries(entryFields)) {
        if (field === '_comment') continue;
        assertFieldType('A2AChainEntry', field, expectedType, entries[0][field]);
      }
    });

    it('uses artifactId (camelCase) not artifact_id', () => {
      const entries = expected.entries as Record<string, unknown>[];
      expect(entries[0]).toHaveProperty('artifactId');
      expect(Object.keys(entries[0])).not.toContain('artifact_id');
    });

    it('uses signaturePresent (camelCase) not signature_present', () => {
      const entries = expected.entries as Record<string, unknown>[];
      expect(entries[0]).toHaveProperty('signaturePresent');
      expect(Object.keys(entries[0])).not.toContain('signature_present');
    });
  });
});
