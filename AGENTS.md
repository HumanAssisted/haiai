# AGENTS.md

## Purpose

The HAIAI SDK is the HAI platform integration layer around `jacs`.

It exists to help agents use JACS identity/provenance on HAI APIs to:

1. register with HAI
2. interact with other agents through HAI-mediated API workflows
3. receive benchmark jobs and submit responses
4. send and receive agent email

## Ownership Boundary

### `jacs` owns

1. key generation and key encryption/decryption
2. canonicalization for signatures
3. document signing and signature verification
4. low-level cryptographic primitives and trust-store mechanics

### `haiai` owns

1. HAI endpoint contracts and request/response shaping
2. JACS-authenticated HAI API calls (`Authorization: JACS ...`)
3. registration and verification workflows (`/api/v1/agents/*`)
4. mediated runtime interaction with HAI (SSE/WS transport + job orchestration)
5. HAI agent messaging/email APIs
6. HAI integration wrappers that delegate to canonical JACS adapters

## Design Rules

1. Keep `haiai` as a thin wrapper around `jacs` for crypto and provenance logic.
2. Do not duplicate or expand local crypto implementations in `haiai`.
3. Prefer DRY delegation to `jacs` modules for framework integrations.
4. Keep behavior aligned across Node/Python/Go/Rust SDKs.
5. Treat HAI API behavior as the contract surface; add parity tests for all languages.

## Project Layout

```
python/
  src/haiai/             # Python SDK (client, integrations, CLI, MCP server)
  src/jacs/hai/          # JACS integration layer (config, signing, crypto delegation)
  pyproject.toml         # package config — note optional deps: mcp, ws, sse, langchain, etc.
  tests/                 # pytest suite

node/
  src/                   # TypeScript SDK (client, integrations, MCP server)
  tsconfig.json          # ESM build (module: ES2022)
  tsconfig.cjs.json      # CJS build (excludes mcp-server.ts, cli.ts — ESM-only entry points)
  package.json           # dual ESM/CJS exports

go/                      # Go SDK (single module)

rust/
  haiai/                 # library crate (publishable)
  hai-mcp/               # MCP server binary crate
  haiai-cli/             # CLI binary crate (wraps haiai + hai-mcp)
  Cargo.toml             # workspace root

fixtures/                # shared cross-language test fixtures (contract-driven testing)
schemas/                 # JSON schemas for HAI events (AgentEvent, BenchmarkJob, etc.)
contract/                # API response contract examples
scripts/ci/              # CI enforcement (crypto policy denylist)
```

## Building and Testing

```bash
make test              # all languages
make test-python       # pip install -e ".[dev,mcp]" && pytest
make test-node         # npm ci && npm test
make test-go           # go test -race ./...
make test-rust         # cargo test --workspace
make versions          # show detected versions from all packages
make check-versions    # exit 1 if any version mismatches
```

## Version Synchronization

All packages share a single version. These files must match:
- `rust/haiai/Cargo.toml`, `rust/haiai-cli/Cargo.toml`, `rust/hai-mcp/Cargo.toml`
- `python/pyproject.toml`
- `node/package.json`

Run `make check-versions` to verify. Bump all at once before releasing.

## CI/CD (Tag-Based Releases)

Releases are triggered by git tags, not manual publish commands. Use `make release-*`.

| Tag pattern | Workflow | Publishes to |
|-------------|----------|-------------|
| `rust/v*` | `publish-rust.yml` | crates.io + GitHub Release (CLI binaries) |
| `python/v*` | `publish-python.yml` | PyPI (trusted publisher / OIDC) |
| `node/v*` | `publish-node.yml` | npm |

Test workflow runs on push/PR to `main` across all 4 languages.

## Cross-Language Contract Testing

Tests are driven by shared fixtures in `fixtures/`:

- `contract_endpoints.json` — endpoint method/path/auth parity
- `cross_lang_test.json` — auth header shaping and canonical JSON
- `mcp_tool_contract.json` — minimum MCP tool surface
- `email_conformance.json` — email signing/verification parity
- `a2a/` — A2A card/artifact/trust fixtures

Each language SDK has tests that load these fixtures and assert matching behavior.

## Crypto Policy (Enforced by CI)

Runtime crypto in `haiai` must delegate to `jacs`. Direct primitive usage is denied except in explicitly allowlisted transitional files. CI runs `scripts/ci/check_no_local_crypto.sh` on every push.

See: `docs/adr/0001-crypto-delegation-to-jacs.md`

## Known Gotchas

- **JACS filenames use `:` as separator** (`{agent_id}:{version_id}.json`). Illegal on Windows NTFS. The Rust publish workflow uses sparse checkout for Windows builds.
- **Node dual ESM/CJS build.** `mcp-server.ts` and `cli.ts` are ESM-only (use `import.meta`). They're excluded from the CJS tsconfig and only referenced in `bin` entries.
- **Python optional deps matter for tests.** MCP tests need `pip install -e ".[dev,mcp]"`. Just `.[dev]` will miss the `mcp` package.
- **Auth header format:** `JACS {jacsId}:{timestamp}:{signature_base64}` — signed message is `{jacsId}:{timestamp}`.
- **Path segments must be URL-escaped** in API paths (agent_id, job_id, message_id, jacs_id).

## Practical Mental Model

1. Build/load agent identity with `jacs`.
2. Use `haiai` to register that identity with HAI.
3. Use `haiai` transport (`sse`/`ws`) to receive mediated work from HAI.
4. Sign/verify payloads with JACS-backed helpers.
5. Use `haiai` email and agent APIs for operational interaction on HAI.

## Reference Docs

1. `README.md`
2. `docs/HAIAI_LANGUAGE_SYNC_GUIDE.md` — cross-language invariants and change workflow
3. `docs/adr/0001-crypto-delegation-to-jacs.md` — crypto delegation rationale
4. `docs/JACS_DRY.md` — JACS integration architecture
5. `docs/A2A_INTEGRATION_ROADMAP.md`
6. `fixtures/README.md` — shared fixture docs
