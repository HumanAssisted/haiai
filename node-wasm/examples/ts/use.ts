// Smoke import — verifies the `@haiai/wasm` shell typechecks against
// the published surface. Used by `tsc --noEmit -p node-wasm/tsconfig.json`
// to catch obvious shape regressions in the hand-written wrapper.
import { initHaiaiWasm, version, BrowserAgent, HaiaiWasmError } from "../../index.js";
import type { HelloResult } from "../../types.js";

export async function smoke(): Promise<string> {
  await initHaiaiWasm();
  try {
    const agent = await BrowserAgent.createEphemeral("ed25519");
    const hello: HelloResult = await agent.client.hello(false);
    return `${version()}: jacs_id=${agent.jacsId()} hello.message=${hello.message}`;
  } catch (err) {
    if (err instanceof HaiaiWasmError) {
      return `${version()}: ${err.code}`;
    }
    throw err;
  }
}
