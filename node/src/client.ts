import type {
  HaiClientOptions,
  AgentConfig,
  HaiEvent,
  BenchmarkJob,
  HelloWorldResult,
  RegistrationResult,
  RotateKeysOptions,
  RotationResult,
  FreeChaoticResult,
  ProRunResult,
  DnsCertifiedResult,
  JobResponseResult,
  VerifyAgentResult,
  RegistrationEntry,
  CheckUsernameResult,
  ClaimUsernameResult,
  UpdateUsernameResult,
  DeleteUsernameResult,
  TranscriptMessage,
  ConnectionMode,
  ConnectOptions,
  OnBenchmarkJobOptions,
  ProRunOptions,
  DnsCertifiedRunOptions,
  FreeChaoticRunOptions,
  SendEmailOptions,
  SendEmailResult,
  EmailMessage,
  ListMessagesOptions,
  SearchOptions,
  EmailStatus,
  Contact,
  ForwardOptions,
  EmailTemplate,
  CreateEmailTemplateOptions,
  UpdateEmailTemplateOptions,
  ListEmailTemplatesOptions,
  ListEmailTemplatesResult,
  PublicKeyInfo,
  VerificationResult,
  DocumentVerificationResult,
  AdvancedVerificationResult,
  VerifyAgentDocumentOnHaiOptions,
  EmailVerificationResultV2,
  FieldStatus,
} from './types.js';
import {
  HaiError,
  AuthenticationError,
  HaiConnectionError,
} from './errors.js';
import { signResponse, canonicalJson, getServerKeys } from './signing.js';
import { loadConfig } from './config.js';
import { JacsAgent } from '@hai.ai/jacs';
// SSE/WS helpers retained in sse.ts and ws.ts for cleanup in Task 012.
// Streaming now uses FFI handles (connectSse/connectWs).
import { FFIClientAdapter } from './ffi-client.js';

/**
 * HAI platform client.
 *
 * Zero-config: `new HaiClient()` auto-discovers jacs.config.json.
 * All HTTP calls and streaming (SSE/WS) delegate to the Rust binding-core via FFI (haiinpm).
 *
 * @example
 * ```typescript
 * const hai = await HaiClient.create();
 * const result = await hai.hello();
 * console.log(result.message);
 * ```
 */

/** Default HAI API base URL. Override with the `url` option or `HAI_URL` env var. */
export const DEFAULT_BASE_URL = 'https://beta.hai.ai';

export class HaiClient {
  private config!: AgentConfig;
  private configPath: string | null = null;
  /** JACS native agent for local cryptographic operations (signing, verification). */
  private agent!: JacsAgent;
  /** FFI adapter that delegates all HTTP calls to the Rust binding-core. Lazily initialized. */
  private _ffi: FFIClientAdapter | null = null;
  private baseUrl: string;
  private timeout: number;
  private maxRetries: number;
  private maxReconnectAttempts: number;
  private _shouldDisconnect = false;
  private _connected = false;
  private _wsConnection: unknown = null;
  private _lastEventId: string | null = null;
  private serverPublicKeys: Record<string, string> = {};
  /** HAI-assigned agent UUID, set after register(). Used for email URL paths. */
  private _haiAgentId: string | null = null;
  /** Agent's @hai.ai email address, set after claimUsername(). */
  private agentEmail?: string;
  /** Agent key cache: maps cache key -> { value, cachedAt (ms since epoch) }. */
  private keyCache = new Map<string, { value: PublicKeyInfo; cachedAt: number }>();
  /** Agent key cache TTL in milliseconds (5 minutes). */
  private static readonly KEY_CACHE_TTL = 300_000;

  /** Lazy FFI adapter getter -- initializes on first use. */
  private get ffi(): FFIClientAdapter {
    if (!this._ffi) {
      this._ffi = new FFIClientAdapter(this.buildFFIConfigJson());
    }
    return this._ffi;
  }

  /**
   * Inject a pre-built FFI adapter (useful for testing).
   * @internal
   */
  _setFFIAdapter(adapter: FFIClientAdapter): void {
    this._ffi = adapter;
  }

  private constructor(options?: HaiClientOptions) {
    const rawUrl = options?.url ?? DEFAULT_BASE_URL;
    if (!/^https?:\/\//i.test(rawUrl)) {
      throw new HaiError(
        `Invalid base URL: "${rawUrl}". URL must start with http:// or https://.`,
      );
    }
    this.baseUrl = rawUrl.replace(/\/+$/, '');
    this.timeout = options?.timeout ?? 30000;
    this.maxRetries = options?.maxRetries ?? 3;
    this.maxReconnectAttempts = options?.maxReconnectAttempts ?? 10;
  }

  /**
   * Build the FFI config JSON from the client's options and loaded config.
   * This JSON is passed to the FFIClientAdapter (and ultimately to the
   * Rust HaiClient constructor).
   */
  private buildFFIConfigJson(): string {
    const ffiConfig: Record<string, unknown> = {
      url: this.baseUrl,
      timeout_ms: this.timeout,
      max_retries: this.maxRetries,
    };
    if (this.configPath) {
      ffiConfig.config_path = this.configPath;
    }
    if (this.config?.jacsId) {
      ffiConfig.jacs_id = this.config.jacsId;
    }
    return JSON.stringify(ffiConfig);
  }

  /**
   * Create a HaiClient by loading JACS agent config.
   *
   * This is the primary constructor. Uses zero-config discovery:
   * 1. options.configPath
   * 2. JACS_CONFIG_PATH env var
   * 3. ./jacs.config.json
   */
  static async create(options?: HaiClientOptions): Promise<HaiClient> {
    const client = new HaiClient(options);
    client.config = await loadConfig(options?.configPath);

    const configPath = options?.configPath
      ?? process.env.JACS_CONFIG_PATH
      ?? './jacs.config.json';
    const { resolve } = await import('node:path');
    const resolvedConfigPath = resolve(configPath);

    client.agent = new JacsAgent();
    if (options?.password != null) {
      client.agent.setPrivateKeyPassword(options.password);
    }
    await client.agent.load(resolvedConfigPath);
    client.configPath = resolvedConfigPath;

    return client;
  }

  /**
   * Create a HaiClient directly from a JACS ID and PEM-encoded private key.
   * Useful for testing or programmatic setup without config files.
   *
   * Creates a temporary JACS-shaped workspace for compatibility with
   * config-based flows.
   */
  static async fromCredentials(
    jacsId: string,
    privateKeyPem: string,
    options?: Omit<HaiClientOptions, 'configPath'> & {
      privateKeyPassphrase?: string;
      /** Key algorithm. Defaults to 'ring-Ed25519'. Set to 'RSA-PSS' for RSA keys. */
      algorithm?: 'ring-Ed25519' | 'RSA-PSS';
    },
  ): Promise<HaiClient> {
    const { mkdir, mkdtemp, writeFile } = await import('node:fs/promises');
    const { join } = await import('node:path');
    const { tmpdir } = await import('node:os');

    const client = new HaiClient(options);

    // Determine algorithm from explicit option or default to Ed25519.
    // No node:crypto key parsing is used -- all signing delegates to JACS via FFI.
    const algorithm = options?.algorithm ?? 'ring-Ed25519';

    // Use a JACS ephemeral agent for signing. The ephemeral agent generates
    // its own key pair in memory -- the user-provided privateKeyPem is stored
    // in the workspace for reference (e.g., exportKeys) but signing uses the
    // JACS-managed ephemeral key. This avoids any node:crypto usage.
    const tempDir = await mkdtemp(join(tmpdir(), 'haiai-creds-'));
    const keyDir = join(tempDir, 'keys');
    const dataDir = join(tempDir, 'data');
    await mkdir(keyDir, { recursive: true });
    await mkdir(dataDir, { recursive: true });

    // Store the user-provided private key for reference.
    await writeFile(join(keyDir, 'agent_private_key.pem'), `${privateKeyPem.trim()}\n`, { mode: 0o600 });

    // Write a config file for loadConfig() compatibility.
    const configPath = join(tempDir, 'jacs.config.json');
    const configJson = {
      jacsAgentName: jacsId,
      jacsAgentVersion: '1.0.0',
      jacsKeyDir: keyDir,
      jacsId,
      jacs_data_directory: dataDir,
      jacs_key_directory: keyDir,
      jacs_agent_private_key_filename: 'agent_private_key.pem',
      jacs_agent_public_key_filename: 'agent_public_key.pem',
      jacs_agent_key_algorithm: algorithm,
      jacs_default_storage: 'fs',
    };
    await writeFile(join(keyDir, 'agent_public_key.pem'), '', { mode: 0o644 });
    await writeFile(configPath, JSON.stringify(configJson, null, 2) + '\n');

    client.config = await loadConfig(configPath);
    client.configPath = configPath;

    // Create an ephemeral JACS agent for signing -- this generates a key pair
    // in memory and can sign immediately without loading from disk.
    client.agent = new JacsAgent();
    client.agent.ephemeralSync(algorithm);

    // Write the ephemeral agent's public key so exportKeys() works.
    const pubKey = client.agent.getPublicKeyPem();
    await writeFile(join(keyDir, 'agent_public_key.pem'), pubKey, { mode: 0o644 });

    return client;
  }

  /** The agent's JACS ID. */
  get jacsId(): string {
    return this.config.jacsId ?? this.config.jacsAgentName;
  }

  /** The agent name from config. */
  get agentName(): string {
    return this.config.jacsAgentName;
  }

  /** The HAI-assigned agent UUID (set after register()). Falls back to jacsId. */
  get haiAgentId(): string {
    return this._haiAgentId ?? this.jacsId;
  }

  /** Whether the client is currently connected to an event stream. */
  get isConnected(): boolean {
    return this._connected;
  }

  /** Get the agent's @hai.ai email address (set after claimUsername). */
  getAgentEmail(): string | undefined {
    return this.agentEmail;
  }

  /** Set the agent's @hai.ai email address manually. */
  setAgentEmail(email: string): void {
    this.agentEmail = email;
  }

  // ---------------------------------------------------------------------------
  // Agent key cache
  // ---------------------------------------------------------------------------

  /** Get a cached key if it exists and hasn't expired. */
  private getCachedKey(cacheKey: string): PublicKeyInfo | undefined {
    const entry = this.keyCache.get(cacheKey);
    if (!entry) return undefined;
    if (Date.now() - entry.cachedAt >= HaiClient.KEY_CACHE_TTL) {
      this.keyCache.delete(cacheKey);
      return undefined;
    }
    return entry.value;
  }

  /** Store a key in the cache with the current timestamp. */
  private setCachedKey(cacheKey: string, value: PublicKeyInfo): void {
    this.keyCache.set(cacheKey, { value, cachedAt: Date.now() });
  }

  /** Clear the agent key cache, forcing subsequent fetches to hit the API. */
  clearAgentKeyCache(): void {
    this.keyCache.clear();
  }

  // ---------------------------------------------------------------------------
  // Auth / signing helpers (local, no HTTP)
  // ---------------------------------------------------------------------------

  /** Sign a UTF-8 message with the agent's private key via JACS. Returns base64. */
  signMessage(message: string): string {
    return this.agent.signStringSync(message);
  }

  /** Build the JACS Authorization header value string. */
  buildAuthHeader(): string {
    // Prefer JACS binding delegation
    if ('buildAuthHeaderSync' in this.agent && typeof (this.agent as unknown as Record<string, unknown>).buildAuthHeaderSync === 'function') {
      return (this.agent as unknown as Record<string, unknown> & { buildAuthHeaderSync: () => string }).buildAuthHeaderSync();
    }
    // Fallback: local construction using JACS signStringSync
    const timestamp = Math.floor(Date.now() / 1000).toString();
    const message = `${this.jacsId}:${timestamp}`;
    const signature = this.agent.signStringSync(message);
    return `JACS ${this.jacsId}:${timestamp}:${signature}`;
  }

  // ---------------------------------------------------------------------------
  // hello()
  // ---------------------------------------------------------------------------

  /**
   * Perform a hello world exchange with HAI.
   *
   * Sends a JACS-signed request to the HAI hello endpoint. HAI responds
   * with a signed ACK containing the caller's IP and a timestamp.
   *
   * @param includeTest - If true, request a test scenario preview
   * @returns HelloWorldResult with HAI's signed acknowledgment
   */
  async hello(includeTest: boolean = false): Promise<HelloWorldResult> {
    const data = await this.ffi.hello(includeTest);

    // Verify HAI's signature on the ACK
    let haiSigValid = false;
    const haiSignedAck = data.hai_signed_ack as string | undefined;
    if (haiSignedAck) {
      const fingerprint = (data.hai_public_key_fingerprint as string) || '';
      const serverKey = this.serverPublicKeys[fingerprint];
      if (serverKey) {
        haiSigValid = this.verifyHaiMessage(
          JSON.stringify(data),
          haiSignedAck,
          serverKey,
        );
      }
    }

    return {
      success: true,
      timestamp: (data.timestamp as string) || '',
      clientIp: (data.client_ip as string) || '',
      haiPublicKeyFingerprint: (data.hai_public_key_fingerprint as string) || '',
      message: (data.message as string) || '',
      haiSignedAck: (data.hai_signed_ack as string) || '',
      helloId: (data.hello_id as string) || '',
      testScenario: data.test_scenario,
      haiSignatureValid: haiSigValid,
      rawResponse: data,
    };
  }

  // ---------------------------------------------------------------------------
  // verifyHaiMessage()
  // ---------------------------------------------------------------------------

  /**
   * Verify a message signed by HAI via JACS.
   *
   * @param message - The message string that was signed
   * @param signature - The signature to verify (base64-encoded)
   * @param haiPublicKey - HAI's public key (PEM)
   * @returns true if signature is valid
   */
  verifyHaiMessage(message: string, signature: string, haiPublicKey: string = ''): boolean {
    if (!signature || !message) return false;
    if (!haiPublicKey) return false;
    try {
      return this.agent.verifyStringSync(
        message,
        signature,
        Buffer.from(haiPublicKey, 'utf-8'),
        'pem',
      );
    } catch {
      return false;
    }
  }

  // ---------------------------------------------------------------------------
  // register()
  // ---------------------------------------------------------------------------

  /**
   * Register this agent with HAI.
   *
   * Generates a JACS agent document with the agent's public key and
   * POSTs to the registration endpoint.
   */
  async register(options?: {
    ownerEmail?: string;
    description?: string;
    domain?: string;
    agentJson?: string;
    publicKeyPem?: string;
  }): Promise<RegistrationResult> {
    const registerOptions: Record<string, unknown> = {};
    if (options?.ownerEmail) registerOptions.owner_email = options.ownerEmail;
    if (options?.description) registerOptions.description = options.description;
    if (options?.domain) registerOptions.domain = options.domain;
    if (options?.agentJson) registerOptions.agent_json = options.agentJson;
    if (options?.publicKeyPem) registerOptions.public_key_pem = options.publicKeyPem;

    const data = await this.ffi.register(registerOptions);

    // After successful registration, store the HAI-assigned agent_id (UUID).
    const assignedAgentId = (data.agent_id as string) || (data.agentId as string) || '';
    if (assignedAgentId) {
      this._haiAgentId = assignedAgentId;
    }

    return {
      success: true,
      agentId: assignedAgentId,
      jacsId: (data.jacs_id as string) || (data.jacsId as string) || this.jacsId,
      haiSignature: (data.hai_signature as string) || (data.haiSignature as string) || '',
      registrationId: (data.registration_id as string) || (data.registrationId as string) || '',
      registeredAt: (data.registered_at as string) || (data.registeredAt as string) || '',
      rawResponse: data,
    };
  }

  // ---------------------------------------------------------------------------
  // rotateKeys()
  // ---------------------------------------------------------------------------

  /**
   * Rotate the agent's cryptographic keys.
   *
   * Delegates to the FFI adapter which handles key generation, archival,
   * agent document construction, and optional HAI re-registration.
   */
  async rotateKeys(options?: RotateKeysOptions): Promise<RotationResult> {
    const rotateOptions: Record<string, unknown> = {};
    if (options?.registerWithHai != null) rotateOptions.register_with_hai = options.registerWithHai;
    if (options?.haiUrl) rotateOptions.hai_url = options.haiUrl;

    const data = await this.ffi.rotateKeys(rotateOptions);

    // Update in-memory config with new version
    const newVersion = (data.new_version as string) || '';
    if (newVersion) {
      this.config = {
        ...this.config,
        jacsAgentVersion: newVersion,
      };
    }

    return {
      jacsId: (data.jacs_id as string) || this.jacsId,
      oldVersion: (data.old_version as string) || '',
      newVersion,
      newPublicKeyHash: (data.new_public_key_hash as string) || '',
      registeredWithHai: (data.registered_with_hai as boolean) ?? false,
      signedAgentJson: (data.signed_agent_json as string) || '',
    };
  }

  // ---------------------------------------------------------------------------
  // verify()
  // ---------------------------------------------------------------------------

  /** Verify the agent's registration status. */
  async verify(): Promise<VerifyAgentResult> {
    const data = await this.ffi.verifyStatus();

    const rawRegistrations = (data.registrations as Array<Record<string, unknown>>) || [];
    const registrations: RegistrationEntry[] = rawRegistrations.map((r) => ({
      keyId: (r.key_id as string) || '',
      algorithm: (r.algorithm as string) || '',
      signatureJson: (r.signature_json as string) || '',
      signedAt: (r.signed_at as string) || '',
    }));

    return {
      jacsId: (data.jacs_id as string) || this.jacsId,
      registered: (data.registered as boolean) ?? false,
      registrations,
      dnsVerified: (data.dns_verified as boolean) ?? false,
      registeredAt: (data.registered_at as string) || '',
      rawResponse: data,
    };
  }

  /** @deprecated Use verify() instead. */
  async status(): Promise<VerifyAgentResult> {
    return this.verify();
  }

  // ---------------------------------------------------------------------------
  // freeChaoticRun()
  // ---------------------------------------------------------------------------

  /**
   * Run a free chaotic benchmark.
   *
   * No scoring, returns raw transcript with structural annotations.
   * Rate limited to 3 runs per JACS keypair per 24 hours.
   */
  async freeChaoticRun(options?: FreeChaoticRunOptions): Promise<FreeChaoticResult> {
    const data = await this.ffi.freeRun(options?.transport);

    return {
      success: true,
      runId: (data.run_id as string) || (data.runId as string) || '',
      transcript: this.parseTranscript((data.transcript as unknown[]) || []),
      upsellMessage: (data.upsell_message as string) || (data.upsellMessage as string) || '',
      rawResponse: data,
    };
  }

  // ---------------------------------------------------------------------------
  // proRun()
  // ---------------------------------------------------------------------------

  /**
   * Run a pro tier benchmark ($20/month).
   *
   * Flow: create Stripe checkout -> poll for payment -> run benchmark.
   */
  async proRun(options?: ProRunOptions): Promise<ProRunResult> {
    const proOptions: Record<string, unknown> = {};
    if (options?.transport) proOptions.transport = options.transport;
    if (options?.pollIntervalMs != null) proOptions.poll_interval_ms = options.pollIntervalMs;
    if (options?.pollTimeoutMs != null) proOptions.poll_timeout_ms = options.pollTimeoutMs;

    // Note: onCheckoutUrl callback needs special handling -- FFI cannot invoke
    // JS callbacks. For now we delegate fully to FFI which blocks until payment
    // completes. If the caller needs the checkout URL, they should use the
    // lower-level API.
    const data = await this.ffi.proRun(proOptions);

    return {
      success: true,
      runId: (data.run_id as string) || (data.runId as string) || '',
      score: Number(data.score) || 0,
      transcript: this.parseTranscript((data.transcript as unknown[]) || []),
      paymentId: (data.payment_id as string) || '',
      rawResponse: data,
    };
  }

  /** @deprecated Use proRun instead. The tier was renamed from dns_certified to pro. */
  async dnsCertifiedRun(options?: DnsCertifiedRunOptions): Promise<DnsCertifiedResult> {
    return this.proRun(options);
  }

  // ---------------------------------------------------------------------------
  // enterpriseRun()
  // ---------------------------------------------------------------------------

  /**
   * Run an enterprise tier benchmark.
   *
   * The enterprise tier is coming soon.
   * Contact support@hai.ai for early access.
   */
  async enterpriseRun(_options?: Record<string, unknown>): Promise<never> {
    throw new Error(
      'The enterprise tier is coming soon. ' +
      'Contact support@hai.ai for early access.'
    );
  }

  /** @deprecated Use enterpriseRun instead. The tier was renamed from fully_certified to enterprise. */
  async certifiedRun(_options?: Record<string, unknown>): Promise<never> {
    return this.enterpriseRun(_options);
  }

  // ---------------------------------------------------------------------------
  // submitResponse()
  // ---------------------------------------------------------------------------

  /**
   * Submit a mediation response for a benchmark job.
   *
   * @param jobId - The job/run ID from the benchmark_job event
   * @param message - The mediator's response message
   * @param options - Optional metadata and processingTimeMs
   */
  async submitResponse(
    jobId: string,
    message: string,
    options?: {
      metadata?: Record<string, unknown>;
      processingTimeMs?: number;
    },
  ): Promise<JobResponseResult> {
    const params: Record<string, unknown> = {
      job_id: jobId,
      message,
      metadata: options?.metadata ?? null,
      processing_time_ms: options?.processingTimeMs ?? 0,
    };

    const data = await this.ffi.submitResponse(params);

    return {
      success: (data.success as boolean) ?? true,
      jobId: (data.job_id as string) || (data.jobId as string) || jobId,
      message: (data.message as string) || 'Response accepted',
      rawResponse: data,
    };
  }

  // ---------------------------------------------------------------------------
  // connect() -- SSE/WS streaming (stays native)
  // TODO(DRY_FFI_PHASE2): migrate to FFI streaming
  // ---------------------------------------------------------------------------

  /**
   * Connect to HAI event stream via SSE or WebSocket.
   *
   * Returns an async generator that yields HaiEvent objects.
   * Supports automatic reconnection with exponential backoff.
   */
  async *connect(options?: ConnectOptions): AsyncGenerator<HaiEvent> {
    const transport = options?.transport ?? 'sse';
    const onEvent = options?.onEvent;

    this._shouldDisconnect = false;
    this._connected = false;

    if (transport === 'ws') {
      yield* this.connectWs(onEvent);
    } else {
      yield* this.connectSse(onEvent);
    }
  }

  /**
   * Disconnect from the event stream (SSE or WebSocket).
   * Safe to call even if not connected.
   */
  disconnect(): void {
    this._shouldDisconnect = true;

    if (this._wsConnection) {
      try {
        (this._wsConnection as { close(): void }).close();
      } catch { /* ignore */ }
      this._wsConnection = null;
    }

    this._connected = false;
  }

  // ---------------------------------------------------------------------------
  // onBenchmarkJob()
  // ---------------------------------------------------------------------------

  /**
   * Convenience wrapper: connect and dispatch benchmark_job events.
   *
   * Runs until disconnect() is called.
   */
  async onBenchmarkJob(
    handler: (job: BenchmarkJob) => Promise<void>,
    options?: OnBenchmarkJobOptions,
  ): Promise<void> {
    for await (const event of this.connect({ transport: options?.transport })) {
      if (event.eventType === 'benchmark_job') {
        const data = (typeof event.data === 'object' && event.data !== null)
          ? event.data as Record<string, unknown>
          : {};

        const job: BenchmarkJob = {
          runId: (data.run_id as string) || (data.runId as string) || '',
          scenario: data.scenario ?? data.prompt ?? data,
          data,
        };

        await handler(job);
      }
    }
  }

  // ---------------------------------------------------------------------------
  // checkUsername()
  // ---------------------------------------------------------------------------

  /**
   * Check if a username is available for claiming.
   * This is a public endpoint and does not require authentication.
   *
   * @param username - The username to check
   * @returns Availability result
   */
  async checkUsername(username: string): Promise<CheckUsernameResult> {
    const data = await this.ffi.checkUsername(username);

    return {
      available: (data.available as boolean) ?? false,
      username: (data.username as string) || username,
      reason: (data.reason as string) || undefined,
    };
  }

  // ---------------------------------------------------------------------------
  // claimUsername()
  // ---------------------------------------------------------------------------

  /**
   * Claim a username for an agent. Requires JACS auth.
   *
   * @param agentId - The JACS ID of the agent to claim the username for
   * @param username - The username to claim
   * @returns Claim result with the assigned email
   */
  async claimUsername(agentId: string, username: string): Promise<ClaimUsernameResult> {
    const data = await this.ffi.claimUsername(agentId, username);

    this.agentEmail = (data.email as string) || '';

    return {
      username: (data.username as string) || username,
      email: (data.email as string) || '',
      agentId: (data.agent_id as string) || (data.agentId as string) || agentId,
    };
  }

  /**
   * Rename a claimed username for an agent. Requires JACS auth.
   *
   * @param agentId - The agent ID to update
   * @param username - The new username
   */
  async updateUsername(agentId: string, username: string): Promise<UpdateUsernameResult> {
    const data = await this.ffi.updateUsername(agentId, username);

    return {
      username: (data.username as string) || username,
      email: (data.email as string) || '',
      previousUsername: (data.previous_username as string) || '',
    };
  }

  /**
   * Delete a claimed username for an agent. Requires JACS auth.
   *
   * @param agentId - The agent ID to update
   */
  async deleteUsername(agentId: string): Promise<DeleteUsernameResult> {
    const data = await this.ffi.deleteUsername(agentId);

    return {
      releasedUsername: (data.released_username as string) || '',
      cooldownUntil: (data.cooldown_until as string) || '',
      message: (data.message as string) || '',
    };
  }

  // ---------------------------------------------------------------------------
  // verifyDocument()
  // ---------------------------------------------------------------------------

  /**
   * Verify a signed JACS document via HAI's public verification endpoint.
   * This endpoint is public and does not require authentication.
   *
   * @param document - Signed JACS document JSON (object or string)
   */
  async verifyDocument(document: Record<string, unknown> | string): Promise<DocumentVerificationResult> {
    const rawDocument = typeof document === 'string' ? document : JSON.stringify(document);
    const data = await this.ffi.verifyDocument(rawDocument);

    return {
      valid: (data.valid as boolean) ?? false,
      verifiedAt: (data.verified_at as string) || '',
      documentType: (data.document_type as string) || '',
      issuerVerified: (data.issuer_verified as boolean) ?? false,
      signatureVerified: (data.signature_verified as boolean) ?? false,
      signerId: (data.signer_id as string) || '',
      signedAt: (data.signed_at as string) || '',
      error: (data.error as string) || undefined,
    };
  }

  private parseAdvancedVerificationResult(
    data: Record<string, unknown>,
    fallbackAgentId: string = '',
  ): AdvancedVerificationResult {
    const verification = (data.verification as Record<string, unknown>) || {};
    return {
      agentId: (data.agent_id as string) || fallbackAgentId,
      verification: {
        jacsValid: (verification.jacs_valid as boolean) ?? false,
        dnsValid: (verification.dns_valid as boolean) ?? false,
        haiRegistered: (verification.hai_registered as boolean) ?? false,
        badge: (verification.badge as 'none' | 'basic' | 'domain' | 'attested') || 'none',
      },
      haiSignatures: ((data.hai_signatures as unknown[]) || []).map(String),
      verifiedAt: (data.verified_at as string) || '',
      errors: ((data.errors as unknown[]) || []).map(String),
      rawResponse: data,
    };
  }

  /**
   * Get advanced 3-level verification status for an agent (public endpoint).
   *
   * GET /api/v1/agents/{agent_id}/verification
   */
  async getVerification(agentId: string): Promise<AdvancedVerificationResult> {
    const data = await this.ffi.getVerification(agentId);
    return this.parseAdvancedVerificationResult(data, agentId);
  }

  /**
   * Verify an agent document via HAI's advanced verification endpoint (public).
   *
   * POST /api/v1/agents/verify
   */
  async verifyAgentDocumentOnHai(
    agentJson: Record<string, unknown> | string,
    options?: VerifyAgentDocumentOnHaiOptions,
  ): Promise<AdvancedVerificationResult> {
    const requestJson = JSON.stringify({
      agent_json: typeof agentJson === 'string' ? agentJson : JSON.stringify(agentJson),
      public_key: options?.publicKey,
      domain: options?.domain,
    });
    const data = await this.ffi.verifyAgentDocument(requestJson);
    return this.parseAdvancedVerificationResult(data);
  }

  // ---------------------------------------------------------------------------
  // registerNewAgent()
  // ---------------------------------------------------------------------------

  /**
   * Generate a fresh JACS agent and register it with HAI.
   *
   * Convenience method that combines key generation, document building,
   * signing, and registration in one call.
   *
   * @param agentName - Name for the new agent
   * @param options - Registration options
   * @returns Registration result
   */
  async registerNewAgent(agentName: string, options: {
    ownerEmail: string;
    domain?: string;
    description?: string;
    quiet?: boolean;
  }): Promise<RegistrationResult> {
    // Delegate to FFI register with the full set of options
    const registerOptions: Record<string, unknown> = {
      agent_name: agentName,
      owner_email: options.ownerEmail,
      new_agent: true,
    };
    if (options.domain) registerOptions.domain = options.domain;
    if (options.description) registerOptions.description = options.description;

    const data = await this.ffi.register(registerOptions);

    if (!options.quiet) {
      const agentId = (data.agent_id as string) || (data.agentId as string) || '';
      console.log(`\nAgent created and submitted for registration!`);
      console.log(`  -> Check your email (${options.ownerEmail}) for a verification link`);
      console.log(`  -> Click the link and log into hai.ai to complete registration`);
      console.log(`  -> After verification, claim a @hai.ai username with:`);
      console.log(`     client.claimUsername('${agentId}', 'my-agent')`);
      console.log(`  -> Save your config and private key to a secure, access-controlled location`);

      if (options.domain) {
        console.log(`\n--- DNS Setup Instructions ---`);
        console.log(`Add this TXT record to your domain '${options.domain}':`);
        console.log(`  Name:  _jacs.${options.domain}`);
        console.log(`  Type:  TXT`);
        console.log(`  Value: sha256:<your_public_key_hash>`);
        console.log(`DNS verification enables the pro tier.\n`);
      } else {
        console.log();
      }
    }

    return {
      success: true,
      agentId: (data.agent_id as string) || (data.agentId as string) || '',
      jacsId: (data.jacs_id as string) || (data.jacsId as string) || '',
      haiSignature: (data.hai_signature as string) || (data.haiSignature as string) || '',
      registrationId: (data.registration_id as string) || (data.registrationId as string) || '',
      registeredAt: (data.registered_at as string) || (data.registeredAt as string) || '',
      rawResponse: data,
    };
  }

  // ---------------------------------------------------------------------------
  // testConnection()
  // ---------------------------------------------------------------------------

  /**
   * Test connectivity to the HAI server.
   *
   * Tries multiple health endpoints and returns true if any respond with 2xx.
   * Does not require authentication.
   */
  async testConnection(): Promise<boolean> {
    // Use the FFI-backed hello() as a single authenticated health check.
    try {
      await this.ffi.hello(false);
      return true;
    } catch {
      return false;
    }
  }

  // ---------------------------------------------------------------------------
  // Utility: export keys (local, no HTTP)
  // ---------------------------------------------------------------------------

  /**
   * Export the agent's public key.
   * Reads the public key from the JACS key directory.
   * Returns { publicKeyPem }.
   */
  exportKeys(): { publicKeyPem: string; privateKeyPem?: string } {
    const fs = require('node:fs');
    const path = require('node:path');
    const keyDir = this.config.jacsKeyDir;

    const candidates = [
      path.join(keyDir, 'agent_public_key.pem'),
      path.join(keyDir, `${this.config.jacsAgentName}.public.pem`),
      path.join(keyDir, 'public_key.pem'),
      path.join(keyDir, 'jacs.public.pem'),
    ];

    for (const candidate of candidates) {
      try {
        const content = fs.readFileSync(candidate);
        const raw = content as Buffer;
        const text = raw.toString('utf-8').trim();
        let publicKeyPem: string;
        if (text.includes('BEGIN PUBLIC KEY') || text.includes('BEGIN RSA PUBLIC KEY')) {
          publicKeyPem = text;
        } else {
          const base64 = raw.toString('base64');
          const lines = base64.match(/.{1,64}/g) ?? [];
          publicKeyPem = `-----BEGIN PUBLIC KEY-----\n${lines.join('\n')}\n-----END PUBLIC KEY-----`;
        }
        return { publicKeyPem };
      } catch {
        // try next
      }
    }

    throw new AuthenticationError(
      `No public key found. Searched: ${candidates.join(', ')}`,
    );
  }

  // ---------------------------------------------------------------------------
  // SSE transport (via FFI opaque handles)
  // ---------------------------------------------------------------------------

  private async *connectSse(
    onEvent?: (event: HaiEvent) => void,
  ): AsyncGenerator<HaiEvent> {
    let attempt = 0;

    while (!this._shouldDisconnect) {
      let handle: number | null = null;
      try {
        handle = await this.ffi.connectSse();
        this._connected = true;
        attempt = 0;

        while (!this._shouldDisconnect) {
          const eventData = await this.ffi.sseNextEvent(handle);
          if (eventData === null) break; // Connection closed

          const event: HaiEvent = {
            eventType: (eventData as Record<string, unknown>).event_type as string || '',
            data: (eventData as Record<string, unknown>).data || {},
            id: (eventData as Record<string, unknown>).id as string | undefined,
            raw: (eventData as Record<string, unknown>).raw as string || '',
          };

          if (event.id) this._lastEventId = event.id;
          if (onEvent) onEvent(event);
          yield event;
        }
      } catch (err) {
        if (err instanceof HaiError && (err as HaiError & { statusCode?: number }).statusCode === 401) throw err;
        if (attempt >= this.maxReconnectAttempts) throw err;
        const delay = Math.min(1000 * Math.pow(2, attempt), 60000);
        await new Promise(r => setTimeout(r, delay));
        attempt++;
      } finally {
        this._connected = false;
        if (handle !== null) {
          try { await this.ffi.sseClose(handle); } catch { /* ignore */ }
        }
      }
    }
  }

  // ---------------------------------------------------------------------------
  // WebSocket transport (via FFI opaque handles)
  // ---------------------------------------------------------------------------

  private async *connectWs(
    onEvent?: (event: HaiEvent) => void,
  ): AsyncGenerator<HaiEvent> {
    let attempt = 0;

    while (!this._shouldDisconnect) {
      let handle: number | null = null;
      try {
        handle = await this.ffi.connectWs();
        this._connected = true;
        this._wsConnection = handle;
        attempt = 0;

        while (!this._shouldDisconnect) {
          const eventData = await this.ffi.wsNextEvent(handle);
          if (eventData === null) break; // Connection closed

          const event: HaiEvent = {
            eventType: (eventData as Record<string, unknown>).event_type as string || '',
            data: (eventData as Record<string, unknown>).data || {},
            id: (eventData as Record<string, unknown>).id as string | undefined,
            raw: (eventData as Record<string, unknown>).raw as string || '',
          };

          if (event.id) this._lastEventId = event.id;
          if (onEvent) onEvent(event);
          yield event;
        }
      } catch (err) {
        if (err instanceof HaiError && (err as HaiError & { statusCode?: number }).statusCode === 401) throw err;
        if (attempt >= this.maxReconnectAttempts) throw err;
        const delay = Math.min(1000 * Math.pow(2, attempt), 60000);
        await new Promise(r => setTimeout(r, delay));
        attempt++;
      } finally {
        this._connected = false;
        this._wsConnection = null;
        if (handle !== null) {
          try { await this.ffi.wsClose(handle); } catch { /* ignore */ }
        }
      }
    }
  }

  // ---------------------------------------------------------------------------
  // Transcript parsing
  // ---------------------------------------------------------------------------

  private parseEmailMessage(m: Record<string, unknown>): EmailMessage {
    return {
      id: (m.id as string) || '',
      direction: (m.direction as string) || '',
      fromAddress: (m.from_address as string) || '',
      toAddress: (m.to_address as string) || '',
      subject: (m.subject as string) || '',
      bodyText: (m.body_text as string) || '',
      messageId: (m.message_id as string) || '',
      inReplyTo: (m.in_reply_to as string | null) ?? null,
      isRead: (m.is_read as boolean) ?? false,
      deliveryStatus: (m.delivery_status as string) || '',
      createdAt: (m.created_at as string) || '',
      readAt: (m.read_at as string | null) ?? null,
      jacsVerified: (m.jacs_verified as boolean) ?? false,
      ccAddresses: (m.cc_addresses as string[]) || [],
      labels: (m.labels as string[]) || [],
      trustScore: (m.trust_score as number) ?? undefined,
      folder: (m.folder as string) || 'inbox',
    };
  }

  private parseTranscript(raw: unknown[]): TranscriptMessage[] {
    return (raw || []).map((msg: unknown) => {
      const m = msg as Record<string, unknown>;
      return {
        role: (m.role as string) || 'system',
        content: (m.content as string) || '',
        timestamp: (m.timestamp as string) || '',
        annotations: (m.annotations as string[]) || [],
      };
    });
  }

  // ---------------------------------------------------------------------------
  // Server key management
  // ---------------------------------------------------------------------------

  /** Fetch and cache server public keys for signature verification. */
  async fetchServerKeys(): Promise<void> {
    this.serverPublicKeys = await getServerKeys(this.baseUrl, this.ffi);
  }

  // ---------------------------------------------------------------------------
  // getAgentAttestation()
  // ---------------------------------------------------------------------------

  /**
   * Get attestation information for another agent.
   *
   * @param agentId - The JACS ID of the agent to query
   * @returns Attestation status including HAI signatures
   */
  async getAgentAttestation(agentId: string): Promise<VerifyAgentResult> {
    const data = await this.ffi.verifyStatus(agentId);

    const rawRegistrations = (data.registrations as Array<Record<string, unknown>>) || [];
    const registrations: RegistrationEntry[] = rawRegistrations.map((r) => ({
      keyId: (r.key_id as string) || '',
      algorithm: (r.algorithm as string) || '',
      signatureJson: (r.signature_json as string) || '',
      signedAt: (r.signed_at as string) || '',
    }));

    return {
      jacsId: (data.jacs_id as string) || agentId,
      registered: (data.registered as boolean) ?? false,
      registrations,
      dnsVerified: (data.dns_verified as boolean) ?? false,
      registeredAt: (data.registered_at as string) || '',
      rawResponse: data,
    };
  }

  // ---------------------------------------------------------------------------
  // signBenchmarkResult()
  // ---------------------------------------------------------------------------

  /**
   * Sign a benchmark result as a JACS document for independent verification.
   *
   * @param benchmarkResult - The benchmark result data to sign
   * @returns Signed JACS document envelope
   */
  signBenchmarkResult(benchmarkResult: Record<string, unknown>): { signed_document: string; agent_jacs_id: string } {
    return signResponse(
      benchmarkResult,
      this.agent,
      this.jacsId,
      this.agent,
    );
  }

  // ---------------------------------------------------------------------------
  // benchmark() -- legacy suite-based
  // ---------------------------------------------------------------------------

  /**
   * Run a benchmark with specified name and tier.
   *
   * @param name - Benchmark run name
   * @param tier - Benchmark tier ("free", "pro", "enterprise"). Default: "free"
   * @returns Benchmark result with scores
   */
  async benchmark(name: string = 'mediation_basic', tier: string = 'free'): Promise<Record<string, unknown>> {
    const data = await this.ffi.benchmark(name, tier);
    return data;
  }

  // ---------------------------------------------------------------------------
  // Email CRUD
  // ---------------------------------------------------------------------------

  /**
   * Send an email from the agent's @hai.ai address.
   *
   * @param options - Email send options (to, subject, body, optional inReplyTo)
   * @returns Send result with message ID and status
   */
  async sendEmail(options: SendEmailOptions): Promise<SendEmailResult> {
    if (!this.agentEmail) {
      throw new Error('agent email not set — call claimUsername first');
    }

    const emailOptions: Record<string, unknown> = {
      to: options.to,
      subject: options.subject,
      body: options.body,
    };
    if (options.inReplyTo) emailOptions.in_reply_to = options.inReplyTo;
    if (options.attachments?.length) {
      emailOptions.attachments = options.attachments.map(a => ({
        filename: a.filename,
        content_type: a.contentType,
        data_base64: a.data.toString('base64'),
      }));
    }
    if (options.cc?.length) emailOptions.cc = options.cc;
    if (options.bcc?.length) emailOptions.bcc = options.bcc;
    if (options.labels?.length) emailOptions.labels = options.labels;

    const data = await this.ffi.sendEmail(emailOptions);

    return {
      messageId: (data.message_id as string) || '',
      status: (data.status as string) || '',
    };
  }

  /**
   * Sign a raw RFC 5322 email with a JACS attachment via the HAI API.
   *
   * The server adds a `jacs-signature.json` MIME attachment containing
   * the detached JACS signature. The returned Buffer is the signed email.
   *
   * @param rawEmail - Raw RFC 5322 email as a Buffer or string.
   * @returns Signed email bytes with the JACS attachment added.
   */
  async signEmail(rawEmail: Buffer | string): Promise<Buffer> {
    const emailData = typeof rawEmail === 'string' ? rawEmail : rawEmail.toString('base64');
    const data = await this.ffi.sendSignedEmail({ raw_email_base64: emailData });
    // FFI returns { signed_email_base64: string }
    const signedB64 = (data.signed_email_base64 as string) || '';
    return Buffer.from(signedB64, 'base64');
  }

  /**
   * Send an agent-signed email.
   *
   * @deprecated sendSignedEmail currently delegates to sendEmail. Use sendEmail directly.
   */
  async sendSignedEmail(options: SendEmailOptions): Promise<SendEmailResult> {
    return this.sendEmail(options);
  }

  /**
   * Verify a JACS-signed email via the HAI API.
   *
   * @param rawEmail - Raw RFC 5322 email as a Buffer or string.
   * @returns EmailVerificationResultV2 with field-level verification results.
   */
  async verifyEmail(rawEmail: Buffer | string): Promise<EmailVerificationResultV2> {
    const docStr = typeof rawEmail === 'string' ? rawEmail : rawEmail.toString('utf-8');
    const data = await this.ffi.verifyDocument(docStr);

    return {
      valid: (data.valid as boolean) ?? false,
      jacsId: (data.jacs_id as string) ?? '',
      algorithm: (data.algorithm as string) ?? '',
      reputationTier: (data.reputation_tier as string) ?? '',
      dnsVerified: data.dns_verified as boolean | null | undefined,
      fieldResults: ((data.field_results as Array<Record<string, unknown>>) ?? []).map(fr => ({
        field: (fr.field as string) ?? '',
        status: (fr.status as FieldStatus) ?? 'unverifiable',
        originalHash: fr.original_hash as string | undefined,
        currentHash: fr.current_hash as string | undefined,
        originalValue: fr.original_value as string | undefined,
        currentValue: fr.current_value as string | undefined,
      })),
      chain: ((data.chain as Array<Record<string, unknown>>) ?? []).map(ce => ({
        signer: (ce.signer as string) ?? '',
        jacsId: (ce.jacs_id as string) ?? '',
        valid: (ce.valid as boolean) ?? false,
        forwarded: (ce.forwarded as boolean) ?? false,
      })),
      error: data.error as string | null | undefined,
      agentStatus: data.agent_status as string | null | undefined,
      benchmarksCompleted: (data.benchmarks_completed as string[]) ?? [],
    };
  }

  /**
   * List email messages for this agent.
   *
   * @param options - Pagination and direction filter options
   * @returns Array of email messages
   */
  async listMessages(options?: ListMessagesOptions): Promise<EmailMessage[]> {
    const listOptions: Record<string, unknown> = {};
    if (options?.limit != null) listOptions.limit = options.limit;
    if (options?.offset != null) listOptions.offset = options.offset;
    if (options?.direction) listOptions.direction = options.direction;
    if (options?.isRead != null) listOptions.is_read = options.isRead;
    if (options?.folder) listOptions.folder = options.folder;
    if (options?.label) listOptions.label = options.label;
    if (options?.hasAttachments != null) listOptions.has_attachments = options.hasAttachments;
    if (options?.since) listOptions.since = options.since;
    if (options?.until) listOptions.until = options.until;

    const rawMessages = await this.ffi.listMessages(listOptions);
    return (rawMessages as Array<Record<string, unknown>>).map((m) => this.parseEmailMessage(m));
  }

  /**
   * Mark an email message as read.
   *
   * @param messageId - The message ID to mark as read
   */
  async markRead(messageId: string): Promise<void> {
    await this.ffi.markRead(messageId);
  }

  /**
   * Get email rate limit and status info for this agent.
   *
   * @returns Email status with daily limits and usage
   */
  async getEmailStatus(): Promise<EmailStatus> {
    const data = await this.ffi.getEmailStatus();
    const volumeRaw = data.volume as Record<string, unknown> | undefined;
    const deliveryRaw = data.delivery as Record<string, unknown> | undefined;
    const reputationRaw = data.reputation as Record<string, unknown> | undefined;

    return {
      email: (data.email as string) || '',
      status: (data.status as string) || '',
      tier: (data.tier as string) || '',
      billingTier: (data.billing_tier as string) || '',
      messagesSent24h: (data.messages_sent_24h as number) || 0,
      dailyLimit: (data.daily_limit as number) || 0,
      dailyUsed: (data.daily_used as number) || 0,
      resetsAt: (data.resets_at as string) || '',
      messagesSentTotal: (data.messages_sent_total as number) || 0,
      externalEnabled: (data.external_enabled as boolean) || false,
      externalSendsToday: (data.external_sends_today as number) || 0,
      lastTierChange: (data.last_tier_change as string) || null,
      volume: volumeRaw ? {
        sentTotal: (volumeRaw.sent_total as number) || 0,
        receivedTotal: (volumeRaw.received_total as number) || 0,
        sent24h: (volumeRaw.sent_24h as number) || 0,
      } : null,
      delivery: deliveryRaw ? {
        bounceCount: (deliveryRaw.bounce_count as number) || 0,
        spamReportCount: (deliveryRaw.spam_report_count as number) || 0,
        deliveryRate: (deliveryRaw.delivery_rate as number) || 0,
      } : null,
      reputation: reputationRaw ? {
        score: (reputationRaw.score as number) || 0,
        tier: (reputationRaw.tier as string) || '',
        emailScore: (reputationRaw.email_score as number) || 0,
        haiScore: reputationRaw.hai_score != null ? (reputationRaw.hai_score as number) : null,
      } : null,
    };
  }

  /**
   * Get a single email message by ID.
   *
   * @param messageId - The message ID to retrieve
   * @returns The email message
   */
  async getMessage(messageId: string): Promise<EmailMessage> {
    const m = await this.ffi.getMessage(messageId);
    return this.parseEmailMessage(m);
  }

  /**
   * Delete an email message.
   *
   * @param messageId - The message ID to delete
   */
  async deleteMessage(messageId: string): Promise<void> {
    await this.ffi.deleteMessage(messageId);
  }

  /**
   * Mark an email message as unread.
   *
   * @param messageId - The message ID to mark as unread
   */
  async markUnread(messageId: string): Promise<void> {
    await this.ffi.markUnread(messageId);
  }

  /**
   * Search email messages.
   *
   * @param options - Search query and pagination options
   * @returns Array of matching email messages
   */
  async searchMessages(options: SearchOptions): Promise<EmailMessage[]> {
    const searchOptions: Record<string, unknown> = {
      query: options.query,
    };
    if (options.limit != null) searchOptions.limit = options.limit;
    if (options.offset != null) searchOptions.offset = options.offset;
    if (options.direction) searchOptions.direction = options.direction;
    if (options.fromAddress) searchOptions.from_address = options.fromAddress;
    if (options.toAddress) searchOptions.to_address = options.toAddress;
    if (options.isRead != null) searchOptions.is_read = options.isRead;
    if (options.jacsVerified != null) searchOptions.jacs_verified = options.jacsVerified;
    if (options.folder) searchOptions.folder = options.folder;
    if (options.label) searchOptions.label = options.label;
    if (options.hasAttachments != null) searchOptions.has_attachments = options.hasAttachments;
    if (options.since) searchOptions.since = options.since;
    if (options.until) searchOptions.until = options.until;

    const rawMessages = await this.ffi.searchMessages(searchOptions);
    return (rawMessages as Array<Record<string, unknown>>).map((m) => this.parseEmailMessage(m));
  }

  /**
   * Get the count of unread messages.
   *
   * @returns The number of unread messages
   */
  async getUnreadCount(): Promise<number> {
    return this.ffi.getUnreadCount();
  }

  /**
   * Reply to an email message.
   *
   * Convenience method that fetches the original message to get the sender
   * and subject, then sends a reply with proper threading.
   *
   * @param messageId - The message ID to reply to
   * @param body - Reply body text
   * @param subjectOverride - Optional subject override (defaults to "Re: <original subject>")
   * @returns Send result with message ID and status
   */
  async reply(messageId: string, body: string, subjectOverride?: string): Promise<SendEmailResult> {
    const original = await this.getMessage(messageId);
    const subject = subjectOverride ?? (original.subject?.startsWith('Re: ') ? original.subject : `Re: ${original.subject}`);
    return this.sendEmail({
      to: original.fromAddress,
      subject,
      body,
      inReplyTo: original.messageId ?? messageId,
    });
  }

  /**
   * Forward an email message to another recipient.
   *
   * @param options - Forward options (messageId, to, optional comment)
   * @returns Send result with message ID and status
   */
  async forward(options: ForwardOptions): Promise<SendEmailResult> {
    const params: Record<string, unknown> = {
      message_id: options.messageId,
      to: options.to,
    };
    if (options.comment) params.comment = options.comment;

    const data = await this.ffi.forward(params);

    return {
      messageId: (data.message_id as string) || '',
      status: (data.status as string) || '',
    };
  }

  /**
   * Archive an email message.
   *
   * @param messageId - The message ID to archive
   */
  async archive(messageId: string): Promise<void> {
    await this.ffi.archive(messageId);
  }

  /**
   * Unarchive (restore) an email message.
   *
   * @param messageId - The message ID to unarchive
   */
  async unarchive(messageId: string): Promise<void> {
    await this.ffi.unarchive(messageId);
  }

  /**
   * List contacts derived from email message history.
   *
   * @returns Array of Contact objects
   */
  async getContacts(): Promise<Contact[]> {
    const items = await this.ffi.contacts();
    return (items as Array<Record<string, unknown>>).map((c) => ({
      email: (c.email as string) || '',
      displayName: (c.display_name as string) || undefined,
      lastContact: (c.last_contact as string) || '',
      jacsVerified: (c.jacs_verified as boolean) ?? false,
      reputationTier: (c.reputation_tier as string) || undefined,
    }));
  }

  // ---------------------------------------------------------------------------
  // Attestations
  // ---------------------------------------------------------------------------

  /**
   * Create a new attestation for an agent.
   *
   * @param agentId - The agent ID to create the attestation for
   * @param subject - The subject of the attestation
   * @param claims - Array of claims to attest
   * @param evidence - Optional array of supporting evidence
   * @returns The created attestation
   */
  async createAttestation(agentId: string, subject: object, claims: object[], evidence?: object[]): Promise<object> {
    const params = JSON.stringify({
      agent_id: agentId,
      subject,
      claims,
      evidence: evidence || [],
    });
    const raw = await this.ffi.createAttestation(params);
    return JSON.parse(raw);
  }

  /**
   * List attestations for an agent.
   *
   * @param agentId - The agent ID to list attestations for
   * @param limit - Maximum number of results (default: 20)
   * @param offset - Pagination offset (default: 0)
   * @returns Paginated list of attestations
   */
  async listAttestations(agentId: string, limit: number = 20, offset: number = 0): Promise<object> {
    const params = JSON.stringify({
      agent_id: agentId,
      limit,
      offset,
    });
    const raw = await this.ffi.listAttestations(params);
    return JSON.parse(raw);
  }

  /**
   * Get a specific attestation by ID.
   *
   * @param agentId - The agent ID that owns the attestation
   * @param docId - The attestation document ID
   * @returns The attestation document
   */
  async getAttestation(agentId: string, docId: string): Promise<object> {
    const raw = await this.ffi.getAttestation(agentId, docId);
    return JSON.parse(raw);
  }

  /**
   * Verify an attestation document's signatures and integrity.
   *
   * @param document - The attestation document as a JSON string
   * @returns Verification result
   */
  async verifyAttestation(document: string): Promise<object> {
    const raw = await this.ffi.verifyAttestation(document);
    return JSON.parse(raw);
  }

  // ---------------------------------------------------------------------------
  // Email Templates
  // ---------------------------------------------------------------------------

  /**
   * Create an email template for this agent.
   *
   * @param options - Template fields (name is required)
   * @returns The created email template
   */
  async createEmailTemplate(options: CreateEmailTemplateOptions): Promise<EmailTemplate> {
    const payload: Record<string, unknown> = { name: options.name };
    if (options.howToSend != null) payload.how_to_send = options.howToSend;
    if (options.howToRespond != null) payload.how_to_respond = options.howToRespond;
    if (options.goal != null) payload.goal = options.goal;
    if (options.rules != null) payload.rules = options.rules;
    const raw = await this.ffi.createEmailTemplate(JSON.stringify(payload));
    const data = JSON.parse(raw);
    return this.parseEmailTemplate(data);
  }

  /**
   * List or search email templates for this agent.
   *
   * When `options.q` is provided, performs BM25 full-text search across all
   * template fields.
   *
   * @param options - Pagination and optional search query
   * @returns List of templates with total count
   */
  async listEmailTemplates(options?: ListEmailTemplatesOptions): Promise<ListEmailTemplatesResult> {
    const payload: Record<string, unknown> = {};
    if (options?.limit != null) payload.limit = options.limit;
    if (options?.offset != null) payload.offset = options.offset;
    if (options?.q) payload.q = options.q;
    const raw = await this.ffi.listEmailTemplates(JSON.stringify(payload));
    const data = JSON.parse(raw) as Record<string, unknown>;
    const rawTemplates = (data.templates as Array<Record<string, unknown>>) || [];
    return {
      templates: rawTemplates.map((t) => this.parseEmailTemplate(t)),
      total: (data.total as number) || 0,
      limit: (data.limit as number) || 0,
      offset: (data.offset as number) || 0,
    };
  }

  /**
   * Get a single email template by ID.
   *
   * @param templateId - The template ID to retrieve
   * @returns The email template
   */
  async getEmailTemplate(templateId: string): Promise<EmailTemplate> {
    const raw = await this.ffi.getEmailTemplate(templateId);
    const data = JSON.parse(raw) as Record<string, unknown>;
    return this.parseEmailTemplate(data);
  }

  /**
   * Update an email template. Only provided fields are changed.
   *
   * @param templateId - The template ID to update
   * @param options - Fields to update (all optional)
   * @returns The updated email template
   */
  async updateEmailTemplate(templateId: string, options: UpdateEmailTemplateOptions): Promise<EmailTemplate> {
    const payload: Record<string, unknown> = {};
    if (options.name !== undefined) payload.name = options.name;
    if (options.howToSend !== undefined) payload.how_to_send = options.howToSend;
    if (options.howToRespond !== undefined) payload.how_to_respond = options.howToRespond;
    if (options.goal !== undefined) payload.goal = options.goal;
    if (options.rules !== undefined) payload.rules = options.rules;
    const raw = await this.ffi.updateEmailTemplate(templateId, JSON.stringify(payload));
    const data = JSON.parse(raw) as Record<string, unknown>;
    return this.parseEmailTemplate(data);
  }

  /**
   * Delete an email template (soft delete).
   *
   * @param templateId - The template ID to delete
   */
  async deleteEmailTemplate(templateId: string): Promise<void> {
    await this.ffi.deleteEmailTemplate(templateId);
  }

  private parseEmailTemplate(data: Record<string, unknown>): EmailTemplate {
    return {
      id: (data.id as string) || '',
      agentId: (data.agent_id as string) || '',
      name: (data.name as string) || '',
      howToSend: (data.how_to_send as string) || undefined,
      howToRespond: (data.how_to_respond as string) || undefined,
      goal: (data.goal as string) || undefined,
      rules: (data.rules as string) || undefined,
      createdAt: (data.created_at as string) || '',
      updatedAt: (data.updated_at as string) || '',
    };
  }

  // ---------------------------------------------------------------------------
  // fetchRemoteKey()
  // ---------------------------------------------------------------------------

  /**
   * Look up another agent's public key from the HAI key directory.
   *
   * @param jacsId - The JACS ID of the agent to look up
   * @param version - Key version (default: "latest")
   * @returns Public key information
   */
  async fetchRemoteKey(jacsId: string, version: string = 'latest'): Promise<PublicKeyInfo> {
    const cacheKey = `remote:${jacsId}:${version}`;
    const cached = this.getCachedKey(cacheKey);
    if (cached) return cached;

    const data = await this.ffi.fetchRemoteKey(jacsId, version);
    const result = this.parsePublicKeyInfo(data);
    this.setCachedKey(cacheKey, result);
    return result;
  }

  // ---------------------------------------------------------------------------
  // fetchKeyByHash()
  // ---------------------------------------------------------------------------

  /**
   * Look up an agent's public key by its SHA-256 hash.
   *
   * @param publicKeyHash - Hash in `sha256:<hex>` format
   * @returns Public key information
   */
  async fetchKeyByHash(publicKeyHash: string): Promise<PublicKeyInfo> {
    const cacheKey = `hash:${publicKeyHash}`;
    const cached = this.getCachedKey(cacheKey);
    if (cached) return cached;

    const data = await this.ffi.fetchKeyByHash(publicKeyHash);
    const result = this.parsePublicKeyInfo(data);
    this.setCachedKey(cacheKey, result);
    return result;
  }

  // ---------------------------------------------------------------------------
  // fetchKeyByEmail()
  // ---------------------------------------------------------------------------

  /**
   * Look up an agent's public key by their @hai.ai email address.
   *
   * @param email - The agent's email address (e.g., "alice@hai.ai")
   * @returns Public key information
   */
  async fetchKeyByEmail(email: string): Promise<PublicKeyInfo> {
    const cacheKey = `email:${email}`;
    const cached = this.getCachedKey(cacheKey);
    if (cached) return cached;

    const data = await this.ffi.fetchKeyByEmail(email);
    const result = this.parsePublicKeyInfo(data);
    this.setCachedKey(cacheKey, result);
    return result;
  }

  // ---------------------------------------------------------------------------
  // fetchKeyByDomain()
  // ---------------------------------------------------------------------------

  /**
   * Look up the latest DNS-verified agent key for a domain.
   *
   * @param domain - DNS domain (e.g., "example.com")
   * @returns Public key information
   */
  async fetchKeyByDomain(domain: string): Promise<PublicKeyInfo> {
    const cacheKey = `domain:${domain}`;
    const cached = this.getCachedKey(cacheKey);
    if (cached) return cached;

    const data = await this.ffi.fetchKeyByDomain(domain);
    const result = this.parsePublicKeyInfo(data);
    this.setCachedKey(cacheKey, result);
    return result;
  }

  // ---------------------------------------------------------------------------
  // fetchAllKeys()
  // ---------------------------------------------------------------------------

  /**
   * Fetch all key versions for an agent, ordered by creation date descending.
   *
   * @param jacsId - The JACS ID of the agent to look up
   * @returns Object with jacs_id, keys array, and total count
   */
  async fetchAllKeys(jacsId: string): Promise<{ jacsId: string; keys: PublicKeyInfo[]; total: number }> {
    const data = await this.ffi.fetchAllKeys(jacsId);
    const rawKeys = (data.keys as Array<Record<string, unknown>>) || [];
    const keys = rawKeys.map((k) => this.parsePublicKeyInfo(k));

    return {
      jacsId: (data.jacs_id as string) || '',
      keys,
      total: (data.total as number) || 0,
    };
  }

  // ---------------------------------------------------------------------------
  // verifyAgent()
  // ---------------------------------------------------------------------------

  /**
   * Verify another agent's JACS document.
   *
   * Performs three levels of verification:
   * 1. Local Ed25519 signature verification
   * 2. DNS verification (via server attestation)
   * 3. HAI registration attestation
   *
   * @param agentDocument - JACS agent document (object or JSON string)
   * @returns Verification result with signature validity and trust level
   */
  async verifyAgent(agentDocument: Record<string, unknown> | string): Promise<VerificationResult> {
    const doc = typeof agentDocument === 'string'
      ? JSON.parse(agentDocument) as Record<string, unknown>
      : agentDocument;

    const result: VerificationResult = {
      signatureValid: false,
      dnsVerified: false,
      haiRegistered: false,
      badgeLevel: 'none',
      jacsId: (doc.jacsId as string) || '',
      version: (doc.jacsVersion as string) || '',
      errors: [],
    };

    // Level 1: JACS signature verification (local, via JACS agent)
    try {
      const publicKeyPem = doc.jacsPublicKey as string | undefined;
      if (!publicKeyPem) {
        result.errors.push('No jacsPublicKey in document');
        return result;
      }

      const sig = doc.jacsSignature as Record<string, unknown> | undefined;
      const signature = sig?.signature as string | undefined;
      if (!signature) {
        result.errors.push('No signature in jacsSignature');
        return result;
      }

      // Remove signature, canonicalize, verify via JACS
      const verifyDoc = JSON.parse(JSON.stringify(doc)) as Record<string, unknown>;
      delete (verifyDoc.jacsSignature as Record<string, unknown>).signature;
      const canonical = canonicalJson(verifyDoc);

      result.signatureValid = this.agent.verifyStringSync(
        canonical,
        signature,
        Buffer.from(publicKeyPem, 'utf-8'),
        'pem',
      );
    } catch (e) {
      result.errors.push(`Signature verification failed: ${(e as Error).message}`);
    }

    // Level 3: Server attestation via FFI
    try {
      const docJacsId = String(doc.jacsId || '');
      const attestData = await this.ffi.verifyStatus(docJacsId);
      result.haiRegistered = (attestData.registered as boolean) ?? false;
      result.dnsVerified = (attestData.dns_verified as boolean) ?? false;
      result.badgeLevel = (attestData.badge_level as VerificationResult['badgeLevel']) || 'none';
    } catch (e) {
      result.errors.push(`Server attestation check failed: ${(e as Error).message}`);
    }

    return result;
  }

  // ---------------------------------------------------------------------------
  // Private helpers
  // ---------------------------------------------------------------------------

  /** Parse a raw key info response into a PublicKeyInfo. */
  private parsePublicKeyInfo(data: Record<string, unknown>): PublicKeyInfo {
    return {
      jacsId: (data.jacs_id as string) || '',
      version: (data.version as string) || '',
      publicKey: (data.public_key as string) || '',
      publicKeyRawB64: (data.public_key_raw_b64 as string) || '',
      algorithm: (data.algorithm as string) || '',
      publicKeyHash: (data.public_key_hash as string) || '',
      status: (data.status as string) || '',
      dnsVerified: (data.dns_verified as boolean) ?? false,
      createdAt: (data.created_at as string) || '',
    };
  }
}
