// @haiai/wasm/worker/haiai-worker.js — Web Worker entry (Task 035).
//
// This file runs INSIDE a DedicatedWorker. Importing the wasm-pack
// `--target web` output triggers a `fetch()` of `haiai_wasm_bg.wasm`
// from the bundler's emitted URL; the worker bundler must rewrite
// `./haiai_wasm.js` to a worker-resolvable path. Vite handles this
// automatically; webpack 5+ via `experiments.outputModule: true`.
//
// Wire protocol (mirrors `worker/index.ts`):
//   incoming: { id, op, args }
//   reply:    { id, ok: true, value } | { id, ok: false, error: { code, message, details? } }

import init, {
  initHaiaiWasm,
  BrowserAgentHandle,
} from "../haiai_wasm.js";

let agent = null;
let baseUrl = null;

function postOk(id, value) {
  self.postMessage({ id, ok: true, value });
}

function postErr(id, err) {
  // err is JS Error whose message is a JSON payload from wasm-bindgen.
  let payload;
  try {
    payload = JSON.parse(err.message);
  } catch {
    payload = { code: "Internal", message: String(err) };
  }
  self.postMessage({ id, ok: false, error: payload });
}

self.addEventListener("message", async (evt) => {
  const { id, op, args } = evt.data;
  try {
    switch (op) {
      case "init": {
        await init(args?.wasmUrl);
        initHaiaiWasm();
        baseUrl = args?.baseUrl ?? null;
        postOk(id);
        return;
      }
      case "createEphemeral": {
        agent = BrowserAgentHandle.createEphemeral(args.algorithm, baseUrl);
        postOk(id, { jacsId: agent.jacsId(), algorithm: agent.algorithm() });
        return;
      }
      case "importEncrypted": {
        agent = BrowserAgentHandle.importEncrypted(args.materialJson, args.password, baseUrl);
        postOk(id, { jacsId: agent.jacsId() });
        return;
      }
      case "sign": {
        if (!agent) throw new Error('{"code":"NoAgent","message":"create or import an agent first"}');
        const signedJson = agent.signMessageJson(JSON.stringify(args));
        postOk(id, JSON.parse(signedJson));
        return;
      }
      case "verify": {
        if (!agent) throw new Error('{"code":"NoAgent","message":"create or import an agent first"}');
        const out = agent.verifyJson(JSON.stringify(args));
        postOk(id, out);
        return;
      }
      case "sendSignedEmail": {
        if (!agent) throw new Error('{"code":"NoAgent","message":"create or import an agent first"}');
        const out = await agent.sendSignedEmail(JSON.stringify(args));
        postOk(id, out);
        return;
      }
      case "signEmailRaw": {
        if (!agent) throw new Error('{"code":"NoAgent","message":"create or import an agent first"}');
        const out = await agent.signEmailRaw(args.rawEmailB64);
        postOk(id, out);
        return;
      }
      default:
        throw new Error(`{"code":"UnknownOp","message":"unknown worker op: ${op}"}`);
    }
  } catch (err) {
    postErr(id, err);
  }
});
