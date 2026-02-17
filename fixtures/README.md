# Shared Test Fixtures

Cross-language test fixtures used by Python, Node, and Go test suites.

## Files

- `cross_lang_test.json` - Cross-language verification fixture for JACS signing compatibility
- Additional fixtures will be added as SDK features are implemented

## Usage

Tests in each language directory reference these fixtures via relative paths
(e.g., `../fixtures/cross_lang_test.json`).
