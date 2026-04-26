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
  /**
   * Private key password for JACS agent decryption.
   * When provided, passed directly to the JacsAgent via setPrivateKeyPassword()
   * instead of relying on the JACS_PRIVATE_KEY_PASSWORD environment variable.
   */
  password?: string;
  /** Maximum SSE/WS reconnect attempts before giving up. Default: 10. */
  maxReconnectAttempts?: number;
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
export type BenchmarkTier = 'free' | 'pro' | 'enterprise';

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
  /** Stripe payment ID (required for pro tier). */
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
  /** Filesystem path where the agent's keys were written (set by registerNewAgent). */
  keyDirectory?: string;
  /** Path to the agent's public key PEM (set by registerNewAgent when available). */
  publicKeyPath?: string;
  /** DNS TXT record value for _jacs.<domain> (set when registerNewAgent was called with a domain). */
  dnsRecord?: string;
  rawResponse: Record<string, unknown>;
}

/** Options for key rotation. */
export interface RotateKeysOptions {
  /** Whether to re-register with HAI after local rotation. Default: true. */
  registerWithHai?: boolean;
  /** HAI server URL (required if registerWithHai is true). */
  haiUrl?: string;
}

/** Result of a key rotation operation. */
export interface RotationResult {
  /** Agent's stable JACS ID (unchanged). */
  jacsId: string;
  /** Version before rotation. */
  oldVersion: string;
  /** New version assigned during rotation. */
  newVersion: string;
  /** SHA-256 hash of the new public key (hex). */
  newPublicKeyHash: string;
  /** Whether re-registration with HAI succeeded. */
  registeredWithHai: boolean;
  /** Complete self-signed agent JSON string. */
  signedAgentJson: string;
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

/** Result of a pro tier benchmark run. Single score, no breakdown. */
export interface ProRunResult {
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

/** Result of an enterprise tier benchmark run. */
export interface EnterpriseRunResult {
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
export type BenchmarkResult = FreeChaoticResult | ProRunResult | EnterpriseRunResult;

/** @deprecated Use ProRunResult instead. */
export type DnsCertifiedResult = ProRunResult;
/** @deprecated Use EnterpriseRunResult instead. */
export type FullyCertifiedResult = EnterpriseRunResult;

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

/** Options for pro tier benchmark run. */
export interface ProRunOptions {
  /** Transport protocol. */
  transport?: ConnectionMode;
  /** Milliseconds between payment status checks. Default: 2000. */
  pollIntervalMs?: number;
  /** Max milliseconds to wait for payment. Default: 300000 (5 min). */
  pollTimeoutMs?: number;
  /** Callback with checkout URL (e.g., to open in browser). */
  onCheckoutUrl?: (url: string) => void;
}

/** @deprecated Use ProRunOptions instead. */
export type DnsCertifiedRunOptions = ProRunOptions;

/** Options for free chaotic run. */
export interface FreeChaoticRunOptions {
  /** Transport protocol. */
  transport?: ConnectionMode;
}

// =============================================================================
// Email types
// =============================================================================

/** An email attachment. */
export interface EmailAttachment {
  /** Attachment file name. */
  filename: string;
  /** MIME content type. */
  contentType: string;
  /** Raw attachment data. */
  data: Buffer;
  /** Base64-encoded attachment data (used when `data` is empty). */
  dataBase64?: string;
}

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
  attachments?: EmailAttachment[];
  /** CC recipient addresses. */
  cc?: string[];
  /** BCC recipient addresses. */
  bcc?: string[];
  /** Labels/tags for the message. */
  labels?: string[];
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
  /** CC recipient addresses. */
  ccAddresses: string[];
  /** Labels/tags on the message. */
  labels: string[];
  /** MUSUBI composite trust score (0-100). Higher = safer. Absent for outbound/unanalyzed. */
  trustScore?: number;
  /** Folder the message is in (e.g., "inbox", "archive"). */
  folder: string;
}

/** Options for listing email messages. */
export interface ListMessagesOptions {
  /** Max number of messages to return. */
  limit?: number;
  /** Offset for pagination. */
  offset?: number;
  /** Filter by direction: "inbound" or "outbound". */
  direction?: 'inbound' | 'outbound';
  /** Filter by read status (true/false/undefined for all). */
  isRead?: boolean;
  /** Filter by folder (e.g., "inbox", "archive"). */
  folder?: string;
  /** Filter by label/tag. */
  label?: string;
  /** Filter to messages with attachments. */
  hasAttachments?: boolean;
  /** Return messages since this ISO 8601 timestamp. */
  since?: string;
  /** Return messages until this ISO 8601 timestamp. */
  until?: string;
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
  /** Filter by read status. */
  isRead?: boolean;
  /** Filter by JACS verification status. */
  jacsVerified?: boolean;
  /** Filter by folder (e.g., "inbox", "archive"). */
  folder?: string;
  /** Filter by label/tag. */
  label?: string;
  /** Filter to messages with attachments. */
  hasAttachments?: boolean;
  /** Return messages since this ISO 8601 timestamp. */
  since?: string;
  /** Return messages until this ISO 8601 timestamp. */
  until?: string;
}

/**
 * Result of `getRawEmail` — raw RFC 5322 bytes for local JACS verification.
 *
 * Byte-fidelity (PRD R2): `rawEmail`, when present, is byte-identical to
 * what JACS signed. No trimming, no line-ending normalization.
 *
 * When `available` is `false`, `rawEmail` is `null` and `omittedReason`
 * explains why:
 * - `"not_stored"`: legacy row predating the feature.
 * - `"oversize"`: MIME exceeded the 25 MB storage cap.
 */
export interface RawEmailResult {
  messageId: string;
  rfcMessageId: string | null;
  available: boolean;
  rawEmail: Buffer | null;
  sizeBytes: number | null;
  omittedReason: string | null;
}

/** A contact derived from email message history. */
export interface Contact {
  /** Contact email address. */
  email: string;
  /** Display name, if known. */
  displayName?: string;
  /** ISO 8601 timestamp of last contact. */
  lastContact: string;
  /** Whether this contact's agent is JACS-verified. */
  jacsVerified: boolean;
  /** Reputation tier of this contact. */
  reputationTier?: string;
}

/** Options for forwarding an email. */
export interface ForwardOptions {
  /** ID of the message to forward. */
  messageId: string;
  /** Recipient to forward to. */
  to: string;
  /** Optional comment prepended to the forwarded body. */
  comment?: string;
}

/** Volume statistics from the email status response. */
export interface EmailVolumeInfo {
  /** Total messages sent all time. */
  sentTotal: number;
  /** Total messages received all time. */
  receivedTotal: number;
  /** Messages sent in the last 24 hours. */
  sent24h: number;
}

/** Delivery metrics from the email status response. */
export interface EmailDeliveryInfo {
  /** Number of bounced messages. */
  bounceCount: number;
  /** Number of spam reports received. */
  spamReportCount: number;
  /** Delivery success rate (0.0 to 1.0). */
  deliveryRate: number;
}

/** Reputation scoring from the email status response. */
export interface EmailReputationInfo {
  /** Overall reputation score. */
  score: number;
  /** Reputation tier string. */
  tier: string;
  /** Email-specific reputation score. */
  emailScore: number;
  /** HAI platform reputation score, or null if not yet computed. */
  haiScore: number | null;
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
  /** Volume statistics (from consolidated status). */
  volume?: EmailVolumeInfo | null;
  /** Delivery metrics (from consolidated status). */
  delivery?: EmailDeliveryInfo | null;
  /** Reputation scoring (from consolidated status). */
  reputation?: EmailReputationInfo | null;
}

// =============================================================================
// Email template types
// =============================================================================

/** An email template with reusable instructions for agent email workflows. */
export interface EmailTemplate {
  /** Unique template ID. */
  id: string;
  /** Agent ID that owns this template. */
  agentId: string;
  /** Human-readable template name. */
  name: string;
  /** Instructions for how to compose outbound emails using this template. */
  howToSend?: string;
  /** Instructions for how to respond to inbound emails matching this template. */
  howToRespond?: string;
  /** Goal or purpose this template serves. */
  goal?: string;
  /** Rules or constraints the agent must follow when using this template. */
  rules?: string;
  /** ISO 8601 creation timestamp. */
  createdAt: string;
  /** ISO 8601 last-updated timestamp. */
  updatedAt: string;
}

/** Options for creating an email template. */
export interface CreateEmailTemplateOptions {
  /** Human-readable template name (required). */
  name: string;
  /** Instructions for how to compose outbound emails. */
  howToSend?: string;
  /** Instructions for how to respond to inbound emails. */
  howToRespond?: string;
  /** Goal or purpose this template serves. */
  goal?: string;
  /** Rules or constraints the agent must follow. */
  rules?: string;
}

/** Options for updating an email template (all fields optional).
 *
 * For the four text fields (howToSend, howToRespond, goal, rules):
 * - `undefined` (or absent) — don't change the current value
 * - `null` — clear the value to NULL
 * - `string` — set to the given value
 */
export interface UpdateEmailTemplateOptions {
  /** New template name. */
  name?: string;
  /** Updated send instructions. Set to null to clear. */
  howToSend?: string | null;
  /** Updated response instructions. Set to null to clear. */
  howToRespond?: string | null;
  /** Updated goal. Set to null to clear. */
  goal?: string | null;
  /** Updated rules. Set to null to clear. */
  rules?: string | null;
}

/** Options for listing/searching email templates. */
export interface ListEmailTemplatesOptions {
  /** Max number of templates to return. */
  limit?: number;
  /** Offset for pagination. */
  offset?: number;
  /** BM25 search query string. */
  q?: string;
}

/** Result of listing email templates. */
export interface ListEmailTemplatesResult {
  /** Array of email templates. */
  templates: EmailTemplate[];
  /** Total number of matching templates. */
  total: number;
  /** Limit used in the query. */
  limit: number;
  /** Offset used in the query. */
  offset: number;
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

/** Status of a single field in JACS email content verification. */
export type FieldStatus = 'pass' | 'modified' | 'fail' | 'unverifiable';

/** Result for a single field in content verification. */
export interface FieldResult {
  /** Field name (e.g., "subject", "body", "from"). */
  field: string;
  /** Verification status for this field. */
  status: FieldStatus;
  /** Original hash from the JACS signature, if available. */
  originalHash?: string;
  /** Current hash computed from the email, if available. */
  currentHash?: string;
  /** Original value (for short fields), if available. */
  originalValue?: string;
  /** Current value (for short fields), if available. */
  currentValue?: string;
}

/** Entry in a JACS email forwarding chain. */
export interface ChainEntry {
  /** Signer identifier (e.g., email address). */
  signer: string;
  /** JACS ID of the signer. */
  jacsId: string;
  /** Whether this chain entry's signature is valid. */
  valid: boolean;
  /** Whether this entry represents a forward (vs. original). */
  forwarded: boolean;
}

/** Result of verifying a JACS attachment-signed email. */
export interface EmailVerificationResultV2 {
  /** Whether overall verification passed. */
  valid: boolean;
  /** JACS ID of the signer. */
  jacsId: string;
  /** Signature algorithm (e.g., "Ed25519"). */
  algorithm: string;
  /** Signer's reputation tier. */
  reputationTier: string;
  /** Whether DNS verification passed, or null if not checked. */
  dnsVerified?: boolean | null;
  /** Per-field verification results. */
  fieldResults: FieldResult[];
  /** Forwarding chain entries. */
  chain: ChainEntry[];
  /** Error message if verification failed, or null. */
  error?: string | null;
  /** Agent status from registry: "active", "suspended", or "revoked". */
  agentStatus?: string | null;
  /** Benchmark tiers the agent has completed. */
  benchmarksCompleted?: string[];
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

// =============================================================================
// Layer 8: Local Media (TASK_008 / JACS 0.10.0)
// =============================================================================

/** Options for `signText`. */
export interface SignTextOptions {
  /** Skip writing a `<path>.bak` backup before modifying. Default: false. */
  noBackup?: boolean;
  /** Re-add a signature even if one with this signer is already valid. */
  allowDuplicate?: boolean;
}

/** Options for `verifyText`. */
export interface VerifyTextOptions {
  /** Optional directory of `<signer_id>.public.pem` files. */
  keyDir?: string;
  /** Treat missing/malformed signature as failure (default permissive). */
  strict?: boolean;
}

/**
 * Options for `signImage`.
 *
 * NOTE: the public option key is `robust` (not `scanRobust`). The Rust
 * binding-core parser maps `robust` → JACS-internal `scan_robust`. Do not
 * leak the JACS internal name into the public API.
 */
export interface SignImageOptions {
  /** Also embed via LSB steganography (PNG/JPEG only — WebP unsupported). */
  robust?: boolean;
  /** Force a specific format (`"png" | "jpeg" | "webp"`); default auto-detect. */
  format?: string;
  /** Refuse to overwrite an existing JACS signature in the input. */
  refuseOverwrite?: boolean;
  /**
   * Skip the `<out>.bak` write. Default `false` (i.e., backup IS taken when
   * out_path overwrites an existing file). Mirrors `signText`'s `noBackup`
   * toggle and Go's `SignImageOptions.NoBackup` (Issue 003 / Issue 009 —
   * cross-language parity).
   */
  noBackup?: boolean;
  /**
   * Override the default `0o600` backup file permission. Set only when
   * integrating with tooling that needs a broader mode (default unset →
   * 0o600).
   */
  unsafeBakMode?: number;
}

/** Options for `verifyImage`. */
export interface VerifyImageOptions {
  /** Optional directory of `<signer_id>.public.pem` files. */
  keyDir?: string;
  /** Treat missing signature as failure (default permissive). */
  strict?: boolean;
  /** Scan the LSB channel if the metadata channel is absent. */
  robust?: boolean;
}

/** Result of `signText`. */
export interface SignTextResult {
  path: string;
  signersAdded: number;
  backupPath?: string;
}

/** Per-block signature inside a `VerifyTextResult`. */
export interface VerifyTextSignature {
  signerId: string;
  algorithm: string;
  timestamp: string;
  /** One of `"valid" | "invalid_signature" | "hash_mismatch" | "key_not_found" | "unsupported_algorithm" | "malformed"`. */
  status: string;
}

/** Result of `verifyText`. */
export interface VerifyTextResult {
  /** One of `"signed" | "missing_signature" | "malformed"`. */
  status: string;
  signatures: VerifyTextSignature[];
  malformedDetail?: string;
}

/** Result of `signImage`. */
export interface SignImageResult {
  outPath: string;
  signerId: string;
  format: string;
  robust: boolean;
  backupPath?: string;
}

/** Result of `verifyImage`. */
export interface VerifyImageResult {
  /** One of `"valid" | "invalid_signature" | "hash_mismatch" | "missing_signature" | "key_not_found" | "unsupported_format" | "malformed"`. */
  status: string;
  signerId?: string;
  algorithm?: string;
  format?: string;
  embeddingChannels?: string;
  /** Decoder error string when `status === "malformed"`; otherwise undefined. */
  malformedDetail?: string;
}

/** Options for `extractMediaSignature`. */
export interface ExtractMediaSignatureOptions {
  /** Return raw base64url-no-pad bytes instead of decoded JSON. Default: false. */
  rawPayload?: boolean;
}

/** Result of `extractMediaSignature`. */
export interface ExtractMediaSignatureResult {
  present: boolean;
  /** Decoded JSON string by default; base64url bytes when `rawPayload=true`. */
  payload?: string;
}
