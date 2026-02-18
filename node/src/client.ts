import type {
  HaiClientOptions,
  AgentConfig,
  HaiEvent,
  BenchmarkJob,
  HelloWorldResult,
  RegistrationResult,
  FreeChaoticResult,
  BaselineResult,
  CertifiedResult,
  JobResponseResult,
  StatusResult,
  TranscriptMessage,
  ConnectionMode,
  ConnectOptions,
  OnBenchmarkJobOptions,
  BaselineRunOptions,
  FreeChaoticRunOptions,
  JobResponse,
} from './types.js';
import {
  HaiError,
  AuthenticationError,
  HaiConnectionError,
  WebSocketError,
} from './errors.js';
import { signString, verifyString, generateKeypair } from './crypt.js';
import { signResponse, canonicalJson, getServerKeys, unwrapSignedEvent } from './signing.js';
import { loadConfig, loadPrivateKey } from './config.js';
import { parseSseStream } from './sse.js';
import { openWebSocket, wsRecv, wsEventStream } from './ws.js';

/**
 * HAI platform client.
 *
 * Zero-config: `new HaiClient()` auto-discovers jacs.config.json.
 * All authentication uses JACS-signed headers (no API keys).
 *
 * @example
 * ```typescript
 * const hai = await HaiClient.create();
 * const result = await hai.hello();
 * console.log(result.message);
 * ```
 */
export class HaiClient {
  private config!: AgentConfig;
  private privateKeyPem!: string;
  private baseUrl: string;
  private timeout: number;
  private maxRetries: number;
  private _shouldDisconnect = false;
  private _connected = false;
  private _wsConnection: unknown = null;
  private _lastEventId: string | null = null;
  private serverPublicKeys: Record<string, string> = {};

  private constructor(options?: HaiClientOptions) {
    this.baseUrl = (options?.url ?? 'https://hai.ai').replace(/\/+$/, '');
    this.timeout = options?.timeout ?? 30000;
    this.maxRetries = options?.maxRetries ?? 3;
  }

  /**
   * Create a HaiClient by loading config and private key.
   *
   * This is the primary constructor. Uses zero-config discovery:
   * 1. options.configPath
   * 2. JACS_CONFIG_PATH env var
   * 3. ./jacs.config.json
   */
  static async create(options?: HaiClientOptions): Promise<HaiClient> {
    const client = new HaiClient(options);
    client.config = await loadConfig(options?.configPath);
    client.privateKeyPem = await loadPrivateKey(client.config);
    return client;
  }

  /**
   * Create a HaiClient directly from a JACS ID and PEM-encoded private key.
   * Useful for testing or programmatic setup without config files.
   */
  static fromCredentials(
    jacsId: string,
    privateKeyPem: string,
    options?: Omit<HaiClientOptions, 'configPath'>,
  ): HaiClient {
    const client = new HaiClient(options);
    client.config = {
      jacsAgentName: jacsId,
      jacsAgentVersion: '1.0.0',
      jacsKeyDir: '.',
      jacsId,
    };
    client.privateKeyPem = privateKeyPem;
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

  /** Whether the client is currently connected to an event stream. */
  get isConnected(): boolean {
    return this._connected;
  }

  // ---------------------------------------------------------------------------
  // Auth helpers
  // ---------------------------------------------------------------------------

  /**
   * Build JACS Authorization header.
   * Format: `JACS {jacsId}:{timestamp}:{signature_base64}`
   */
  private buildAuthHeaders(): Record<string, string> {
    const timestamp = Math.floor(Date.now() / 1000).toString();
    const message = `${this.jacsId}:${timestamp}`;
    const signature = signString(this.privateKeyPem, message);
    return {
      'Authorization': `JACS ${this.jacsId}:${timestamp}:${signature}`,
      'Content-Type': 'application/json',
    };
  }

  /** Sign a UTF-8 message with the agent's private key. Returns base64. */
  signMessage(message: string): string {
    return signString(this.privateKeyPem, message);
  }

  /** Build the JACS Authorization header value string. */
  buildAuthHeader(): string {
    const timestamp = Math.floor(Date.now() / 1000).toString();
    const message = `${this.jacsId}:${timestamp}`;
    const signature = signString(this.privateKeyPem, message);
    return `JACS ${this.jacsId}:${timestamp}:${signature}`;
  }

  private makeUrl(path: string): string {
    const cleanPath = path.startsWith('/') ? path : `/${path}`;
    return `${this.baseUrl}${cleanPath}`;
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
    const url = this.makeUrl('/api/v1/agents/hello');
    const payload: Record<string, unknown> = { agent_id: this.jacsId };
    if (includeTest) {
      payload.include_test = true;
    }

    const response = await this.fetchWithRetry(url, {
      method: 'POST',
      headers: this.buildAuthHeaders(),
      body: JSON.stringify(payload),
    });

    const data = await response.json() as Record<string, unknown>;

    // Verify HAI's signature on the ACK
    let haiSigValid = false;
    const haiAckSignature = data.hai_ack_signature as string | undefined;
    if (haiAckSignature) {
      haiSigValid = this.verifyHaiMessage(
        JSON.stringify(data),
        haiAckSignature,
        (data.hai_public_key as string) || '',
      );
    }

    return {
      success: true,
      timestamp: (data.timestamp as string) || '',
      clientIp: (data.client_ip as string) || '',
      haiPublicKeyFingerprint: (data.hai_public_key_fingerprint as string) || '',
      message: (data.message as string) || '',
      haiSignatureValid: haiSigValid,
      rawResponse: data,
    };
  }

  // ---------------------------------------------------------------------------
  // verifyHaiMessage()
  // ---------------------------------------------------------------------------

  /**
   * Verify a message signed by HAI.
   *
   * @param message - The message string that was signed
   * @param signature - The signature to verify (base64-encoded)
   * @param haiPublicKey - HAI's public key (PEM or base64)
   * @returns true if signature is valid
   */
  verifyHaiMessage(message: string, signature: string, haiPublicKey: string = ''): boolean {
    if (!signature || !message) return false;
    if (!haiPublicKey) return false;
    return verifyString(haiPublicKey, message, signature);
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
  async register(): Promise<RegistrationResult> {
    const { publicKeyPem } = this.exportKeys();

    // Build JACS agent document
    const agentDoc: Record<string, unknown> = {
      jacsId: this.jacsId,
      jacsVersion: '1.0.0',
      jacsSignature: {
        agentID: this.jacsId,
        date: new Date().toISOString(),
      },
      jacsPublicKey: publicKeyPem,
      name: this.config.jacsAgentName,
      capabilities: ['mediation'],
      version: this.config.jacsAgentVersion,
    };

    // Sign canonical JSON
    const canonical = JSON.stringify(agentDoc, Object.keys(agentDoc).sort());
    const signature = signString(this.privateKeyPem, canonical);
    (agentDoc.jacsSignature as Record<string, string>).signature = signature;

    const url = this.makeUrl('/api/v1/agents/register');

    const response = await this.fetchWithRetry(url, {
      method: 'POST',
      headers: this.buildAuthHeaders(),
      body: JSON.stringify({
        agent_json: JSON.stringify(agentDoc),
        public_key: publicKeyPem,
      }),
    });

    const data = await response.json() as Record<string, unknown>;

    return {
      success: true,
      agentId: (data.agent_id as string) || (data.agentId as string) || '',
      haiSignature: (data.hai_signature as string) || (data.haiSignature as string) || '',
      registrationId: (data.registration_id as string) || (data.registrationId as string) || '',
      registeredAt: (data.registered_at as string) || (data.registeredAt as string) || '',
      rawResponse: data,
    };
  }

  // ---------------------------------------------------------------------------
  // status()
  // ---------------------------------------------------------------------------

  /** Check the agent's registration status. */
  async status(): Promise<StatusResult> {
    const url = this.makeUrl(`/api/v1/agents/${this.jacsId}/status`);

    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: this.buildAuthHeaders(),
    });

    const data = await response.json() as Record<string, unknown>;

    return {
      registered: (data.registered as boolean) ?? (data.active as boolean) ?? false,
      agentId: (data.agent_id as string) || (data.agentId as string) || this.jacsId,
      registrationId: (data.registration_id as string) || (data.registrationId as string) || '',
      registeredAt: (data.registered_at as string) || (data.registeredAt as string) || '',
      haiSignatures: (data.hai_signatures as string[]) || (data.haiSignatures as string[]) || [],
      benchmarkCount: Number(data.benchmark_count ?? data.benchmarkCount ?? 0),
      rawResponse: data,
    };
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
    const url = this.makeUrl('/api/benchmark/run');
    const payload = {
      name: `Free Chaotic Run - ${this.jacsId.slice(0, 8)}`,
      tier: 'free_chaotic',
      transport: options?.transport ?? 'sse',
    };

    const response = await this.fetchWithRetry(url, {
      method: 'POST',
      headers: this.buildAuthHeaders(),
      body: JSON.stringify(payload),
    }, Math.max(this.timeout, 120000));

    const data = await response.json() as Record<string, unknown>;

    return {
      success: true,
      runId: (data.run_id as string) || (data.runId as string) || '',
      transcript: this.parseTranscript((data.transcript as unknown[]) || []),
      upsellMessage: (data.upsell_message as string) || (data.upsellMessage as string) || '',
      rawResponse: data,
    };
  }

  // ---------------------------------------------------------------------------
  // baselineRun()
  // ---------------------------------------------------------------------------

  /**
   * Run a $5 baseline benchmark.
   *
   * Flow: create Stripe checkout -> poll for payment -> run benchmark.
   */
  async baselineRun(options?: BaselineRunOptions): Promise<BaselineResult> {
    const pollIntervalMs = options?.pollIntervalMs ?? 2000;
    const pollTimeoutMs = options?.pollTimeoutMs ?? 300000;

    // Step 1: Create Stripe Checkout session
    const purchaseUrl = this.makeUrl('/api/benchmark/purchase');
    const purchasePayload = { tier: 'baseline', agent_id: this.jacsId };

    const purchaseResp = await this.fetchWithRetry(purchaseUrl, {
      method: 'POST',
      headers: this.buildAuthHeaders(),
      body: JSON.stringify(purchasePayload),
    });

    const purchaseData = await purchaseResp.json() as Record<string, unknown>;
    const checkoutUrl = (purchaseData.checkout_url as string) || '';
    const paymentId = (purchaseData.payment_id as string) || '';

    if (!checkoutUrl) {
      throw new HaiError('No checkout URL returned from API');
    }

    // Step 2: Notify caller of checkout URL
    if (options?.onCheckoutUrl) {
      options.onCheckoutUrl(checkoutUrl);
    }

    // Step 3: Poll for payment confirmation
    const paymentStatusUrl = this.makeUrl(`/api/benchmark/payments/${paymentId}/status`);
    const startTime = Date.now();

    while (Date.now() - startTime < pollTimeoutMs) {
      try {
        const statusResp = await this.fetchWithRetry(paymentStatusUrl, {
          headers: this.buildAuthHeaders(),
        });

        if (statusResp.status === 200) {
          const statusData = await statusResp.json() as Record<string, unknown>;
          const paymentStatus = (statusData.status as string) || '';

          if (paymentStatus === 'paid') break;
          if (['failed', 'expired', 'cancelled'].includes(paymentStatus)) {
            throw new HaiError(`Payment ${paymentStatus}: ${statusData.message || ''}`);
          }
        }
      } catch (e) {
        if (e instanceof HaiError) throw e;
        // Ignore transient errors during polling
      }

      await new Promise(resolve => setTimeout(resolve, pollIntervalMs));
    }

    if (Date.now() - startTime >= pollTimeoutMs) {
      throw new HaiError('Payment not confirmed within timeout. Complete payment and retry.');
    }

    // Step 4: Run the benchmark
    const runUrl = this.makeUrl('/api/benchmark/run');
    const runPayload = {
      name: `Baseline Run - ${this.jacsId.slice(0, 8)}`,
      tier: 'baseline',
      payment_id: paymentId,
      transport: options?.transport ?? 'sse',
    };

    const runResponse = await this.fetchWithRetry(runUrl, {
      method: 'POST',
      headers: this.buildAuthHeaders(),
      body: JSON.stringify(runPayload),
    }, Math.max(this.timeout, 300000));

    const data = await runResponse.json() as Record<string, unknown>;

    return {
      success: true,
      runId: (data.run_id as string) || (data.runId as string) || '',
      score: Number(data.score) || 0,
      transcript: this.parseTranscript((data.transcript as unknown[]) || []),
      paymentId,
      rawResponse: data,
    };
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
    const url = this.makeUrl(`/api/v1/agents/jobs/${jobId}/response`);

    const body: JobResponse = {
      response: {
        message,
        metadata: options?.metadata ?? null,
        processing_time_ms: options?.processingTimeMs ?? 0,
      },
    };

    // Sign the response as a JACS document
    const signed = signResponse(body, this.privateKeyPem, this.jacsId);

    const response = await this.fetchWithRetry(url, {
      method: 'POST',
      headers: this.buildAuthHeaders(),
      body: JSON.stringify(signed),
    });

    const data = await response.json() as Record<string, unknown>;

    return {
      success: (data.success as boolean) ?? true,
      jobId: (data.job_id as string) || (data.jobId as string) || jobId,
      message: (data.message as string) || 'Response accepted',
      rawResponse: data,
    };
  }

  // ---------------------------------------------------------------------------
  // connect()
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
  // testConnection()
  // ---------------------------------------------------------------------------

  /**
   * Test connectivity to the HAI server.
   *
   * Tries multiple health endpoints and returns true if any respond with 2xx.
   * Does not require authentication.
   */
  async testConnection(): Promise<boolean> {
    const endpoints = ['/api/v1/health', '/health', '/api/health', '/'];
    const timeoutMs = Math.min(this.timeout, 10000);

    for (const endpoint of endpoints) {
      try {
        const url = this.makeUrl(endpoint);
        const controller = new AbortController();
        const timeoutId = setTimeout(() => controller.abort(), timeoutMs);

        const resp = await fetch(url, {
          signal: controller.signal,
          redirect: 'follow',
        });

        clearTimeout(timeoutId);

        if (resp.ok) {
          return true;
        }
      } catch {
        // Ignore errors and try next endpoint
      }
    }
    return false;
  }

  // ---------------------------------------------------------------------------
  // Utility: export keys
  // ---------------------------------------------------------------------------

  /**
   * Export the agent's public key (derived from the private key).
   * Returns { publicKeyPem, privateKeyPem }.
   */
  exportKeys(): { publicKeyPem: string; privateKeyPem: string } {
    const { createPrivateKey, createPublicKey } = require('node:crypto') as typeof import('node:crypto');
    const privKey = createPrivateKey(this.privateKeyPem);
    const pubKey = createPublicKey(privKey);
    const publicKeyPem = pubKey.export({ type: 'spki', format: 'pem' }) as string;
    return { publicKeyPem, privateKeyPem: this.privateKeyPem };
  }

  // ---------------------------------------------------------------------------
  // SSE transport (internal)
  // ---------------------------------------------------------------------------

  private async *connectSse(
    onEvent?: (event: HaiEvent) => void,
  ): AsyncGenerator<HaiEvent> {
    const url = this.makeUrl('/api/v1/agents/connect');
    let reconnectDelay = 1000;
    const maxReconnectDelay = 60000;

    while (!this._shouldDisconnect) {
      try {
        const headers: Record<string, string> = {
          ...this.buildAuthHeaders(),
          'Accept': 'text/event-stream',
          'Cache-Control': 'no-cache',
        };
        if (this._lastEventId) {
          headers['Last-Event-ID'] = this._lastEventId;
        }

        const response = await fetch(url, { headers });

        if (response.status === 401) {
          throw new AuthenticationError('JACS signature rejected by HAI', 401);
        }
        if (!response.ok) {
          throw new HaiConnectionError(`SSE connection failed with status ${response.status}`);
        }
        if (!response.body) {
          throw new HaiConnectionError('SSE response has no body');
        }

        this._connected = true;
        reconnectDelay = 1000;

        for await (const event of parseSseStream(response.body)) {
          if (this._shouldDisconnect) break;
          if (event.id) this._lastEventId = event.id;

          // Unwrap signed events if we have server keys
          if (typeof event.data === 'object' && event.data !== null) {
            event.data = unwrapSignedEvent(
              event.data as Record<string, unknown>,
              this.serverPublicKeys,
            );
          }

          if (onEvent) onEvent(event);
          yield event;
        }
      } catch (e) {
        this._connected = false;
        if (this._shouldDisconnect) break;
        if (e instanceof HaiError) throw e;

        await new Promise(resolve => setTimeout(resolve, reconnectDelay));
        reconnectDelay = Math.min(reconnectDelay * 2, maxReconnectDelay);
      }
    }

    this._connected = false;
  }

  // ---------------------------------------------------------------------------
  // WebSocket transport (internal)
  // ---------------------------------------------------------------------------

  private async *connectWs(
    onEvent?: (event: HaiEvent) => void,
  ): AsyncGenerator<HaiEvent> {
    const wsUrl = this.baseUrl
      .replace(/^https:/, 'wss:')
      .replace(/^http:/, 'ws:')
      + '/ws/agent/connect';

    let reconnectDelay = 1000;
    const maxReconnectDelay = 60000;

    while (!this._shouldDisconnect) {
      try {
        const ws = await openWebSocket(wsUrl, {
          Authorization: this.buildAuthHeader(),
        }, this.timeout);
        this._wsConnection = ws;

        try {
          // Send JACS-signed handshake
          const handshake = this.buildWsHandshake();
          ws.send(JSON.stringify(handshake));

          // Wait for handshake ACK
          const ackData = await wsRecv(ws);
          if (typeof ackData === 'object' && ackData !== null) {
            const ack = ackData as Record<string, unknown>;
            if (ack.type === 'error') {
              const msg = (ack.message as string) || 'Handshake rejected';
              if (ack.code === 401) throw new AuthenticationError(msg, 401);
              throw new WebSocketError(msg);
            }
          }

          this._connected = true;
          reconnectDelay = 1000;

          // Yield connected event
          const connEvent: HaiEvent = {
            eventType: 'connected',
            data: ackData,
            raw: JSON.stringify(ackData),
          };
          if (onEvent) onEvent(connEvent);
          yield connEvent;

          // Yield all subsequent messages
          for await (const event of wsEventStream(ws)) {
            if (this._shouldDisconnect) break;
            if (event.id) this._lastEventId = event.id;

            // Auto-pong on heartbeat
            if (event.eventType === 'heartbeat') {
              const data = event.data as Record<string, unknown>;
              const timestamp = (data.timestamp as number) ?? Math.floor(Date.now() / 1000);
              ws.send(JSON.stringify({ type: 'pong', timestamp }));
            }

            if (onEvent) onEvent(event);
            yield event;
          }
        } finally {
          try { ws.close(); } catch { /* ignore */ }
          this._wsConnection = null;
        }
      } catch (e) {
        this._connected = false;
        if (this._shouldDisconnect) break;
        if (e instanceof HaiError) throw e;

        await new Promise(resolve => setTimeout(resolve, reconnectDelay));
        reconnectDelay = Math.min(reconnectDelay * 2, maxReconnectDelay);
      }
    }

    this._connected = false;
  }

  private buildWsHandshake(): Record<string, unknown> {
    const timestamp = Math.floor(Date.now() / 1000).toString();
    const message = `${this.jacsId}:${timestamp}`;
    const signature = signString(this.privateKeyPem, message);

    const handshake: Record<string, unknown> = {
      type: 'handshake',
      agent_id: this.jacsId,
      timestamp,
      signature,
    };

    if (this._lastEventId) {
      handshake.last_event_id = this._lastEventId;
    }

    return handshake;
  }

  // ---------------------------------------------------------------------------
  // Fetch with retry and error handling
  // ---------------------------------------------------------------------------

  private async fetchWithRetry(
    url: string,
    init: RequestInit,
    timeoutMs?: number,
  ): Promise<Response> {
    const effectiveTimeout = timeoutMs ?? this.timeout;
    let lastError: Error | null = null;

    for (let attempt = 0; attempt < this.maxRetries; attempt++) {
      try {
        const controller = new AbortController();
        const timeoutId = setTimeout(() => controller.abort(), effectiveTimeout);

        const response = await fetch(url, {
          ...init,
          signal: controller.signal,
        });

        clearTimeout(timeoutId);

        if (response.status === 401) {
          throw new AuthenticationError('JACS signature rejected by HAI', 401);
        }
        if (response.status === 429) {
          throw new HaiError('Rate limited', 429);
        }
        if (response.ok) {
          return response;
        }

        let msg = `Request failed with status ${response.status}`;
        try {
          const errBody = await response.json() as Record<string, unknown>;
          if (errBody.error) msg = String(errBody.error);
        } catch { /* empty */ }
        lastError = new HaiError(msg, response.status);
      } catch (e) {
        if (e instanceof HaiError) throw e;
        if (e instanceof Error && e.name === 'AbortError') {
          throw new HaiConnectionError(`Request timed out after ${effectiveTimeout}ms`);
        }
        lastError = e instanceof Error ? e : new Error(String(e));
      }

      // Exponential backoff
      if (attempt < this.maxRetries - 1) {
        await new Promise(resolve => setTimeout(resolve, Math.pow(2, attempt) * 1000));
      }
    }

    throw lastError ?? new HaiError('Request failed after all retries');
  }

  // ---------------------------------------------------------------------------
  // Transcript parsing
  // ---------------------------------------------------------------------------

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
    this.serverPublicKeys = await getServerKeys(this.baseUrl);
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
  async getAgentAttestation(agentId: string): Promise<Record<string, unknown>> {
    const url = this.makeUrl(`/api/v1/agents/${agentId}/status`);

    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: this.buildAuthHeaders(),
    });

    return await response.json() as Record<string, unknown>;
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
      this.privateKeyPem,
      this.jacsId,
    );
  }

  // ---------------------------------------------------------------------------
  // benchmark() -- legacy suite-based
  // ---------------------------------------------------------------------------

  /**
   * Run a legacy suite-based benchmark.
   *
   * @param suite - Benchmark suite name (e.g., 'mediation_basic')
   * @returns Benchmark result with scores
   */
  async benchmark(suite: string = 'mediation_basic'): Promise<Record<string, unknown>> {
    const url = this.makeUrl('/api/benchmark/run');

    const response = await this.fetchWithRetry(url, {
      method: 'POST',
      headers: this.buildAuthHeaders(),
      body: JSON.stringify({ suite }),
    });

    const data = await response.json() as Record<string, unknown>;
    return data;
  }
}
