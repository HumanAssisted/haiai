// Smoke import — verifies the `@haiai/wasm` shell typechecks against
// the published surface. Used by `tsc --noEmit -p node-wasm/tsconfig.json`
// to catch obvious shape regressions in the hand-written wrapper.
import { initHaiaiWasm, version, BrowserAgent, HaiaiWasmError } from "../../index.js";

export async function smoke(): Promise<string> {
  await initHaiaiWasm();
  try {
    // BrowserAgent surface lands in Task 032; the skeleton intentionally
    // throws so consumers fail fast if they hit it before the real
    // methods land.
    await BrowserAgent.createEphemeral();
    return "unexpected: createEphemeral resolved on the skeleton";
  } catch (err) {
    if (err instanceof HaiaiWasmError) {
      return `${version()}: ${err.code}`;
    }
    throw err;
  }
}
