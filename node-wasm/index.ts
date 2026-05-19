// @haiai/wasm — hand-written ergonomic wrapper around the wasm-bindgen
// output from rust/haiai-wasm/.
//
// Skeleton only — Tasks 032-034 land the real BrowserAgent +
// BrowserHaiClient surfaces. The marker line above is read by
// scripts/ci/check_wasm_surface.sh so the wasm-surface drift check
// skips the TS side until Task 033 lands the full method set.
//
// HAIAI_WASM_PRD §4.4: one initializer (`initHaiaiWasm`), one stateful
// entry point (`BrowserAgent`), typed promises, stable error codes.

import init, {
  initHaiaiWasm as wasmInit,
  version as wasmVersion,
  about as wasmAbout,
} from "./haiai_wasm.js";

let initialized = false;

/**
 * Initialize the wasm runtime. Idempotent — safe to call multiple
 * times. Internally:
 *   1. Bootstraps the @jacs/wasm runtime (TODO Task 032: wire in
 *      `await initJacsWasm()` once the dep resolution is settled).
 *   2. Loads the haiai-wasm .wasm artifact via the default export.
 *   3. Calls the haiai-wasm-side init (`init_haiai_wasm` Rust fn),
 *      which sets up the panic hook + any haiai-side tracing.
 */
export async function initHaiaiWasm(): Promise<void> {
  if (initialized) return;
  // Load the .wasm artifact. wasm-pack's `--target web` output exposes
  // a default async fn that fetches + instantiates the module.
  await init();
  // haiai-wasm-side bookkeeping (panic hook, etc.).
  await wasmInit();
  initialized = true;
}

/** @haiai/wasm package version (matches rust/haiai-wasm/Cargo.toml). */
export function version(): string {
  return wasmVersion();
}

/** Build descriptor for diagnostics. */
export function about(): string {
  return wasmAbout();
}

/**
 * Typed error surface — every `@haiai/wasm` failure path returns a
 * `HaiaiWasmError` with a stable `code` discriminator. PRD §3.1
 * enumerates the codes (JACS-layer pass-throughs + HAI-layer codes).
 */
export class HaiaiWasmError extends Error {
  readonly code: string;
  readonly details?: unknown;
  constructor(code: string, message: string, details?: unknown) {
    super(message);
    this.code = code;
    this.details = details;
    this.name = "HaiaiWasmError";
  }
}

/**
 * Stateful browser agent. Real constructors + methods land in Tasks
 * 032-034 (BrowserAgent class + BrowserHaiClient interface). For now
 * this is a sentinel that throws on use so consumers fail fast if they
 * try to use the skeleton before the real surface lands.
 */
export const BrowserAgent = {
  createEphemeral(): Promise<never> {
    return Promise.reject(
      new HaiaiWasmError(
        "NotImplemented",
        "BrowserAgent.createEphemeral lands in Task 032; the skeleton only exposes initHaiaiWasm / version / about",
      ),
    );
  },
  importEncrypted(): Promise<never> {
    return Promise.reject(
      new HaiaiWasmError(
        "NotImplemented",
        "BrowserAgent.importEncrypted lands in Task 032",
      ),
    );
  },
  publicOnly(): Promise<never> {
    return Promise.reject(
      new HaiaiWasmError(
        "NotImplemented",
        "BrowserAgent.publicOnly lands in Task 032",
      ),
    );
  },
  load(): Promise<never> {
    return Promise.reject(
      new HaiaiWasmError(
        "NotImplemented",
        "BrowserAgent.load lands in Task 032 (wraps @jacs/wasm localStore.loadEncryptedAgent)",
      ),
    );
  },
} as const;
