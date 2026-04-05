// Client
export { HaiClient, DEFAULT_BASE_URL } from './client.js';

// Agent (high-level wrapper with agent.email.* namespace)
export { Agent, EmailNamespace } from './agent.js';
export type { AgentOptions, SendOptions } from './agent.js';

// MIME construction
export { buildRfc5322Email } from './mime.js';
export type { MimeSendEmailOptions, MimeEmailAttachment } from './mime.js';

// Content hash computation (cross-SDK conformance)
export { computeContentHash } from './hash.js';
export type { ContentHashAttachment } from './hash.js';

// Signing (all crypto delegated to JACS core)
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

// A2A integration wrappers (delegates to @hai.ai/jacs/a2a)
export {
  getA2AIntegration,
  quickstartA2A,
  exportAgentCard,
  signArtifact,
  verifyArtifact,
  createChainOfCustody,
  generateWellKnownDocuments,
  assessRemoteAgent,
  trustA2AAgent,
  mergeAgentJsonWithAgentCard,
  registerWithAgentCard,
  onMediatedBenchmarkJob,
} from './a2a.js';
export type {
  A2ATrustPolicy,
  GetA2AIntegrationOptions,
  QuickstartA2AOptions,
  RegisterWithAgentCardOptions,
  RegisterWithAgentCardResult,
  A2AMediatedJobOptions,
} from './a2a.js';

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
  EmailNotActiveError,
  RecipientNotFoundError,
  RateLimitedError,
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
  ProRunResult,
  DnsCertifiedResult,
  EnterpriseRunResult,
  FullyCertifiedResult,
  BenchmarkResult,
  JobResponseResult,
  VerifyAgentResult,
  RegistrationEntry,
  UpdateUsernameResult,
  DeleteUsernameResult,
  JobResponse,
  AgentCapability,
  ConnectOptions,
  OnBenchmarkJobOptions,
  ProRunOptions,
  DnsCertifiedRunOptions,
  FreeChaoticRunOptions,
  EmailAttachment,
  SendEmailOptions,
  SendEmailResult,
  EmailMessage,
  ListMessagesOptions,
  SearchOptions,
  EmailStatus,
  EmailVolumeInfo,
  EmailDeliveryInfo,
  EmailReputationInfo,
  Contact,
  ForwardOptions,
  EmailTemplate,
  CreateEmailTemplateOptions,
  UpdateEmailTemplateOptions,
  ListEmailTemplatesOptions,
  ListEmailTemplatesResult,
  KeyRegistryResponse,
  EmailVerificationResultV2,
  FieldStatus,
  FieldResult,
  ChainEntry,
  PublicKeyInfo,
  BadgeLevel,
  VerificationResult,
  DocumentVerificationResult,
  AdvancedBadgeLevel,
  AdvancedVerificationStatus,
  AdvancedVerificationResult,
  RotateKeysOptions,
  RotationResult,
  VerifyAgentDocumentOnHaiOptions,
  HaiErrorCode,
  ApiErrorResponse,
} from './types.js';
