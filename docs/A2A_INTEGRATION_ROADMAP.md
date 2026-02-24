# A2A Integration Roadmap (HAISDK)

## Scope

This roadmap defines a practical, DRY, TDD-first plan to make A2A a first-class capability in `haisdk` while preserving the architecture boundary:

1. `jacs` owns cryptography and provenance primitives.
2. `haisdk` owns HAI-mediated transport, registration, orchestration, and email workflows.
3. `haisdk` should expose the full relevant JACS capability set as a unified facade so users can stay at the `haisdk` layer.

## Non-Negotiable Layering Principles

1. **JACS implementation parity first**: A2A logic is implemented in `jacs` (or canonical JACS modules) as the source of truth.
2. **HAISDK facade parity second**: All stable JACS A2A features should be accessible through `haisdk` APIs.
3. **No duplicated protocol engines in HAISDK**: `haisdk` wraps/delegates JACS A2A logic instead of re-implementing it.
4. **HAISDK composes with HAI**: `haisdk` adds HAI-specific composition (registration, mediated transport, orchestration, email) on top of JACS primitives.
5. **Examples use facade APIs**: examples should consume first-class `haisdk` A2A facades, not ad-hoc duplicate structs/logic.

## What Is Already Implemented In JACS (Baseline Snapshot)

Baseline snapshot date: **February 24, 2026**.

### JACS Node (`jacsnpm`)

1. Client-level A2A convenience APIs:
   - `getA2A()`
   - `exportAgentCard(...)`
   - `signArtifact(...)`
   - `verifyArtifact(...)`
   - `generateWellKnownDocuments(...)`
2. A2A integration APIs in `a2a` module:
   - `exportAgentCard(...)`
   - `signArtifact(...)`
   - `verifyWrappedArtifact(...)`
   - `createChainOfCustody(...)`
   - `generateWellKnownDocuments(...)`
   - `assessRemoteAgent(...)`
   - `trustA2AAgent(...)`
3. A2A discovery/server helpers (`a2a-discovery`, `a2a-server`).

### JACS Python (`jacspy`)

1. Client-level A2A convenience APIs:
   - `get_a2a(...)`
   - `export_agent_card(...)`
   - `sign_artifact(...)`
2. `jacs.a2a.JACSA2AIntegration` includes:
   - `export_agent_card(...)`
   - `sign_artifact(...)` / `wrap_artifact_with_provenance(...)`
   - `verify_wrapped_artifact(...)`
   - `create_chain_of_custody(...)`
   - `generate_well_known_documents(...)`
   - `assess_remote_agent(...)`
   - `trust_a2a_agent(...)`
   - `serve(...)` helper for discovery endpoints
3. A2A discovery/server modules (`a2a_discovery`, `a2a_server`).

### JACS Rust Core (`jacs`)

1. `SimpleAgent` A2A support:
   - `export_agent_card(...)`
   - `generate_well_known_documents(...)`
   - `wrap_a2a_artifact(...)`
   - `sign_artifact(...)` (alias)
   - `verify_a2a_artifact(...)`
2. Trust-store support includes A2A card trust path (`trust_a2a_card(...)`) plus standard trust operations.

### Gap Relative To HAISDK Today

1. `haisdk` examples demonstrate A2A flows, but first-class unified A2A facades are not yet complete across all SDK languages.
2. Several example flows in `haisdk` currently duplicate protocol structs/logic that should move behind delegated facade APIs.

## Current State (Baseline)

1. `haisdk` has A2A quickstart examples in Node/Python/Go.
2. A2A functionality is mostly example-level in `haisdk` (not unified SDK surface).
3. JACS already has richer A2A support (agent cards, artifact signing/verification, trust policy, discovery server helpers).
4. HAI registration accepts `agent_json` and extracts A2A-like fields (`skills`, `capabilities`, etc.) but does not yet expose dedicated A2A card endpoints.

## Product Goal

Make A2A a practical default for agent-to-agent trust and task/document provenance in HAI workflows, without duplicating JACS crypto logic.

## Security Model

1. A2A + JACS signatures are the default for service-to-service trust and provenance.
2. OAuth/OIDC is still required for delegated user authorization and user-scoped data access.
3. In `haisdk`, all A2A task/document handoffs should be signable and verifiable using JACS-backed helpers.

## Design Principles (DRY)

1. No new crypto primitives in `haisdk`.
2. Reuse JACS A2A implementations where available, and expose them via `haisdk` facades.
3. Keep one shared A2A contract fixture set for all SDKs.
4. Keep one shared API naming model across languages.
5. Keep quickstarts thin and built on first-class SDK APIs (no duplicated protocol structs in examples).

## Target SDK Surface (Parity)

Add a first-class `A2A` facade per language with these operations:

1. `getA2A()` / `get_a2a()` / `A2A(client)` constructor.
2. `exportAgentCard(...)`
3. `signArtifact(artifact, artifactType, parentSignatures?)`
4. `verifyArtifact(wrappedArtifact, options?)`
5. `createChainOfCustody(artifacts)`
6. `generateWellKnownDocuments(...)`
7. `assessRemoteAgent(card, policy?)`
8. `trustA2AAgent(card)`
9. `registerWithAgentCard(...)` helper that embeds card metadata into `agent_json` and calls HAI registration.

## Version Strategy

1. Treat A2A `v0.4.0` and `v1.0` as supported wire profiles.
2. Introduce internal profile enum/version tag instead of hard-coded `"0.4.0"` literals in examples.
3. Ship fixtures for both versions and validate conversions/compatibility in tests.

## Phase Plan

## Phase 1: First-Class Facades (Node + Python)

### Deliverables

1. Node: `node/src/a2a.ts` thin wrappers delegating to `@hai.ai/jacs` A2A modules.
2. Python: `python/src/haisdk/a2a.py` thin wrappers delegating to `jacs.a2a`.
3. Public exports from `node/src/index.ts` and `python/src/haisdk/__init__.py`, covering the full stable JACS A2A surface.
4. Unified registration helper using agent-card metadata embedding.

### TDD

1. Add failing tests that wrappers call JACS adapters (delegation tests).
2. Add parity tests for method names/return shape between Node and Python.
3. Add fixture-based tests for card/artifact JSON structure.

## Phase 2: Go + Rust Practical Parity

### Deliverables

1. Add `a2a` packages/modules in Go and Rust with the same facade methods.
2. Reuse existing signing/verification hooks in SDKs (no new crypto primitives).
3. Add shared fixture parsing + validation for card/artifact/well-known docs.
4. Add `register_with_agent_card` helper behavior parity.

### TDD

1. Add failing tests for facade behavior and JSON contract compliance.
2. Add cross-language fixture tests (same input => same normalized output shape).
3. Add tests for safe path escaping and public/private endpoint auth behavior where relevant.

## Phase 3: Mediation + Workflow Integration

### Deliverables

1. Add helpers for mediated A2A job handling:
   - receive job via SSE/WS
   - sign inbound task envelope
   - sign outbound result envelope
   - submit response to HAI
2. Add email integration helper for signed task/result exchange links.
3. Add trust-policy gates (`open`, `verified`, `strict`) in runtime flow.

### TDD

1. End-to-end workflow tests with mocked HAI transport/event streams.
2. Failure tests for invalid signatures, trust policy rejection, and missing key material.
3. Golden tests for chain-of-custody generation.

## Phase 4: Examples + Docs Consolidation

### Deliverables

1. Replace current quickstarts with first-class facade APIs (no protocol redefinition duplication).
2. Add practical examples per language:
   - registration with embedded agent card metadata
   - signed task handoff + verification
   - multi-step chain of custody
   - trust-policy enforcement
   - emailing signed artifact links
3. Add one architecture doc linking JACS vs HAISDK ownership boundaries.

### TDD

1. Smoke tests for all examples in CI (where environment permits).
2. Static checks that docs/examples do not diverge from exported API names.

## Shared Fixture Plan

Add `fixtures/a2a/`:

1. `agent_card.v04.json`
2. `agent_card.v10.json`
3. `wrapped_task.minimal.json`
4. `wrapped_task.with_parents.json`
5. `well_known_bundle.v04.json`
6. `well_known_bundle.v10.json`
7. `trust_assessment_cases.json`

Use these fixtures in all language SDK tests.

## Acceptance Criteria

1. All four SDKs expose matching A2A facade operations.
2. All stable JACS A2A capabilities are reachable from `haisdk` (API map + parity tests).
3. Node/Python wrappers are thin delegation layers to JACS A2A modules.
4. Go/Rust provide equivalent behavior and pass shared fixture contracts.
5. A2A workflows are integrated with HAI transport/orchestration and email APIs.
6. Example code uses first-class SDK APIs, not duplicate protocol implementations.

## Practical Example Backlog

1. `a2a_register_and_publish`: register agent and emit `.well-known` document bundle.
2. `a2a_signed_task_roundtrip`: sign task, verify task, sign result, verify result.
3. `a2a_chain_of_custody_pipeline`: multi-agent pipeline with parent signatures.
4. `a2a_trust_policy_gate`: block/allow execution by trust policy.
5. `a2a_mediated_hai_flow`: SSE/WS benchmark job handling with signed artifacts.
6. `a2a_email_dispatch`: send signed task/result links and verify on receipt.

## Production Use Cases

This section defines the highest-frequency real-world A2A scenarios that must be
explicitly supported by `haisdk` (via JACS-backed facades and HAI composition).

### P0: Identity + Discovery

1. Register agent with HAI using JACS identity.
2. Publish and maintain `.well-known` documents and agent card.
3. Keep card metadata aligned with registered `agent_json`.

Required capabilities:

1. `registerWithAgentCard(...)` facade helper.
2. `exportAgentCard(...)` + `generateWellKnownDocuments(...)`.
3. Update/re-register flow when card metadata changes.

Acceptance checks:

1. Cross-language fixture parity for card and well-known document output.
2. Integration tests proving HAI registration recognizes embedded A2A metadata.

### P0: Signed Task/Result Exchange

1. Agent A sends signed task artifact to Agent B.
2. Agent B verifies task, produces signed result, returns result artifact.
3. Both sides can independently verify provenance and integrity.

Required capabilities:

1. `signArtifact(...)` and `verifyArtifact(...)`.
2. Standardized artifact envelope structure across languages.
3. Parent-signature support for chained workflows.

Acceptance checks:

1. Roundtrip tests for task -> result across all SDKs.
2. Negative tests for tampered artifacts and unknown signers.

### P0: Trust Gating In Execution Paths

1. Runtime policy gate before accepting/processing remote A2A artifacts.
2. Support `open`, `verified`, and `strict` policy modes.
3. Trust store integration for strict mode.

Required capabilities:

1. `assessRemoteAgent(...)` and `trustA2AAgent(...)`.
2. Runtime policy hooks in mediated transport processing.

Acceptance checks:

1. Policy matrix tests (`open`/`verified`/`strict`) across SDKs.
2. Tests proving strict mode rejects untrusted agents.

### P1: Mediated Runtime Over HAI Transport

1. Receive jobs over SSE/WS.
2. Sign inbound/outbound task payloads.
3. Submit responses to HAI with preserved provenance.

Required capabilities:

1. Transport handlers that optionally enforce trust policy + artifact verification.
2. Benchmark orchestration helpers that preserve A2A metadata/signatures.

Acceptance checks:

1. End-to-end mocked transport tests with signature/trust assertions.
2. Retry/reconnect tests preserving verification state.

### P1: Key Rotation + Revocation

1. Rotate agent keys without breaking downstream verification.
2. Distinguish active/stale keys in verification and trust workflows.
3. Handle key revocation and rejected signatures safely.

Required capabilities:

1. Public-key refresh and cache invalidation strategy in SDKs.
2. Verification path that reports stale/revoked key outcomes clearly.

Acceptance checks:

1. Rotation tests: old signatures remain auditable; new signatures verify on new key.
2. Revocation tests: revoked/stale keys fail policy where required.

### P1: Replay, Idempotency, and Task Lifecycle

1. Prevent duplicate processing/replay of signed artifacts.
2. Support long-running tasks (progress updates, cancellation, retry semantics).
3. Preserve signed audit trail through lifecycle transitions.

Required capabilities:

1. Artifact IDs + timestamp/window validation guidance.
2. Idempotency hooks for job/task handlers.
3. Lifecycle envelope conventions for progress/cancel/retry events.

Acceptance checks:

1. Replay attack simulation tests.
2. Idempotent re-delivery tests over SSE/WS reconnect scenarios.
3. Lifecycle state-transition tests with signed envelopes.

### P1: OAuth Coexistence (When User Delegation Is Needed)

1. Use A2A+JACS for agent trust and provenance.
2. Use OAuth/OIDC where user consent/scoped delegation is required.
3. Ensure both models can coexist in one request pipeline.

Required capabilities:

1. Clear endpoint-level guidance on when OAuth is required.
2. Examples combining OAuth-protected resource access with signed A2A artifacts.

Acceptance checks:

1. Docs + examples validated in CI for mixed-auth workflows.
2. Tests ensuring A2A verification remains independent of OAuth token validity.

### P1: Version Interop (A2A v0.4.0 and v1.0)

1. Consume and emit compatible artifacts/cards across profile versions.
2. Provide explicit profile selection and normalized internal shape.

Required capabilities:

1. Profile enum in each SDK.
2. Compatibility transformers or adapters where required.

Acceptance checks:

1. Fixture conformance tests for v0.4.0 and v1.0.
2. Interop tests for mixed-version producer/consumer pairs.

## Implementation Order (Recommended)

1. Phase 1 (Node/Python facades + tests)
2. Phase 2 (Go/Rust parity + fixtures)
3. Phase 3 (mediation/email integration)
4. Phase 4 (docs/examples consolidation)

This sequence gives immediate developer value while controlling risk and keeping the implementation DRY.
