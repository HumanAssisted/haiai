# haiai purpose and the advocate surface — plan (October 8, 2026)

**Status:** plan. Nothing here is implemented or published. The product design, rulings and phases live in the hai repository: [Third-party advocate agents on hai.ai — investigation and plan](https://github.com/HumanAssisted/hai/blob/claude/hai-agent-integration-plan-dpisct/docs/OCT8_2026_EXTERNAL_ADVOCATE_AGENTS_PLAN.md) (PR [HumanAssisted/hai#148](https://github.com/HumanAssisted/hai/pull/148)). This document is the SDK-side companion: what haiai is for after that plan, what changes, and in which release.

## Founder direction (verbatim, October 8)

> The hai.ai sdk is is supposed to do this for devs. Now it is focused on mediation bench, so a major change for that sdk.

> Haiai sdk needs this clear purpose but we wouldn't need it for integration with closed source platforms. I care a lot bout the jacs security model. [...] Agents though need an id that a user consents to.

## Purpose

haiai is the developer kit for agents you run yourself with your own JACS key. Its main job is to act for one person as that person's **advocate** on hai.ai: pair the agent with the person, read that person's side of an Agreement, answer hai's interview questions as the agent's labelled words, and suggest draft wording, while the person confirms, shares and approves in the hai app. It also signs and verifies JACS documents and email for those agents.

Closed platforms (ChatGPT and dots, claude.ai, Grok Bot, Muse) do not use haiai; they reach the same advocate lane through hai's remote MCP door. Instinct has no documented developer surface yet and is tracked, not covered. MediationBench tooling stays in the SDK as lab tooling, not as its purpose.

What haiai never does: consent, approve, confirm understanding, share private points, send invitations, or sign an Agreement for a person. An agent signature is provenance, never a person's approval ([ADR 0001](adr/0001-crypto-delegation-to-jacs.md)).

## Where the SDK is today (verified October 8, 2026)

- Source is 0.4.1 in ten lockstep packages; nothing after 0.4.0 (April 28, 2026) is published on crates.io, PyPI or npm. Published 0.4.0 predates the request-bound `JACS v2.` auth the hai API now requires (`rust/haiai/src/request_auth.rs`), so no published SDK can authenticate today.
- The only job contract is the MediationBench mediator worker: the `benchmark_mediator` capability, admin admission, SSE/WebSocket jobs with journaled, context-bound signed replies (`rust/haiai/src/client.rs`, `python/examples/benchmark_mediator.py`, `fixtures/benchmark_mediator_contract.json`).
- The Rust client already has `get_agreement_intake`, `get_agreement_intake_by_public_id` and `record_agreement_interview_turn` (`client.rs:1685-1743`), but they are untyped, not exposed through `hai-binding-core`, the FFI bindings or the MCP server, and the hai server binds only its own hosted agent to a participant, so no third-party agent can use them.
- `haiai mcp` (stdio, rmcp) exposes email, identity, signing, documents, memory and `hai_conflict_*` tools and no Agreement tools (`rust/hai-mcp/src/hai_tools.rs`). Its default profile signs with the agent key (`hai_sign_text`, `hai_conflict_create`), `hai_register_agent` takes the `registration_key` bearer secret as a tool argument (`hai_tools.rs:196, 839`), and `skills/jacs/SKILL.md:228-234` still teaches `jacs_create_agreement` and `jacs_sign_agreement`, which JACS removed from the portable MCP.
- Reusable as is: the request signer in all four languages, the verified live event stream and signed-reply pattern, the opaque-handle streaming FFI, registration with A2A card merge, the Claude plugin and skill packaging (`.claude-plugin/`, `.mcp.json`).
- Constraints that hold: no local crypto (rule 1); four-language parity from shared fixtures (rule 2); ten manifests plus three lockfiles move together (rule 3); tag-triggered releases (rule 4); HTTP only in `rust/haiai` (rule 5). Developer MCP signing must remain, per hai's September 5 ruling ("MCP signing remains as JACS will be used by devs"), so the advocate surface is additive, not a removal.

## Release 0.5.0 — one purpose, no new authority (hai phase P0b)

Can ship before any hai server change.

- Rewrite `README.md`, `QUICKSTART` material and `AGENTS.md` around the purpose above. The MediationBench section moves below the fold and links to mediationbench.com.
- Publish the JACS v2 request auth already in source, across all ten packages and three lockfiles.
- Move MediationBench methods (`benchmark`, `submit_response`, `on_benchmark_job`, `on_benchmark_job_with_reconnect`, A2A `on_mediated_benchmark_job`) to `haiai::lab::mediationbench` (Python `haiai.lab`, Node `haiai/lab`; Go once the module path is fixed) with deprecated re-exports at the old paths through 0.6.x. Examples move to `examples/lab/`; tests stay; the server contract is unchanged. A default-on `lab` Cargo feature becomes default-off no earlier than 0.7.
- Correct `skills/jacs/SKILL.md` so it no longer advertises agent agreement signing.
- Deprecate the `registration_key` MCP tool parameter in favour of a CLI path that reads the key from a hidden terminal prompt only; keep it working for one minor version.
- Document `save_agreement` and `countersign_agreement` as provenance-only and never a consent path (the hai records lane is off in production and its v2 verification never accepts policy). Document `get_agreement_intake` and `record_agreement_interview_turn` as hosted-lane only.
- Mark `haiai deploy anthropic` unsupported for advocates: it co-locates the key and password in a container an LLM can read, against hai's September 17 Task 009 ("never move a key into an LLM process"). Developer use awaits the founder's answer.

Breaking under 0.x caret semantics, so the minor bump is required. Tests: `lab_aliases_still_dispatch_benchmark_methods` (Rust, Python, Node), `jacs_skill_does_not_advertise_agent_agreement_signing`, `register_agent_cli_reads_key_from_tty_only`, `scripts/ci/check_no_local_crypto.sh`, `make check-versions`, and hai's path consumers (`agent-runtime`, `tests/signing-journey`) green at the new pin.

## Release 0.6.0 — the advocate surface (with hai phase P2)

Ships only when hai's advocate lane exists (`/api/v1/agreements/advocate/*`), compiled closed behind hai's flag.

- `rust/haiai/src/advocate.rs`: an `AdvocateClient` mapped 1:1 to the lane: `pair_redeem`, `status`, `instructions`, `get_step`, `read_private_summary`, `answer(AnswerInput { context_key, turn_id, basis, text })`, `read_draft`, `suggest_wording`. HTTP only in `rust/haiai`; all signing through the existing request signer.
- Typed errors: `HumanActionRequired { kind, url }`, `NotGranted`, `GrantPaused`, `ConnectionRevoked`, `ConfirmedLocked`, `StaleQuestion`, `RateLimited`, `PolicyRefused`. A 403 policy refusal is not an auth failure; the agent asks its person.
- `hai-binding-core` JSON wrappers plus PyO3, napi-rs and CGo methods with Python, Node and Go facades in the same release, driven by `fixtures/advocate/*.json`. `tools.v1.json` is generated from hai's `advocate-contract` crate and checked in, so the remote and stdio tool lists cannot drift.
- `haiai mcp --profile advocate`: a stdio MCP exposing exactly the seven advocate tools plus `jacs_verify_document`, with the same names and annotations as hai's remote door. Embedded JACS is forced to verify-only. A compile-time allowlist test excludes `jacs_sign_*`, `hai_sign_*`, `hai_conflict_*`, email, memory, soul, records and `hai_register_agent`. Key passwords come from the environment or keychain, never from tool arguments. The default `haiai mcp` profile is unchanged.
- CLI: `haiai advocate init` (a separate pq2025 identity in its own config directory), `pair` (reads the person's one-use code from a hidden prompt and prints the key fingerprint for the person to compare), `status`, `step`, `answer`, `draft`, `suggest`.
- A separate advocate-only plugin and manifest (`hai-advocate`, with its own `.mcp.json` that starts only `haiai mcp --profile advocate`). It is never an entry appended to the existing developer plugin: that plugin's `.mcp.json` starts the unrestricted `haiai mcp`, whose server combines the JACS tools with every HAI tool including email (`rust/hai-mcp/src/server.rs:30-40`), and HAI tool dispatch is independent of the JACS profile (`server.rs:92`), so a machine with an existing developer identity would otherwise expose both servers to the same model. The developer plugin stays a separate opt-in. `skills/hai-advocate/SKILL.md` is generated from hai's public instruction pack with a CI digest check, and `self_knowledge` embeds the pack.
- A Rust reference worker, `examples/advocate_worker`: poll status, read the step, ask the person in the host, post the answer. No push: hai's October 5 ruling keeps notifications on existing database and API reads.

Tests: `advocate_profile_tool_allowlist`, `advocate_profile_refuses_excluded_tools_at_dispatch` (direct `tools/call` of every excluded name is refused, not merely unlisted), `advocate_plugin_installs_only_the_advocate_server` (complete installed tool inventory), `advocate_fixture_parity_{rust,python,node,go}`, `advocate_tools_never_accept_key_or_password`, `default_profile_retains_developer_signing`. Every advocate request carries the grant id and signed-grant digest inside the signed body or query, as hai's lane requires.

## How a developer's agent becomes a person's advocate

1. The developer runs `haiai advocate init`. The agent holds its own pq2025 key; hai never sees it.
2. The person creates a one-use pairing code in the hai app (Agents tab). The developer runs `haiai advocate pair` and enters it at a hidden prompt. The agent redeems the code with its self-signed JACS agent document plus a request proof signed by the same key, which proves possession. Both sides show the key fingerprint. Pairing grants no Agreement access.
3. On an Agreement, the person holds "Let your developer agent help with this Agreement" and signs the grant with their device key. The grant names the agent's key hash, capabilities, the Agreement participant, the instruction pack and an expiry.
4. The agent polls `status` and `step`, works with its person, posts `answer` (labelled as the agent's words; never consent), reads the draft and may `suggest_wording`. The person confirms, shares and approves in hai.
5. The person can Stop (one Agreement) or Disconnect (everything) at any time from the app.

## Open questions (for the founder, mirrored from the hai plan)

- Is BUSL-1.1 acceptable for a developer kit meant for broad third-party adoption, while JACS is Apache-2.0?
- Is Go in scope for 0.6.0 without a `libhaiigo` distribution and with the module path unresolved, or documented as source-only?
- Retire `haiai deploy anthropic`, or fix it (it installs 0.4.0, allowlists `api.hai.ai` only, and co-locates key and password)?
- Retire the records-lane `countersign_agreement` and `save_agreement` for every agent, not only advocates?
- The pq2025 request-auth header is about 7 KB, near common 8 KB proxy header buffers; hai sets the nginx buffer explicitly in P2, and the SDK should surface a clear error if a proxy truncates it.
