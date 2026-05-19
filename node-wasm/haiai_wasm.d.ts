// Hand-maintained stub mirroring the wasm-bindgen exports of
// `rust/haiai-wasm/` (HAIAI_WASM_PRD §4.3 / §4.10 / Task 031).
//
// Used only by `tsc --noEmit` from a fresh checkout — at publish time
// `finalize-pkg.sh` produces the real `pkg/haiai_wasm.d.ts` from
// `wasm-pack build` and ships that instead. Keep this stub in sync
// with the wasm-bindgen exports surface (Tasks 021-029).

/** Initialize the wasm runtime. Idempotent. */
export function initHaiaiWasm(): Promise<void>;

/** Package version string (matches rust/haiai-wasm/Cargo.toml::version). */
export function version(): string;

/** One-line build descriptor. */
export function about(): string;

/**
 * Stateful browser agent handle. Real lifecycle constructors
 * (`createEphemeral`, `importEncrypted`, `publicOnly`) + local crypto
 * (`signMessageJson`, `verifyJson`) + HAI HTTP wrappers (`hello`,
 * `sendSignedEmail`, ...) + event streams land in Tasks 021-029.
 *
 * Stub for now so the TS skeleton typechecks.
 */
export class BrowserAgentHandle {
  free(): void;
}

/** SSE / WS event stream handle (Task 029 lands the real surface). */
export class EventStreamHandle {
  free(): void;
}

declare const _default: () => Promise<unknown>;
export default _default;
