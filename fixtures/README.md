# Shared Test Fixtures

Cross-language test fixtures used by Python, Node, and Go test suites.

## Files

- `cross_lang_test.json` - Cross-language verification fixture for JACS signing compatibility
- `contract_endpoints.json` - Shared HAI endpoint contract used for parity tests
- `a2a/` - Shared A2A card/artifact/trust fixtures for cross-language parity
  - includes golden fixtures for mixed-profile normalization and chain-of-custody outputs

## Usage

Tests in each language directory reference these fixtures via relative paths
(e.g., `../fixtures/cross_lang_test.json`).
