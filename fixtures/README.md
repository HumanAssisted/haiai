# Shared Test Fixtures

Cross-language test fixtures used by Python, Node, and Go test suites.

## Files

- `cross_lang_test.json` - Cross-language HAIAI wrapper contract for auth-header shaping and canonical JSON selection
- `contract_endpoints.json` - Shared HAI endpoint contract used for parity tests
- `mcp_tool_contract.json` - Minimum shared HAIAI MCP tool surface across language SDKs
- `email_conformance.json` - Cross-SDK email conformance tests for EmailVerificationResultV2, content hash golden values, API contracts, FieldStatus enum, and error type mapping
- `crypto_delegation_contract.json` - JACS-only crypto policy enforcement vectors (canonicalization, signing, verification)
- `error_contract.json` - Error codes, message patterns, and action hints across all SDKs
- `path_escaping_contract.json` - URL path segment escaping test vectors for special characters
- `security_regression_contract.json` - Security invariants that must never regress (no private key in register, fallback prevention, etc.)
- `a2a/` - Shared A2A card/artifact/trust fixtures for cross-language parity
  - includes golden fixtures for mixed-profile normalization and chain-of-custody outputs

## Usage

Tests in each language directory reference these fixtures via relative paths
(e.g., `../fixtures/cross_lang_test.json`).

`cross_lang_test.json` is intentionally scoped to HAIAI-owned behavior.
Key material and raw signature vectors belong in JACS fixtures, not here.
