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

// Verify link
export { generateVerifyLink, MAX_VERIFY_URL_LEN, MAX_VERIFY_DOCUMENT_BYTES } from './verify.js';

// JACS CLI passthrough helpers
export { runJacsCli, resolveJacsCliBin } from './jacs.js';
export type { RunJacsCliOptions } from './jacs.js';

// Framework integrations (LangGraph/LangChain, MCP, Agent SDK wrapper)
export {
  langchainSignedTool,
  langgraphWrapToolCall,
  langgraphToolNode,
  createJacsLangchainTools,
  createJacsMcpTransportProxy,
  getJacsMcpToolDefinitions,
  registerJacsMcpTools,
  createAgentSdkToolWrapper,
  verifyAgentSdkPayload,
} from './integrations.js';
export type {
  JacsModuleError,
  AgentSdkSigningClient,
  AgentSdkToolWrapperOptions,
} from './integrations.js';

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
  DnsCertifiedResult,
  FullyCertifiedResult,
  BenchmarkResult,
  JobResponseResult,
  VerifyAgentResult,
  RegistrationEntry,
  CheckUsernameResult,
  ClaimUsernameResult,
  JobResponse,
  AgentCapability,
  ConnectOptions,
  OnBenchmarkJobOptions,
  DnsCertifiedRunOptions,
  FreeChaoticRunOptions,
  SendEmailOptions,
  SendEmailResult,
  EmailMessage,
  ListMessagesOptions,
  EmailStatus,
  PublicKeyInfo,
  BadgeLevel,
  VerificationResult,
} from './types.js';
