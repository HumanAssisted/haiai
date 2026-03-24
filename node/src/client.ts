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
  FullyCertifiedResult,
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
  JobResponse,
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
  FieldResult,
  FieldStatus,
  ChainEntry,
} from './types.js';
import {
  HaiError,
  AuthenticationError,
  HaiConnectionError,
  HaiApiError,
  EmailNotActiveError,
  RecipientNotFoundError,
  RateLimitedError,
} from './errors.js';
import {
  constants as cryptoConstants,
  createPrivateKey,
  createPublicKey,
  sign as cryptoSign,
} from 'node:crypto';
import { rmSync } from 'node:fs';
import { signResponse, canonicalJson, getServerKeys, unwrapSignedEvent } from './signing.js';
import { loadConfig } from './config.js';
import { JacsAgent, createAgentSync, verifyDocumentStandalone, hashString } from '@hai.ai/jacs';
import { parseSseStream } from './sse.js';
import { openWebSocket, wsEventStream } from './ws.js';

function armorKeyData(raw: Buffer, blockType: string): string {
  const base64 = raw.toString('base64');
  const lines = base64.match(/.{1,64}/g) ?? [];
  return `-----BEGIN ${blockType}-----\n${lines.join('\n')}\n-----END ${blockType}-----`;
}

function normalizeKeyText(raw: Buffer, blockType: string): string {
  const text = raw.toString('utf-8').trim();
  if (text.includes(`BEGIN ${blockType}`)) {
    return text;
  }
  if (blockType === 'PUBLIC KEY' && text.includes('BEGIN RSA PUBLIC KEY')) {
    return text;
  }
  return armorKeyData(raw, blockType);
}

type CredentialSigner = {
  algorithm: 'ring-Ed25519' | 'RSA-PSS';
  privateKeyPem: string;
  publicKeyPem: string;
  signStringSync(message: string): string;
};

/** Module-level WeakMap to hold private key PEM material off the client instance. */
const privateKeyStore = new WeakMap<HaiClient, string>();

const credentialWorkspaceDirs = new Set<string>();
let credentialWorkspaceCleanupRegistered = false;

function registerCredentialWorkspace(dir: string): void {
  credentialWorkspaceDirs.add(dir);
  if (credentialWorkspaceCleanupRegistered) {
    return;
  }
  credentialWorkspaceCleanupRegistered = true;
  process.once('exit', () => {
    for (const workspaceDir of credentialWorkspaceDirs) {
      try {
        rmSync(workspaceDir, { recursive: true, force: true });
      } catch {
        // Best-effort temp workspace cleanup.
      }
    }
    credentialWorkspaceDirs.clear();
  });
}

function createCredentialSigner(
  privateKeyPem: string,
  privateKeyPassphrase?: string,
): CredentialSigner {
  const privateKey = privateKeyPassphrase
    ? createPrivateKey({ key: privateKeyPem, format: 'pem', passphrase: privateKeyPassphrase })
    : createPrivateKey({ key: privateKeyPem, format: 'pem' });
  const keyType = privateKey.asymmetricKeyType;

  if (keyType !== 'ed25519' && keyType !== 'rsa' && keyType !== 'rsa-pss') {
    throw new AuthenticationError(
      `fromCredentials does not support ${keyType ?? 'this'} private key type in this runtime. ` +
      'Use a config-backed client for other JACS key types.',
    );
  }

  const publicKeyPem = createPublicKey(privateKey)
    .export({ format: 'pem', type: 'spki' })
    .toString()
    .trim();

  return {
    algorithm: keyType === 'ed25519' ? 'ring-Ed25519' : 'RSA-PSS',
    privateKeyPem: privateKeyPem.trim(),
    publicKeyPem,
    signStringSync(message: string): string {
      const data = Buffer.from(message, 'utf-8');
      const signature = keyType === 'ed25519'
        ? cryptoSign(null, data, privateKey)
        : cryptoSign('sha256', data, {
          key: privateKey,
          padding: cryptoConstants.RSA_PKCS1_PSS_PADDING,
          saltLength: cryptoConstants.RSA_PSS_SALTLEN_DIGEST,
        });
      return signature.toString('base64');
    },
  };
}

async function withPrivateKeyPassphrase<T>(
  passphrase: string | undefined,
  run: () => Promise<T>,
): Promise<T> {
  const envKey = 'JACS_PRIVATE_KEY_PASSWORD';
  const hadOriginal = Object.prototype.hasOwnProperty.call(process.env, envKey);
  const original = process.env[envKey];

  if (passphrase) {
    process.env[envKey] = passphrase;
  }

  try {
    return await run();
  } finally {
    if (hadOriginal) {
      process.env[envKey] = original;
    } else {
      delete process.env[envKey];
    }
  }
}

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

/** Default HAI API base URL. Override with the `url` option or `HAI_URL` env var. */
export const DEFAULT_BASE_URL = 'https://beta.hai.ai';

export class HaiClient {
  private config!: AgentConfig;
  private configPath: string | null = null;
  /** JACS native agent for all cryptographic operations. */
  private agent!: JacsAgent;
  /** Explicit credential signer used when the runtime cannot import a PEM into JACS directly. */
  private credentialSigner: CredentialSigner | null = null;
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
   * config-based flows. When this runtime cannot import PEM material into
   * JACS directly, signing uses the supplied credentials and verification
   * still delegates to JACS.
   */
  static async fromCredentials(
    jacsId: string,
    privateKeyPem: string,
    options?: Omit<HaiClientOptions, 'configPath'> & { privateKeyPassphrase?: string },
  ): Promise<HaiClient> {
    const client = new HaiClient(options);
    const signer = createCredentialSigner(privateKeyPem, options?.privateKeyPassphrase);

    // Store the caller's PEM material for exportKeys/rotateKeys compatibility.
    privateKeyStore.set(client, signer.privateKeyPem);
    (client as any)._publicKeyPem = signer.publicKeyPem;
    (client as any)._privateKeyPassphrase = options?.privateKeyPassphrase;
    client.credentialSigner = signer;

    // Materialize a minimal JACS-shaped workspace so file-based flows (for
    // example rotateKeys) have a stable key directory, even though this
    // runtime still lacks a direct "load PEM credentials into JacsAgent"
    // bootstrap path.
    const { mkdir, mkdtemp, writeFile } = await import('node:fs/promises');
    const { join } = await import('node:path');
    const { tmpdir } = await import('node:os');
    const tempDir = await mkdtemp(join(tmpdir(), 'haiai-creds-'));
    registerCredentialWorkspace(tempDir);
    const keyDir = join(tempDir, 'keys');
    const dataDir = join(tempDir, 'data');
    await mkdir(keyDir, { recursive: true });
    await mkdir(dataDir, { recursive: true });
    await writeFile(join(keyDir, 'agent_private_key.pem'), `${signer.privateKeyPem}\n`, { mode: 0o600 });
    await writeFile(join(keyDir, 'agent_public_key.pem'), `${signer.publicKeyPem}\n`, { mode: 0o644 });

    const configPath = join(tempDir, 'jacs.config.json');
    const configJson = {
      jacsAgentName: jacsId,
      jacsAgentVersion: '1.0.0',
      jacsKeyDir: './keys',
      jacsPrivateKeyPath: './keys/agent_private_key.pem',
      jacsId,
      jacs_data_directory: './data',
      jacs_key_directory: './keys',
      jacs_agent_private_key_filename: 'agent_private_key.pem',
      jacs_agent_public_key_filename: 'agent_public_key.pem',
      jacs_agent_key_algorithm: signer.algorithm,
      jacs_default_storage: 'fs',
    };
    await writeFile(configPath, JSON.stringify(configJson, null, 2) + '\n');
    client.config = await loadConfig(configPath);
    client.configPath = configPath;

    // Keep a lightweight JACS agent around for canonicalization and explicit
    // verifyStringSync checks. Signing is handled by the credentialSigner above
    // so fromCredentials actually uses the caller's key material.
    client.agent = new JacsAgent();
    client.agent.ephemeralSync(signer.algorithm);

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

  private activeSigner(): CredentialSigner | JacsAgent {
    return this.credentialSigner ?? this.agent;
  }

  // ---------------------------------------------------------------------------
  // Auth helpers
  // ---------------------------------------------------------------------------

  /**
   * Build JACS Authorization header.
   * Format: `JACS {jacsId}:{timestamp}:{signature_base64}`
   */
  private buildAuthHeaders(): Record<string, string> {
    return {
      'Authorization': this.buildAuthHeader(),
      'Content-Type': 'application/json',
    };
  }

  /** Sign a UTF-8 message with the agent's private key via JACS. Returns base64. */
  signMessage(message: string): string {
    return this.activeSigner().signStringSync(message);
  }

  /** Build the JACS Authorization header value string. */
  buildAuthHeader(): string {
    if (this.credentialSigner) {
      const timestamp = Math.floor(Date.now() / 1000).toString();
      const message = `${this.jacsId}:${timestamp}`;
      const signature = this.credentialSigner.signStringSync(message);
      return `JACS ${this.jacsId}:${timestamp}:${signature}`;
    }
    // Prefer JACS binding delegation
    if ('buildAuthHeaderSync' in this.agent && typeof (this.agent as unknown as Record<string, unknown>).buildAuthHeaderSync === 'function') {
      return (this.agent as unknown as Record<string, unknown> & { buildAuthHeaderSync: () => string }).buildAuthHeaderSync();
    }
    // Fallback: local construction
    const timestamp = Math.floor(Date.now() / 1000).toString();
    const message = `${this.jacsId}:${timestamp}`;
    const signature = this.agent.signStringSync(message);
    return `JACS ${this.jacsId}:${timestamp}:${signature}`;
  }

  private makeUrl(path: string): string {
    const cleanPath = path.startsWith('/') ? path : `/${path}`;
    return `${this.baseUrl}${cleanPath}`;
  }

  private encodePathSegment(segment: string): string {
    return encodeURIComponent(segment);
  }

  private usernameEndpoint(agentId: string): string {
    const safeAgentId = this.encodePathSegment(agentId);
    return this.makeUrl(`/api/v1/agents/${safeAgentId}/username`);
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
   *
   * This is the haiai equivalent of JACS's `registerWithHai()`. Unlike
   * the JACS version (which uses API-key Bearer auth), this method uses
   * the self-signed agent document as authentication. See also {@link registerNewAgent}
   * for a full generate-and-register workflow.
   *
   * @param options - Optional registration parameters
   */
  async register(options?: {
    ownerEmail?: string;
    description?: string;
    domain?: string;
    agentJson?: string;
    publicKeyPem?: string;
  }): Promise<RegistrationResult> {
    const derived = this.exportKeys();
    const publicKeyPem = options?.publicKeyPem ?? derived.publicKeyPem;
    let agentJson = options?.agentJson;

    if (!agentJson) {
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
        description: options?.description ?? 'Agent registered via Node SDK',
        capabilities: ['mediation'],
        version: this.config.jacsAgentVersion,
      };
      if (options?.domain) {
        agentDoc.domain = options.domain;
      }

      // Sign canonical JSON via JACS
      const canonical = canonicalJson(agentDoc);
      const signature = this.activeSigner().signStringSync(canonical);
      (agentDoc.jacsSignature as Record<string, string>).signature = signature;
      agentJson = JSON.stringify(agentDoc);
    }

    const url = this.makeUrl('/api/v1/agents/register');
    const publicKeyB64 = Buffer.from(publicKeyPem, 'utf-8').toString('base64');
    const body: Record<string, unknown> = {
      agent_json: agentJson,
      public_key: publicKeyB64,
    };
    if (options?.ownerEmail) {
      body.owner_email = options.ownerEmail;
    }
    if (options?.domain) {
      body.domain = options.domain;
    }
    if (options?.description) {
      body.description = options.description;
    }

    const response = await this.fetchWithRetry(url, {
      method: 'POST',
      // New-agent registration is self-authenticated by the signed agent document.
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });

    const data = await response.json() as Record<string, unknown>;

    // After successful registration, store the HAI-assigned agent_id (UUID).
    // Email endpoints use this UUID in their URL paths while auth headers
    // continue to use the original JACS ID string.
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
   * Archives old keys, generates a new keypair via JACS core, builds a new
   * self-signed agent document, updates config, and optionally re-registers
   * with HAI.
   *
   * @param options - Rotation options (registerWithHai, haiUrl).
   * @returns RotationResult with old/new versions and registration status.
   */
  async rotateKeys(options?: RotateKeysOptions): Promise<RotationResult> {
    const {
      copyFile,
      mkdtemp,
      readFile: readF,
      rename,
      rm,
      stat: fsStat,
      writeFile,
    } = await import('node:fs/promises');
    const { randomUUID } = await import('node:crypto');
    const { join, resolve } = await import('node:path');
    const { tmpdir } = await import('node:os');

    const registerWithHai = options?.registerWithHai ?? true;
    const haiUrl = options?.haiUrl ?? this.baseUrl;

    if (!this.config.jacsId) {
      throw new AuthenticationError('Cannot rotate keys: no jacsId in config. Register first.');
    }

    const jacsId = this.config.jacsId;
    const oldVersion = this.config.jacsAgentVersion;
    const keyDir = this.config.jacsKeyDir;

    // Build old-key auth header BEFORE rotation (chain of trust)
    const oldAuthTimestamp = Math.floor(Date.now() / 1000).toString();
    const oldAuthMessage = `${jacsId}:${oldVersion}:${oldAuthTimestamp}`;
    const oldAuthSig = this.activeSigner().signStringSync(oldAuthMessage);
    const oldAgent = this.agent;

    // Find existing private key file
    const candidates = [
      join(keyDir, 'agent_private_key.pem'),
      join(keyDir, `${this.config.jacsAgentName}.private.pem`),
      join(keyDir, 'private_key.pem'),
    ];

    let privKeyPath: string | null = null;
    for (const candidate of candidates) {
      try {
        await fsStat(candidate);
        privKeyPath = candidate;
        break;
      } catch {
        // continue
      }
    }

    if (!privKeyPath) {
      throw new AuthenticationError(
        `Cannot rotate keys: private key not found. Searched: ${candidates.join(', ')}`,
      );
    }

    // Derive public key path
    const pubKeyPath = privKeyPath.replace('private', 'public');

    // 1. Archive old keys
    const archivePriv = privKeyPath.replace('.pem', `.${oldVersion}.pem`);
    const archivePub = pubKeyPath.replace('.pem', `.${oldVersion}.pem`);

    await rename(privKeyPath, archivePriv);
    try {
      await fsStat(pubKeyPath);
      await rename(pubKeyPath, archivePub);
    } catch (err) {
      if ((err as NodeJS.ErrnoException).code !== 'ENOENT') {
        console.warn('Failed to archive public key:', err);
      }
    }

    // 2. Generate new JACS agent (keys + config) via JACS core
    const passphrase = (this as any)._privateKeyPassphrase
      ?? process.env.JACS_PRIVATE_KEY_PASSWORD
      ?? '';
    const newVersion = randomUUID();
    const generatedKeyDir = await mkdtemp(join(tmpdir(), 'haiai-rotate-'));
    let newPublicKeyPem: string;
    let generatedSignerAgent: JacsAgent | null = null;
    try {
      const resultJson = createAgentSync(
        this.config.jacsAgentName,
        passphrase,
        'pq2025',
        null, // data dir
        generatedKeyDir,
        join(generatedKeyDir, 'jacs.config.json'),
        null, // agent type
        (this.config as unknown as Record<string, unknown>).description as string
          ?? 'Agent registered via Node SDK',
        null, // domain
        null, // default storage
      );
      const result = JSON.parse(resultJson);
      const generatedConfigPath = result.config_path as string | undefined;
      const newPubKeyPath = result.public_key_path || join(keyDir, 'jacs.public.pem');
      const newPrivKeyPath = result.private_key_path || join(keyDir, 'jacs.private.pem.enc');
      const newPublicKeyRaw = await readF(newPubKeyPath);
      newPublicKeyPem = normalizeKeyText(newPublicKeyRaw, 'PUBLIC KEY');

      if (generatedConfigPath) {
        const nextAgent = new JacsAgent();
        try {
          await withPrivateKeyPassphrase(passphrase || undefined, () => nextAgent.load(generatedConfigPath));
          generatedSignerAgent = nextAgent;
        } catch {
          generatedSignerAgent = null;
        }
      }

      if (newPrivKeyPath !== privKeyPath) {
        await copyFile(newPrivKeyPath, privKeyPath);
      }
      if (newPubKeyPath !== pubKeyPath) {
        await writeFile(pubKeyPath, `${newPublicKeyPem}\n`);
      } else {
        await writeFile(pubKeyPath, `${newPublicKeyPem}\n`);
      }
      privateKeyStore.set(this, (await readF(privKeyPath, 'utf-8')).trim());
      (this as any)._publicKeyPem = newPublicKeyPem.trim();
    } catch (err) {
      // Rollback: restore archived keys
      await rename(archivePriv, privKeyPath).catch(() => {});
      try { await rename(archivePub, pubKeyPath); } catch { /* noop */ }
      throw new AuthenticationError(`Key generation failed: ${err}`);
    } finally {
      await rm(generatedKeyDir, { recursive: true, force: true }).catch(() => {});
    }

    // 3. Build new agent document
    const agentDoc: Record<string, unknown> = {
      jacsId,
      jacsVersion: newVersion,
      jacsPreviousVersion: oldVersion,
      jacsPublicKey: newPublicKeyPem,
      name: this.config.jacsAgentName,
      description: (this.config as unknown as Record<string, unknown>).description
        ?? (this.config as unknown as Record<string, unknown>).jacsAgentDescription
        ?? `Agent registered via Node SDK`,
      jacsSignature: {
        agentID: jacsId,
        date: new Date().toISOString(),
      },
    };

    // Reload the agent with new keys for signing
    const configPath = resolve(process.env.JACS_CONFIG_PATH ?? this.configPath ?? './jacs.config.json');
    const reloadedAgent = new JacsAgent();
    let reloadedWithNewKeys = false;
    try {
      await withPrivateKeyPassphrase(passphrase || undefined, () => reloadedAgent.load(configPath));
      this.agent = reloadedAgent;
      reloadedWithNewKeys = true;
    } catch {
      if (generatedSignerAgent) {
        this.agent = generatedSignerAgent;
        reloadedWithNewKeys = true;
      } else {
        // Fall back to the currently loaded in-memory agent when we cannot
        // hydrate the new key material in this process.
        this.agent = oldAgent;
      }
    }

    let rotatedSigner: CredentialSigner | JacsAgent = this.agent;
    if (this.credentialSigner) {
      try {
        this.credentialSigner = createCredentialSigner(
          privateKeyStore.get(this) ?? '',
          passphrase || undefined,
        );
        rotatedSigner = this.credentialSigner;
      } catch {
        // Some runtimes can generate post-quantum keys before they can
        // re-import them through the local PEM compatibility path. In that
        // case we keep the best available in-memory signer and preserve the
        // local rotation result rather than failing the entire operation.
        this.credentialSigner = null;
        rotatedSigner = this.agent;
      }
    }

    const canonical = canonicalJson(agentDoc);
    const signature = rotatedSigner.signStringSync(canonical);
    (agentDoc.jacsSignature as Record<string, string>).signature = signature;
    const signedAgentJson = JSON.stringify(agentDoc, null, 2);

    // 4. Compute new public key hash via JACS
    const newPublicKeyHash = hashString(newPublicKeyPem);

    // 5. Update in-memory state
    this.config = {
      ...this.config,
      jacsAgentVersion: newVersion,
    };

    // 6. Update config file
    try {
      const raw = JSON.parse(await readF(configPath, 'utf-8')) as Record<string, unknown>;
      raw.jacsAgentVersion = newVersion;
      await writeFile(configPath, JSON.stringify(raw, null, 2) + '\n');
    } catch {
      // Config update failure is non-fatal for rotation
    }

    // 7. Optionally re-register with HAI using the OLD key for auth
    let registeredWithHai = false;
    if (registerWithHai && haiUrl) {
      try {
        const authHeader = `JACS ${jacsId}:${oldVersion}:${oldAuthTimestamp}:${oldAuthSig}`;

        const url = this.makeUrl('/api/v1/agents/register');
        const publicKeyB64 = Buffer.from(newPublicKeyPem, 'utf-8').toString('base64');
        const body = JSON.stringify({
          agent_json: signedAgentJson,
          public_key: publicKeyB64,
        });
        const resp = await this.fetchWithRetry(url, {
          method: 'POST',
          headers: {
            'Authorization': authHeader,
            'Content-Type': 'application/json',
          },
          body,
        });
        if (resp.ok) {
          registeredWithHai = true;
        }
      } catch {
        // HAI failure is non-fatal — local rotation is preserved
      }
    }

    return {
      jacsId,
      oldVersion,
      newVersion,
      newPublicKeyHash,
      registeredWithHai,
      signedAgentJson,
    };
  }

  // ---------------------------------------------------------------------------
  // verify()
  // ---------------------------------------------------------------------------

  /** Verify the agent's registration status. */
  async verify(): Promise<VerifyAgentResult> {
    const safeJacsId = this.encodePathSegment(this.jacsId);
    const url = this.makeUrl(`/api/v1/agents/${safeJacsId}/verify`);

    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: this.buildAuthHeaders(),
    });

    const data = await response.json() as Record<string, unknown>;

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
    const url = this.makeUrl('/api/benchmark/run');
    const payload = {
      name: `Free Run - ${this.jacsId.slice(0, 8)}`,
      tier: 'free',
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
  // proRun()
  // ---------------------------------------------------------------------------

  /**
   * Run a pro tier benchmark ($20/month).
   *
   * Flow: create Stripe checkout -> poll for payment -> run benchmark.
   */
  async proRun(options?: ProRunOptions): Promise<ProRunResult> {
    const pollIntervalMs = options?.pollIntervalMs ?? 2000;
    const pollTimeoutMs = options?.pollTimeoutMs ?? 300000;

    // Step 1: Create Stripe Checkout session
    const purchaseUrl = this.makeUrl('/api/benchmark/purchase');
    const purchasePayload = { tier: 'pro', agent_id: this.jacsId };

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
    const paymentStatusUrl = this.makeUrl(
      `/api/benchmark/payments/${this.encodePathSegment(paymentId)}/status`,
    );
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
      name: `Pro Run - ${this.jacsId.slice(0, 8)}`,
      tier: 'pro',
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
    const safeJobId = this.encodePathSegment(jobId);
    const url = this.makeUrl(`/api/v1/agents/jobs/${safeJobId}/response`);

    const body: JobResponse = {
      response: {
        message,
        metadata: options?.metadata ?? null,
        processing_time_ms: options?.processingTimeMs ?? 0,
      },
    };

    // Sign the response as a JACS document via JACS
    const signed = signResponse(body, this.activeSigner(), this.jacsId, this.agent);

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
    const url = this.makeUrl(`/api/v1/agents/username/check?username=${encodeURIComponent(username)}`);

    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: { 'Content-Type': 'application/json' },
    });

    const data = await response.json() as Record<string, unknown>;

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
    const url = this.usernameEndpoint(agentId);

    const response = await this.fetchWithRetry(url, {
      method: 'POST',
      headers: this.buildAuthHeaders(),
      body: JSON.stringify({ username }),
    });

    const data = await response.json() as Record<string, unknown>;

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
    const url = this.usernameEndpoint(agentId);

    const response = await this.fetchWithRetry(url, {
      method: 'PUT',
      headers: this.buildAuthHeaders(),
      body: JSON.stringify({ username }),
    });

    const data = await response.json() as Record<string, unknown>;
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
    const url = this.usernameEndpoint(agentId);

    const response = await this.fetchWithRetry(url, {
      method: 'DELETE',
      headers: this.buildAuthHeaders(),
    });

    const data = await response.json() as Record<string, unknown>;
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
    const url = this.makeUrl('/api/jacs/verify');
    const rawDocument = typeof document === 'string' ? document : JSON.stringify(document);

    const response = await this.fetchWithRetry(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ document: rawDocument }),
    });

    const data = await response.json() as Record<string, unknown>;
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
    const safeAgentId = this.encodePathSegment(agentId);
    const url = this.makeUrl(`/api/v1/agents/${safeAgentId}/verification`);
    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: { 'Content-Type': 'application/json' },
    });

    const data = await response.json() as Record<string, unknown>;
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
    const url = this.makeUrl('/api/v1/agents/verify');
    const payload: Record<string, unknown> = {
      agent_json: typeof agentJson === 'string' ? agentJson : JSON.stringify(agentJson),
    };
    if (options?.publicKey) {
      payload.public_key = options.publicKey;
    }
    if (options?.domain) {
      payload.domain = options.domain;
    }

    const response = await this.fetchWithRetry(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    });

    const data = await response.json() as Record<string, unknown>;
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
    const { mkdtemp, readFile: readF, rm } = await import('node:fs/promises');
    const { join } = await import('node:path');
    const { tmpdir } = await import('node:os');

    // Generate a new JACS agent with keys via JACS core
    const tempDir = await mkdtemp(join(tmpdir(), 'haiai-register-'));
    const keyDir = join(tempDir, 'keys');
    const dataDir = join(tempDir, 'data');
    const { randomBytes } = await import('node:crypto');
    const passphrase = process.env.JACS_PRIVATE_KEY_PASSWORD ?? randomBytes(32).toString('hex');
    try {
      const resultJson = createAgentSync(
        agentName,
        passphrase,
        'pq2025',
        dataDir,
        keyDir,
        join(tempDir, 'jacs.config.json'),
        null,
        options.description ?? 'Agent registered via Node SDK',
        options.domain ?? null,
        null,
      );
      const createResult = JSON.parse(resultJson);

      const pubKeyPath = createResult.public_key_path || join(keyDir, 'jacs.public.pem');
      const publicKeyPem = normalizeKeyText(await readF(pubKeyPath), 'PUBLIC KEY');

      // Load the new agent for signing
      const tempAgent = new JacsAgent();
      const tempConfigPath = createResult.config_path || join(tempDir, 'jacs.config.json');
      const { resolve } = await import('node:path');
      let signingAgent: JacsAgent = tempAgent;
      try {
        await withPrivateKeyPassphrase(passphrase || undefined, () => tempAgent.load(resolve(tempConfigPath)));
      } catch {
        tempAgent.ephemeralSync('pq2025');
        signingAgent = tempAgent;
      }

      // Build minimal JACS agent document
      const agentDoc: Record<string, unknown> = {
        jacsId: agentName,
        jacsVersion: '1.0.0',
        jacsSignature: {
          agentID: agentName,
          date: new Date().toISOString(),
        },
        jacsPublicKey: publicKeyPem,
        name: agentName,
        description: options.description ?? 'Agent registered via Node SDK',
        capabilities: ['mediation'],
        version: '1.0.0',
      };

      // Sign canonical JSON via JACS
      const canonical = canonicalJson(agentDoc);
      const signature = signingAgent.signStringSync(canonical);
      (agentDoc.jacsSignature as Record<string, string>).signature = signature;

      const url = this.makeUrl('/api/v1/agents/register');
      const publicKeyB64 = Buffer.from(publicKeyPem, 'utf-8').toString('base64');
      const body: Record<string, unknown> = {
        agent_json: JSON.stringify(agentDoc),
        public_key: publicKeyB64,
        owner_email: options.ownerEmail,
      };
      if (options.domain) {
        body.domain = options.domain;
      }
      if (options.description) {
        body.description = options.description;
      }

      const response = await this.fetchWithRetry(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });

      const data = await response.json() as Record<string, unknown>;

      if (!options.quiet) {
        console.log(`\nAgent created and submitted for registration!`);
        console.log(`  -> Check your email (${options.ownerEmail}) for a verification link`);
        console.log(`  -> Click the link and log into hai.ai to complete registration`);
        console.log(`  -> After verification, claim a @hai.ai username with:`);
        console.log(`     client.claimUsername('${(data.agent_id as string) || ''}', 'my-agent')`);
        console.log(`  -> Save your config and private key to a secure, access-controlled location`);

        if (options.domain) {
          const pubKeyHash = hashString(publicKeyPem);
          console.log(`\n--- DNS Setup Instructions ---`);
          console.log(`Add this TXT record to your domain '${options.domain}':`);
          console.log(`  Name:  _jacs.${options.domain}`);
          console.log(`  Type:  TXT`);
          console.log(`  Value: sha256:${pubKeyHash}`);
          console.log(`DNS verification enables the pro tier.\n`);
        } else {
          console.log();
        }
      }

      return {
        success: true,
        agentId: (data.agent_id as string) || (data.agentId as string) || '',
        jacsId: (data.jacs_id as string) || (data.jacsId as string) || (agentDoc.jacsId as string) || '',
        haiSignature: (data.hai_signature as string) || (data.haiSignature as string) || '',
        registrationId: (data.registration_id as string) || (data.registrationId as string) || '',
        registeredAt: (data.registered_at as string) || (data.registeredAt as string) || '',
        rawResponse: data,
      };
    } finally {
      await rm(tempDir, { recursive: true, force: true }).catch(() => {});
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
   * Export the agent's public key.
   * Reads the public key from the JACS key directory.
   * Returns { publicKeyPem }.
   */
  exportKeys(): { publicKeyPem: string; privateKeyPem?: string } {
    const fs = require('node:fs');
    const path = require('node:path');
    const explicitPublicKeyPem = (this as any)._publicKeyPem;
    const explicitPrivateKeyPem = privateKeyStore.get(this);
    if (typeof explicitPublicKeyPem === 'string' && explicitPublicKeyPem.trim() !== '') {
      return {
        publicKeyPem: explicitPublicKeyPem.trim(),
        privateKeyPem: explicitPrivateKeyPem,
      };
    }
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
        return {
          publicKeyPem: normalizeKeyText(content, 'PUBLIC KEY'),
          privateKeyPem: typeof explicitPrivateKeyPem === 'string' ? explicitPrivateKeyPem : undefined,
        };
      } catch {
        // try next
      }
    }

    throw new AuthenticationError(
      `No public key found. Searched: ${candidates.join(', ')}`,
    );
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
    let reconnectAttempts = 0;

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

        reconnectAttempts++;
        if (reconnectAttempts >= this.maxReconnectAttempts) {
          throw new HaiConnectionError(
            `SSE connection failed after ${reconnectAttempts} reconnect attempts`,
          );
        }

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
    let reconnectAttempts = 0;

    while (!this._shouldDisconnect) {
      try {
        const headers: Record<string, string> = {
          Authorization: this.buildAuthHeader(),
        };
        if (this._lastEventId) {
          headers['Last-Event-ID'] = this._lastEventId;
        }

        const ws = await openWebSocket(wsUrl, headers, this.timeout);
        this._wsConnection = ws;

        try {
          this._connected = true;
          reconnectDelay = 1000;

          // Yield connected event
          const connEvent: HaiEvent = {
            eventType: 'connected',
            data: null,
            raw: '',
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

        reconnectAttempts++;
        if (reconnectAttempts >= this.maxReconnectAttempts) {
          throw new HaiConnectionError(
            `WebSocket connection failed after ${reconnectAttempts} reconnect attempts`,
          );
        }

        await new Promise(resolve => setTimeout(resolve, reconnectDelay));
        reconnectDelay = Math.min(reconnectDelay * 2, maxReconnectDelay);
      }
    }

    this._connected = false;
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
        if (response.ok) {
          return response;
        }

        let msg = `Request failed with status ${response.status}`;
        try {
          const errBody = await response.json() as Record<string, unknown>;
          if (errBody.error) msg = String(errBody.error);
        } catch { /* empty */ }
        lastError = response.status === 429
          ? new RateLimitedError(msg, 429)
          : new HaiError(msg, response.status);
      } catch (e) {
        if (e instanceof HaiError && !(e instanceof RateLimitedError)) throw e;
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
  async getAgentAttestation(agentId: string): Promise<VerifyAgentResult> {
    const safeAgentId = this.encodePathSegment(agentId);
    const url = this.makeUrl(`/api/v1/agents/${safeAgentId}/verify`);

    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: this.buildAuthHeaders(),
    });

    const data = await response.json() as Record<string, unknown>;

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
      this.activeSigner(),
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
    const url = this.makeUrl('/api/benchmark/run');

    const response = await this.fetchWithRetry(url, {
      method: 'POST',
      headers: this.buildAuthHeaders(),
      body: JSON.stringify({ name, tier }),
    });

    const data = await response.json() as Record<string, unknown>;
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
    const safeAgentId = this.encodePathSegment(this.haiAgentId);
    const url = this.makeUrl(`/api/agents/${safeAgentId}/email/send`);

    if (!this.agentEmail) {
      throw new Error('agent email not set — call claimUsername first');
    }

    // Server handles JACS attachment signing (TASK_014/018).
    // Client only sends content fields.
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), this.timeout);

    let response: Response;
    try {
      const payload: Record<string, unknown> = {
          to: options.to,
          subject: options.subject,
          body: options.body,
          in_reply_to: options.inReplyTo,
          attachments: options.attachments?.map(a => ({
            filename: a.filename,
            content_type: a.contentType,
            data_base64: a.data.toString('base64'),
          })),
        };
      if (options.cc?.length) payload.cc = options.cc;
      if (options.bcc?.length) payload.bcc = options.bcc;
      if (options.labels?.length) payload.labels = options.labels;

      response = await fetch(url, {
        method: 'POST',
        headers: this.buildAuthHeaders(),
        body: JSON.stringify(payload),
        signal: controller.signal,
      });
    } catch (e) {
      clearTimeout(timeoutId);
      if (e instanceof Error && e.name === 'AbortError') {
        throw new HaiConnectionError(`Request timed out after ${this.timeout}ms`);
      }
      throw e;
    }
    clearTimeout(timeoutId);

    if (!response.ok) {
      const text = await response.text();
      let errCode = '';
      let errMsg = text;
      try {
        const errData = JSON.parse(text) as Record<string, unknown>;
        errCode = (errData.error_code as string) || '';
        errMsg = (errData.message as string) || (errData.error as string) || text;
      } catch { /* non-JSON body */ }

      if (response.status === 401) {
        throw new AuthenticationError('JACS signature rejected by HAI', 401);
      }
      if (response.status === 403 && (errCode === 'EMAIL_NOT_ACTIVE' || text.toLowerCase().includes('allocated'))) {
        throw new EmailNotActiveError(errMsg, response.status, text);
      }
      if (response.status === 400 && (errCode === 'RECIPIENT_NOT_FOUND' || text.includes('Invalid recipient'))) {
        throw new RecipientNotFoundError(errMsg, response.status, text);
      }
      if (response.status === 429) {
        throw new RateLimitedError(errMsg, response.status, text);
      }
      throw new HaiApiError(errMsg, response.status, undefined, errCode, text);
    }

    const data = await response.json() as Record<string, unknown>;
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
    const url = this.makeUrl('/api/v1/email/sign');
    const headers = this.buildAuthHeaders();
    headers['Content-Type'] = 'message/rfc822';

    const body = typeof rawEmail === 'string' ? Buffer.from(rawEmail) : rawEmail;

    const response = await this.fetchWithRetry(url, {
      method: 'POST',
      headers,
      body,
    });

    if (!response.ok) {
      const text = await response.text();
      throw new HaiApiError(`Email sign failed: HTTP ${response.status}`, response.status, undefined, '', text);
    }

    const arrayBuf = await response.arrayBuffer();
    return Buffer.from(arrayBuf);
  }

  /**
   * Send an agent-signed email.
   *
   * @deprecated sendSignedEmail currently delegates to sendEmail. The previous
   * implementation called /api/v1/email/sign (HAI authority key) then POSTed
   * to send-signed, which rejects because the signer ID does not match the
   * authenticated agent. True agent-key local signing will be available when
   * the Rust SDK core (DevEx TASK_017) ships. Use sendEmail directly.
   *
   * @param options - Email options (to, subject, body, attachments, etc.)
   * @returns SendEmailResult with messageId and status.
   */
  async sendSignedEmail(options: SendEmailOptions): Promise<SendEmailResult> {
    // Deprecated: delegates to sendEmail until local agent-key signing
    // is available (DevEx TASK_017). Use sendEmail directly.
    return this.sendEmail(options);
  }

  /**
   * Verify a JACS-signed email via the HAI API.
   *
   * The server extracts the `jacs-signature.json` attachment, validates
   * the cryptographic signature and content hashes, and returns a
   * detailed verification result.
   *
   * @param rawEmail - Raw RFC 5322 email as a Buffer or string.
   * @returns EmailVerificationResultV2 with field-level verification results.
   */
  async verifyEmail(rawEmail: Buffer | string): Promise<EmailVerificationResultV2> {
    const url = this.makeUrl('/api/v1/email/verify');
    const headers = this.buildAuthHeaders();
    headers['Content-Type'] = 'message/rfc822';

    const body = typeof rawEmail === 'string' ? Buffer.from(rawEmail) : rawEmail;

    const response = await this.fetchWithRetry(url, {
      method: 'POST',
      headers,
      body,
    });

    if (!response.ok) {
      const text = await response.text();
      throw new HaiApiError(`Email verify failed: HTTP ${response.status}`, response.status, undefined, '', text);
    }

    const data = await response.json() as Record<string, unknown>;
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
    const params = new URLSearchParams();
    if (options?.limit != null) params.set('limit', String(options.limit));
    if (options?.offset != null) params.set('offset', String(options.offset));
    if (options?.direction) params.set('direction', options.direction);
    if (options?.isRead != null) params.set('is_read', String(options.isRead));
    if (options?.folder) params.set('folder', options.folder);
    if (options?.label) params.set('label', options.label);
    if (options?.hasAttachments != null) params.set('has_attachments', String(options.hasAttachments));
    if (options?.since) params.set('since', options.since);
    if (options?.until) params.set('until', options.until);

    const qs = params.toString();
    const safeAgentId = this.encodePathSegment(this.haiAgentId);
    const url = this.makeUrl(`/api/agents/${safeAgentId}/email/messages${qs ? `?${qs}` : ''}`);

    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: this.buildAuthHeaders(),
    });

    const data = await response.json() as Record<string, unknown>;
    const messages = (data.messages as Array<Record<string, unknown>>) || [];
    return messages.map((m) => this.parseEmailMessage(m));
  }

  /**
   * Mark an email message as read.
   *
   * @param messageId - The message ID to mark as read
   */
  async markRead(messageId: string): Promise<void> {
    const safeAgentId = this.encodePathSegment(this.haiAgentId);
    const safeMessageId = this.encodePathSegment(messageId);
    const url = this.makeUrl(`/api/agents/${safeAgentId}/email/messages/${safeMessageId}/read`);
    await this.fetchWithRetry(url, {
      method: 'POST',
      headers: this.buildAuthHeaders(),
    });
  }

  /**
   * Get email rate limit and status info for this agent.
   *
   * @returns Email status with daily limits and usage
   */
  async getEmailStatus(): Promise<EmailStatus> {
    const safeAgentId = this.encodePathSegment(this.haiAgentId);
    const url = this.makeUrl(`/api/agents/${safeAgentId}/email/status`);
    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: this.buildAuthHeaders(),
    });

    const data = await response.json() as Record<string, unknown>;
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
    const safeAgentId = this.encodePathSegment(this.haiAgentId);
    const safeMessageId = this.encodePathSegment(messageId);
    const url = this.makeUrl(`/api/agents/${safeAgentId}/email/messages/${safeMessageId}`);
    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: this.buildAuthHeaders(),
    });

    const m = await response.json() as Record<string, unknown>;
    return this.parseEmailMessage(m);
  }

  /**
   * Delete an email message.
   *
   * @param messageId - The message ID to delete
   */
  async deleteMessage(messageId: string): Promise<void> {
    const safeAgentId = this.encodePathSegment(this.haiAgentId);
    const safeMessageId = this.encodePathSegment(messageId);
    const url = this.makeUrl(`/api/agents/${safeAgentId}/email/messages/${safeMessageId}`);
    await this.fetchWithRetry(url, {
      method: 'DELETE',
      headers: this.buildAuthHeaders(),
    });
  }

  /**
   * Mark an email message as unread.
   *
   * @param messageId - The message ID to mark as unread
   */
  async markUnread(messageId: string): Promise<void> {
    const safeAgentId = this.encodePathSegment(this.haiAgentId);
    const safeMessageId = this.encodePathSegment(messageId);
    const url = this.makeUrl(`/api/agents/${safeAgentId}/email/messages/${safeMessageId}/unread`);
    await this.fetchWithRetry(url, {
      method: 'POST',
      headers: this.buildAuthHeaders(),
    });
  }

  /**
   * Search email messages.
   *
   * @param options - Search query and pagination options
   * @returns Array of matching email messages
   */
  async searchMessages(options: SearchOptions): Promise<EmailMessage[]> {
    const params = new URLSearchParams();
    params.set('q', options.query);
    if (options.limit != null) params.set('limit', String(options.limit));
    if (options.offset != null) params.set('offset', String(options.offset));
    if (options.direction) params.set('direction', options.direction);
    if (options.fromAddress) params.set('from_address', options.fromAddress);
    if (options.toAddress) params.set('to_address', options.toAddress);
    if (options.isRead != null) params.set('is_read', String(options.isRead));
    if (options.jacsVerified != null) params.set('jacs_verified', String(options.jacsVerified));
    if (options.folder) params.set('folder', options.folder);
    if (options.label) params.set('label', options.label);
    if (options.hasAttachments != null) params.set('has_attachments', String(options.hasAttachments));
    if (options.since) params.set('since', options.since);
    if (options.until) params.set('until', options.until);

    const safeAgentId = this.encodePathSegment(this.haiAgentId);
    const url = this.makeUrl(`/api/agents/${safeAgentId}/email/search?${params.toString()}`);
    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: this.buildAuthHeaders(),
    });

    const data = await response.json() as Record<string, unknown>;
    const messages = (data.messages as Array<Record<string, unknown>>) || [];
    return messages.map((m) => this.parseEmailMessage(m));
  }

  /**
   * Get the count of unread messages.
   *
   * @returns The number of unread messages
   */
  async getUnreadCount(): Promise<number> {
    const safeAgentId = this.encodePathSegment(this.haiAgentId);
    const url = this.makeUrl(`/api/agents/${safeAgentId}/email/unread-count`);
    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: this.buildAuthHeaders(),
    });

    const data = await response.json() as Record<string, unknown>;
    return (data.count as number) || 0;
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
    const safeAgentId = this.encodePathSegment(this.haiAgentId);
    const url = this.makeUrl(`/api/agents/${safeAgentId}/email/forward`);

    const payload: Record<string, unknown> = {
      message_id: options.messageId,
      to: options.to,
    };
    if (options.comment) payload.comment = options.comment;

    const response = await this.fetchWithRetry(url, {
      method: 'POST',
      headers: this.buildAuthHeaders(),
      body: JSON.stringify(payload),
    });

    const data = await response.json() as Record<string, unknown>;
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
    const safeAgentId = this.encodePathSegment(this.haiAgentId);
    const safeMessageId = this.encodePathSegment(messageId);
    const url = this.makeUrl(`/api/agents/${safeAgentId}/email/messages/${safeMessageId}/archive`);
    await this.fetchWithRetry(url, {
      method: 'POST',
      headers: this.buildAuthHeaders(),
    });
  }

  /**
   * Unarchive (restore) an email message.
   *
   * @param messageId - The message ID to unarchive
   */
  async unarchive(messageId: string): Promise<void> {
    const safeAgentId = this.encodePathSegment(this.haiAgentId);
    const safeMessageId = this.encodePathSegment(messageId);
    const url = this.makeUrl(`/api/agents/${safeAgentId}/email/messages/${safeMessageId}/unarchive`);
    await this.fetchWithRetry(url, {
      method: 'POST',
      headers: this.buildAuthHeaders(),
    });
  }

  /**
   * List contacts derived from email message history.
   *
   * @returns Array of Contact objects
   */
  async getContacts(): Promise<Contact[]> {
    const safeAgentId = this.encodePathSegment(this.haiAgentId);
    const url = this.makeUrl(`/api/agents/${safeAgentId}/email/contacts`);
    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: this.buildAuthHeaders(),
    });

    const data = await response.json() as Record<string, unknown>;
    const items = Array.isArray(data) ? data : (data.contacts as Array<Record<string, unknown>>) || [];
    return items.map((c: Record<string, unknown>) => ({
      email: (c.email as string) || '',
      displayName: (c.display_name as string) || undefined,
      lastContact: (c.last_contact as string) || '',
      jacsVerified: (c.jacs_verified as boolean) ?? false,
      reputationTier: (c.reputation_tier as string) || undefined,
    }));
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
    const safeAgentId = this.encodePathSegment(this.haiAgentId);
    const url = this.makeUrl(`/api/agents/${safeAgentId}/email/templates`);

    const payload: Record<string, unknown> = { name: options.name };
    if (options.howToSend != null) payload.how_to_send = options.howToSend;
    if (options.howToRespond != null) payload.how_to_respond = options.howToRespond;
    if (options.goal != null) payload.goal = options.goal;
    if (options.rules != null) payload.rules = options.rules;

    const response = await this.fetchWithRetry(url, {
      method: 'POST',
      headers: this.buildAuthHeaders(),
      body: JSON.stringify(payload),
    });

    const data = await response.json() as Record<string, unknown>;
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
    const params = new URLSearchParams();
    if (options?.limit != null) params.set('limit', String(options.limit));
    if (options?.offset != null) params.set('offset', String(options.offset));
    if (options?.q) params.set('q', options.q);

    const qs = params.toString();
    const safeAgentId = this.encodePathSegment(this.haiAgentId);
    const url = this.makeUrl(`/api/agents/${safeAgentId}/email/templates${qs ? `?${qs}` : ''}`);

    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: this.buildAuthHeaders(),
    });

    const data = await response.json() as Record<string, unknown>;
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
    const safeAgentId = this.encodePathSegment(this.haiAgentId);
    const safeTemplateId = this.encodePathSegment(templateId);
    const url = this.makeUrl(`/api/agents/${safeAgentId}/email/templates/${safeTemplateId}`);

    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: this.buildAuthHeaders(),
    });

    const data = await response.json() as Record<string, unknown>;
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
    const safeAgentId = this.encodePathSegment(this.haiAgentId);
    const safeTemplateId = this.encodePathSegment(templateId);
    const url = this.makeUrl(`/api/agents/${safeAgentId}/email/templates/${safeTemplateId}`);

    const payload: Record<string, unknown> = {};
    if (options.name !== undefined) payload.name = options.name;
    // For nullable fields: undefined = omit (don't update), null = send null (clear), string = set value
    if (options.howToSend !== undefined) payload.how_to_send = options.howToSend;
    if (options.howToRespond !== undefined) payload.how_to_respond = options.howToRespond;
    if (options.goal !== undefined) payload.goal = options.goal;
    if (options.rules !== undefined) payload.rules = options.rules;

    const response = await this.fetchWithRetry(url, {
      method: 'PUT',
      headers: this.buildAuthHeaders(),
      body: JSON.stringify(payload),
    });

    const data = await response.json() as Record<string, unknown>;
    return this.parseEmailTemplate(data);
  }

  /**
   * Delete an email template (soft delete).
   *
   * @param templateId - The template ID to delete
   */
  async deleteEmailTemplate(templateId: string): Promise<void> {
    const safeAgentId = this.encodePathSegment(this.haiAgentId);
    const safeTemplateId = this.encodePathSegment(templateId);
    const url = this.makeUrl(`/api/agents/${safeAgentId}/email/templates/${safeTemplateId}`);

    await this.fetchWithRetry(url, {
      method: 'DELETE',
      headers: this.buildAuthHeaders(),
    });
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

    const safeJacsId = this.encodePathSegment(jacsId);
    const safeVersion = this.encodePathSegment(version);
    const url = this.makeUrl(`/jacs/v1/agents/${safeJacsId}/keys/${safeVersion}`);
    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: { 'Content-Type': 'application/json' },
    });

    const warning = response.headers.get('Warning');
    if (warning) {
      console.warn(`HAI key service: ${warning}`);
    }

    const data = await response.json() as Record<string, unknown>;
    const result: PublicKeyInfo = {
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

    const safeHash = this.encodePathSegment(publicKeyHash);
    const url = this.makeUrl(`/jacs/v1/keys/by-hash/${safeHash}`);
    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: { 'Content-Type': 'application/json' },
    });

    const data = await response.json() as Record<string, unknown>;
    const result: PublicKeyInfo = {
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

    const safeEmail = this.encodePathSegment(email);
    const url = this.makeUrl(`/api/agents/keys/${safeEmail}`);
    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: { 'Content-Type': 'application/json' },
    });

    const data = await response.json() as Record<string, unknown>;
    const result: PublicKeyInfo = {
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

    const safeDomain = this.encodePathSegment(domain);
    const url = this.makeUrl(`/jacs/v1/agents/by-domain/${safeDomain}`);
    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: { 'Content-Type': 'application/json' },
    });

    const data = await response.json() as Record<string, unknown>;
    const result: PublicKeyInfo = {
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
    const safeJacsId = this.encodePathSegment(jacsId);
    const url = this.makeUrl(`/jacs/v1/agents/${safeJacsId}/keys`);
    const response = await this.fetchWithRetry(url, {
      method: 'GET',
      headers: { 'Content-Type': 'application/json' },
    });

    const data = await response.json() as Record<string, unknown>;
    const rawKeys = (data.keys as Array<Record<string, unknown>>) || [];
    const keys = rawKeys.map((k) => ({
      jacsId: (k.jacs_id as string) || '',
      version: (k.version as string) || '',
      publicKey: (k.public_key as string) || '',
      publicKeyRawB64: (k.public_key_raw_b64 as string) || '',
      algorithm: (k.algorithm as string) || '',
      publicKeyHash: (k.public_key_hash as string) || '',
      status: (k.status as string) || '',
      dnsVerified: (k.dns_verified as boolean) ?? false,
      createdAt: (k.created_at as string) || '',
    }));

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

    // Level 1: JACS signature verification
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

    // Level 3: Server attestation
    try {
      const safeDocJacsId = this.encodePathSegment(String(doc.jacsId || ''));
      const attestUrl = this.makeUrl(`/api/v1/agents/${safeDocJacsId}/verify`);
      const resp = await this.fetchWithRetry(attestUrl, {
        method: 'GET',
        headers: { 'Content-Type': 'application/json' },
      });
      const data = await resp.json() as Record<string, unknown>;
      result.haiRegistered = (data.registered as boolean) ?? false;
      result.dnsVerified = (data.dns_verified as boolean) ?? false;
      result.badgeLevel = (data.badge_level as VerificationResult['badgeLevel']) || 'none';
    } catch (e) {
      result.errors.push(`Server attestation check failed: ${(e as Error).message}`);
    }

    return result;
  }
}
