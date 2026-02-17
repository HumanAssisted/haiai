// Client
export { HaiClient } from './client.js';

// Crypto
export { signString, verifyString, generateKeypair } from './crypt.js';

// Signing
export {
  unwrapSignedEvent,
  signResponse,
  getServerKeys,
  clearServerKeysCache,
  canonicalJson,
} from './signing.js';
export type { JacsDocument } from './signing.js';

// Config
export { loadConfig, loadPrivateKey } from './config.js';

// SSE parser
export { parseSseStream } from './sse.js';

// WebSocket helpers
export { openWebSocket, wsRecv, wsEventStream } from './ws.js';
export type { WsLike } from './ws.js';

// Errors
export {
  HaiError,
  AuthenticationError,
  HaiConnectionError,
  WebSocketError,
  RegistrationError,
  BenchmarkError,
  SSEError,
  HaiApiError,
} from './errors.js';

// Types
export type {
  HaiClientOptions,
  AgentConfig,
  EventType,
  HaiEvent,
  ConnectionMode,
  BenchmarkTier,
  BenchmarkJob,
  BenchmarkJobConfig,
  TranscriptMessage,
  ConversationTurn,
  HelloWorldResult,
  RegistrationResult,
  FreeChaoticResult,
  BaselineResult,
  CertifiedResult,
  BenchmarkResult,
  JobResponseResult,
  StatusResult,
  JobResponse,
  AgentCapability,
  ConnectOptions,
  OnBenchmarkJobOptions,
  BaselineRunOptions,
  FreeChaoticRunOptions,
} from './types.js';
