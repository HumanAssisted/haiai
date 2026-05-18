// Worker-backed browser JACS surface for expensive WASM operations.

export {
  WorkerAgentHandle,
  createEphemeralInWorker,
  importEncryptedAgentInWorker,
  terminateWorker,
} from '@jacs/wasm/worker';

export type {
  Algorithm,
  JacsWorkerError,
} from '@jacs/wasm/worker';

