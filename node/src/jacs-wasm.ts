// Browser JACS surface for the haiai WASM branch.
//
// This entrypoint deliberately re-exports the local @jacs/wasm package instead
// of wiring it into the default Node SDK entrypoint. Native Node builds keep
// using haiinpm / @hai.ai/jacs; browser builds can import
// `@haiai/haiai/jacs-wasm`.

export {
  CoreAgentHandle,
  algorithmFromPublicKeyLength,
  createAgreementJson,
  createEphemeral,
  createVerifier,
  importEncryptedAgent,
  importEncryptedAgentFiles,
  initJacsWasm,
  localStore,
} from '@jacs/wasm';

export type {
  Algorithm,
  EncryptedAgentFiles,
  JacsWasmError,
} from '@jacs/wasm';

