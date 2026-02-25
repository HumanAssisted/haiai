// =============================================================================
// Core types
// =============================================================================

/** Options for HaiClient constructor. */
export interface HaiClientOptions {
  /** Path to jacs.config.json. Defaults to JACS_CONFIG_PATH env or ./jacs.config.json. */
  configPath?: string;
  /** HAI server URL. Default: https://hai.ai */
  url?: string;
  /** Request timeout in milliseconds. Default: 30000. */
  timeout?: number;
  /** Maximum retry attempts for retryable requests. Default: 3. */
  maxRetries?: number;
}

/** JACS agent configuration loaded from jacs.config.json. */
export interface AgentConfig {
  jacsAgentName: string;
  jacsAgentVersion: string;
  jacsKeyDir: string;
  jacsId?: string;
  jacsPrivateKeyPath?: string;
}

// =============================================================================
// Event types
// =============================================================================

/** Event types emitted by the HAI event stream. */
export type EventType =
  | 'connected'
  | 'benchmark_job'
  | 'heartbeat'
  | 'error'
  | 'disconnected'
  | 'job_complete'
  | 'score'
  | string;

/** An event received from HAI via SSE or WebSocket. */
export interface HaiEvent {
  /** Type of event (e.g., "benchmark_job", "heartbeat", "connected"). */
  eventType: EventType;
  /** Event payload as parsed JSON. */
  data: unknown;
  /** Event ID if provided by the server. */
  id?: string;
  /** Raw event data string. */
  raw: string;
}

/** Connection transport mode. */
export type ConnectionMode = 'sse' | 'ws';

// =============================================================================
// Benchmark types
// =============================================================================

/** Benchmark tier identifiers. */
export type BenchmarkTier = 'free' | 'dns_certified' | 'fully_certified';

/** A benchmark job received from HAI via SSE or WebSocket. */
export interface BenchmarkJob {
  /** Unique run/job ID. */
  runId: string;
  /** Scenario description or prompt for the mediator. */
  scenario: unknown;
  /** Full event data. */
  data: Record<string, unknown>;
}

/** Configuration for a benchmark job. */
export interface BenchmarkJobConfig {
  /** Benchmark tier. */
  tier: BenchmarkTier;
  /** Run name. */
  name?: string;
  /** Transport protocol for the benchmark. */
  transport?: ConnectionMode;
  /** Stripe payment ID (required for dns_certified). */
  paymentId?: string;
}

/** A single message in a benchmark transcript. */
export interface TranscriptMessage {
  /** Speaker role ("party_a", "party_b", "mediator", "system"). */
  role: string;
  /** Message text content. */
  content: string;
  /** ISO 8601 timestamp of the message. */
  timestamp: string;
  /** Structural annotations (e.g., "Dispute escalated"). */
  annotations: string[];
}

/** A turn in a conversation (alias for TranscriptMessage). */
export type ConversationTurn = TranscriptMessage;

// =============================================================================
// Result types
// =============================================================================

/** Result of a hello world exchange with HAI. */
export interface HelloWorldResult {
  /** Whether the exchange succeeded. */
  success: boolean;
  /** ISO 8601 timestamp from HAI's response. */
  timestamp: string;
  /** The caller's IP address as seen by HAI. */
  clientIp: string;
  /** HAI's public key fingerprint. */
  haiPublicKeyFingerprint: string;
  /** Human-readable acknowledgment message from HAI. */
  message: string;
  /** HAI's signed acknowledgment. */
  haiSignedAck: string;
  /** Unique hello exchange ID. */
  helloId: string;
  /** Test scenario preview (if requested). */
  testScenario?: unknown;
  /** Whether HAI's signature on the ACK was verified. */
  haiSignatureValid: boolean;
  /** Full response from the API. */
  rawResponse: Record<string, unknown>;
}

/** Result of registering an agent with HAI. */
export interface RegistrationResult {
  success: boolean;
  agentId: string;
  jacsId: string;
  haiSignature: string;
  registrationId: string;
  registeredAt: string;
  rawResponse: Record<string, unknown>;
}

/** Result of a free chaotic benchmark run. No score, transcript only. */
export interface FreeChaoticResult {
  /** Whether the run completed. */
  success: boolean;
  /** Unique ID for this benchmark run. */
  runId: string;
  /** List of transcript messages. */
  transcript: TranscriptMessage[];
  /** CTA message for paid tiers. */
  upsellMessage: string;
  /** Full response from the API. */
  rawResponse: Record<string, unknown>;
}

/** Result of a $5 DNS-certified benchmark run. Single score, no breakdown. */
export interface DnsCertifiedResult {
  /** Whether the run completed. */
  success: boolean;
  /** Unique ID for this benchmark run. */
  runId: string;
  /** Single aggregate score (0-100). */
  score: number;
  /** List of transcript messages. */
  transcript: TranscriptMessage[];
  /** ID of the Stripe payment used. */
  paymentId: string;
  /** Full response from the API. */
  rawResponse: Record<string, unknown>;
}

/** Result of a $499 fully-certified benchmark run. */
export interface FullyCertifiedResult {
  /** Whether the run completed. */
  success: boolean;
  /** Unique ID for this benchmark run. */
  runId: string;
  /** Aggregate score (0-100). */
  score: number;
  /** Category-level breakdowns. */
  categories: Record<string, number>;
  /** List of transcript messages. */
  transcript: TranscriptMessage[];
  /** ID of the Stripe payment used. */
  paymentId: string;
  /** Full response from the API. */
  rawResponse: Record<string, unknown>;
}

/** Generic benchmark result (union of all tiers). */
export type BenchmarkResult = FreeChaoticResult | DnsCertifiedResult | FullyCertifiedResult;

/** Result of submitting a benchmark job response. */
export interface JobResponseResult {
  /** Whether the response was accepted. */
  success: boolean;
  /** The job ID that was responded to. */
  jobId: string;
  /** Acknowledgment message from HAI. */
  message: string;
  /** Full response from the API. */
  rawResponse: Record<string, unknown>;
}

/** A single registration entry from the verify endpoint. */
export interface RegistrationEntry {
  /** Key ID used for this registration. */
  keyId: string;
  /** Signature algorithm (e.g., "Ed25519"). */
  algorithm: string;
  /** Signature JSON payload. */
  signatureJson: string;
  /** Timestamp when the registration was signed. */
  signedAt: string;
}

/** Result of verifying an agent's registration (GET /api/v1/agents/{jacs_id}/verify). */
export interface VerifyAgentResult {
  /** The agent's JACS ID. */
  jacsId: string;
  /** Whether the agent is registered with HAI. */
  registered: boolean;
  /** List of registration entries. */
  registrations: RegistrationEntry[];
  /** Whether DNS has been verified for the agent's domain. */
  dnsVerified: boolean;
  /** Agent registration timestamp. */
  registeredAt: string;
  /** Full response from the API. */
  rawResponse: Record<string, unknown>;
}

/** Result of checking username availability. */
export interface CheckUsernameResult {
  /** Whether the username is available. */
  available: boolean;
  /** The username that was checked. */
  username: string;
  /** Reason if unavailable. */
  reason?: string;
}

/** Result of claiming a username. */
export interface ClaimUsernameResult {
  /** The claimed username. */
  username: string;
  /** The resulting hai.ai email address. */
  email: string;
  /** The agent ID the username was claimed for. */
  agentId: string;
}

/** Result of updating (renaming) a username. */
export interface UpdateUsernameResult {
  /** The new username. */
  username: string;
  /** The resulting hai.ai email address. */
  email: string;
  /** Previous username before rename. */
  previousUsername: string;
}

/** Result of deleting a username claim. */
export interface DeleteUsernameResult {
  /** Released username placed into cooldown. */
  releasedUsername: string;
  /** ISO 8601 timestamp when cooldown expires. */
  cooldownUntil: string;
  /** Human-readable server message. */
  message: string;
}

/** Payload submitted to HAI for a benchmark job response. */
export interface JobResponse {
  response: {
    message: string;
    metadata: Record<string, unknown> | null;
    processing_time_ms: number;
  };
}

// =============================================================================
// Agent types
// =============================================================================

/** Capabilities an agent can declare. */
export type AgentCapability =
  | 'mediation'
  | 'arbitration'
  | 'negotiation'
  | 'translation'
  | 'summarization'
  | string;

// =============================================================================
// Connection options
// =============================================================================

/** Options for HaiClient.connect(). */
export interface ConnectOptions {
  /** Transport protocol: "sse" (default) or "ws" (WebSocket). */
  transport?: ConnectionMode;
  /** Callback function called for each event. */
  onEvent?: (event: HaiEvent) => void;
}

/** Options for HaiClient.onBenchmarkJob(). */
export interface OnBenchmarkJobOptions {
  /** Transport protocol: "sse" (default) or "ws". */
  transport?: ConnectionMode;
}

/** Options for DNS-certified benchmark run. */
export interface DnsCertifiedRunOptions {
  /** Transport protocol. */
  transport?: ConnectionMode;
  /** Milliseconds between payment status checks. Default: 2000. */
  pollIntervalMs?: number;
  /** Max milliseconds to wait for payment. Default: 300000 (5 min). */
  pollTimeoutMs?: number;
  /** Callback with checkout URL (e.g., to open in browser). */
  onCheckoutUrl?: (url: string) => void;
}

/** Options for free chaotic run. */
export interface FreeChaoticRunOptions {
  /** Transport protocol. */
  transport?: ConnectionMode;
}

// =============================================================================
// Email types
// =============================================================================

/** Options for sending an email. */
export interface SendEmailOptions {
  /** Recipient email address. */
  to: string;
  /** Email subject line. */
  subject: string;
  /** Email body text. */
  body: string;
  /** Message ID to reply to (for threading). */
  inReplyTo?: string;
  /** File attachments to include with the email. */
  attachments?: Array<{
    /** Attachment file name. */
    filename: string;
    /** MIME content type (e.g., "application/pdf"). */
    contentType: string;
    /** Raw attachment data. */
    data: Buffer;
  }>;
}

/** Result of sending an email. */
export interface SendEmailResult {
  /** Unique message ID assigned by HAI. */
  messageId: string;
  /** Delivery status. */
  status: string;
}

/** An email message. */
export interface EmailMessage {
  /** Unique message ID. */
  id: string;
  /** Direction: "inbound" or "outbound". */
  direction: string;
  /** Sender email address. */
  fromAddress: string;
  /** Recipient email address. */
  toAddress: string;
  /** Email subject. */
  subject: string;
  /** Email body text. */
  bodyText: string;
  /** RFC 2822 Message-ID. */
  messageId: string;
  /** Message-ID of the parent message (for threading), or null. */
  inReplyTo: string | null;
  /** Whether the message has been read. */
  isRead: boolean;
  /** Delivery status (e.g., "queued", "delivered", "failed"). */
  deliveryStatus: string;
  /** ISO 8601 timestamp when the message was created. */
  createdAt: string;
  /** ISO 8601 timestamp when the message was read, or null. */
  readAt: string | null;
  /** Whether the JACS signature on this message was verified. */
  jacsVerified: boolean;
}

/** Options for listing email messages. */
export interface ListMessagesOptions {
  /** Max number of messages to return. */
  limit?: number;
  /** Offset for pagination. */
  offset?: number;
  /** Filter by direction: "inbound" or "outbound". */
  direction?: 'inbound' | 'outbound';
}

/** Options for searching email messages. */
export interface SearchOptions {
  /** Search query string. */
  query: string;
  /** Max number of results. */
  limit?: number;
  /** Offset for pagination. */
  offset?: number;
  /** Filter by direction: "inbound" or "outbound". */
  direction?: 'inbound' | 'outbound';
  /** Filter by sender address. */
  fromAddress?: string;
  /** Filter by recipient address. */
  toAddress?: string;
}

/** Email rate limit and status info. */
export interface EmailStatus {
  /** The agent's email address. */
  email: string;
  /** Email provisioning status. */
  status: string;
  /** Agent's reputation tier. */
  tier: string;
  /** Current billing tier. */
  billingTier: string;
  /** Messages sent in the last 24 hours. */
  messagesSent24h: number;
  /** Maximum emails per day for current tier. */
  dailyLimit: number;
  /** Emails sent today. */
  dailyUsed: number;
  /** ISO 8601 timestamp when the daily counter resets. */
  resetsAt: string;
  /** Total messages sent all time. */
  messagesSentTotal: number;
  /** Whether external (non-hai.ai) email sending is enabled. */
  externalEnabled: boolean;
  /** Number of external emails sent today. */
  externalSendsToday: number;
  /** ISO 8601 timestamp of last tier change, or null. */
  lastTierChange: string | null;
}

/** Response from the public key registry endpoint. */
export interface KeyRegistryResponse {
  /** The agent's email address. */
  email: string;
  /** The agent's JACS ID. */
  jacsId: string;
  /** Base64-encoded public key. */
  publicKey: string;
  /** Signature algorithm (e.g., "ed25519"). */
  algorithm: string;
  /** Agent's reputation tier. */
  reputationTier: string;
  /** ISO 8601 registration timestamp. */
  registeredAt: string;
}

/** Result of verifying an email's JACS signature. */
export interface EmailVerificationResult {
  /** Whether the signature is valid. */
  valid: boolean;
  /** The signer's JACS ID. */
  jacsId: string;
  /** The signer's reputation tier. */
  reputationTier: string;
  /** Error message if verification failed, or null. */
  error: string | null;
}

// =============================================================================
// Key lookup types
// =============================================================================

/** Public key information returned by the key lookup endpoint. */
export interface PublicKeyInfo {
  /** Agent's JACS ID. */
  jacsId: string;
  /** Key version. */
  version: string;
  /** Public key in PEM format. */
  publicKey: string;
  /** Base64-encoded raw public key bytes. */
  publicKeyRawB64: string;
  /** Signature algorithm (e.g., "Ed25519"). */
  algorithm: string;
  /** Hash of the public key. */
  publicKeyHash: string;
  /** Key status (e.g., "active"). */
  status: string;
  /** Whether DNS has been verified for this agent. */
  dnsVerified: boolean;
  /** ISO 8601 timestamp when the key was created. */
  createdAt: string;
}

// =============================================================================
// Verification types
// =============================================================================

/** Badge levels for agent verification. */
export type BadgeLevel = 'none' | 'basic' | 'domain' | 'attested';

/** Result of verifying an agent document. */
export interface VerificationResult {
  /** Whether the Ed25519 signature is valid. */
  signatureValid: boolean;
  /** Whether DNS has been verified. */
  dnsVerified: boolean;
  /** Whether the agent is registered with HAI. */
  haiRegistered: boolean;
  /** Agent's badge/trust level. */
  badgeLevel: BadgeLevel;
  /** Agent's JACS ID from the document. */
  jacsId: string;
  /** JACS version from the document. */
  version: string;
  /** Any errors encountered during verification. */
  errors: string[];
}

/** Result of verifying a signed JACS document via HAI verify endpoint. */
export interface DocumentVerificationResult {
  /** Whether verification succeeded. */
  valid: boolean;
  /** ISO 8601 verification timestamp from HAI. */
  verifiedAt: string;
  /** Document type string from verifier. */
  documentType: string;
  /** Whether issuer trust checks passed. */
  issuerVerified: boolean;
  /** Whether signature checks passed. */
  signatureVerified: boolean;
  /** Signer identifier from verifier. */
  signerId: string;
  /** ISO 8601 signed-at timestamp from verifier. */
  signedAt: string;
  /** Optional error message. */
  error?: string;
}

/** Advanced verification badge levels from /api/v1/agents/{agent_id}/verification. */
export type AdvancedBadgeLevel = 'none' | 'basic' | 'domain' | 'attested';

/** Three-level verification status from advanced verification endpoints. */
export interface AdvancedVerificationStatus {
  /** Level 1 cryptographic JACS signature verification. */
  jacsValid: boolean;
  /** Level 2 DNS/domain verification. */
  dnsValid: boolean;
  /** Level 3 HAI registration/attestation. */
  haiRegistered: boolean;
  /** Computed trust badge level. */
  badge: AdvancedBadgeLevel;
}

/** Result from GET /api/v1/agents/{agent_id}/verification and POST /api/v1/agents/verify. */
export interface AdvancedVerificationResult {
  /** Agent identifier that was verified. */
  agentId: string;
  /** Multi-level verification status. */
  verification: AdvancedVerificationStatus;
  /** Optional HAI signature summaries. */
  haiSignatures: string[];
  /** ISO 8601 verification timestamp. */
  verifiedAt: string;
  /** Errors/warnings produced during verification. */
  errors: string[];
  /** Full raw response payload. */
  rawResponse: Record<string, unknown>;
}

// =============================================================================
// API error types
// =============================================================================

/** Structured error codes returned by the HAI API. */
export type HaiErrorCode =
  | 'EMAIL_NOT_ACTIVE'
  | 'RECIPIENT_NOT_FOUND'
  | 'SUBJECT_TOO_LONG'
  | 'BODY_TOO_LARGE'
  | 'EXTERNAL_RECIPIENT'
  | 'RATE_LIMITED'
  | 'MESSAGE_NOT_FOUND'
  | 'SIGNATURE_INVALID';

/** API error response shape. */
export interface ApiErrorResponse {
  error: string;
  message: string;
  status: number;
  request_id?: string;
  error_code?: HaiErrorCode;
}

/** Request payload options for POST /api/v1/agents/verify. */
export interface VerifyAgentDocumentOnHaiOptions {
  /** Optional public key PEM if not embedded in agent_json. */
  publicKey?: string;
  /** Optional domain override for DNS verification. */
  domain?: string;
}
