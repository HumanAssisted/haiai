// @haiai/wasm — ergonomic TS wrapper around the wasm-bindgen output
// from rust/haiai-wasm/.
//
// HAIAI_WASM_PRD §4.4 — `BrowserAgent` (lifecycle + local crypto) +
// `BrowserHaiClient` (HTTP wrappers + AsyncIterableIterator event
// stream). Errors map to `HaiaiWasmError` with stable `.code`.

import init, {
  initHaiaiWasm as wasmInit,
  version as wasmVersion,
  about as wasmAbout,
  BrowserAgentHandle,
  EventStreamHandle,
} from "./haiai_wasm.js";

// `@jacs/wasm` is the source of truth for browser-side encrypted-agent
// persistence. HAIAI does not vendor its own localStorage layer per
// CLAUDE.md Rule 1 ("No JACS reimplementation"); save/load below
// delegate every write/read through this surface so policy (key
// namespacing, quota typing, validate_encrypted_material_shape) stays
// in one place.
import { localStore as jacsLocalStore } from "@jacs/wasm";

import type {
  Algorithm,
  HaiEvent,
  EventStreamTransport,
  HaiaiWasmMetrics,
  HelloResult,
  RegisterAgentOptions,
  RegistrationResult,
  RotationResult,
  SendEmailOptions,
  SendEmailResult,
  ListMessagesOptions,
  EmailMessage,
  RawEmailResponse,
  EmailStatus,
  Contact,
  CreateEmailTemplateOptions,
  UpdateEmailTemplateOptions,
  EmailTemplate,
  SearchOptions,
  PublicKeyInfo,
  DocumentVerificationResult,
  AgentVerificationResult,
  VerifyAgentDocumentRequest,
} from "./types.js";

let initialized = false;

/**
 * Initialize the wasm runtime. Idempotent — safe to call multiple times.
 * Awaits both the wasm-pack `init()` (loads `.wasm`) and the haiai-wasm
 * `init_haiai_wasm` (installs panic hook).
 */
export async function initHaiaiWasm(): Promise<void> {
  if (initialized) return;
  await init();
  wasmInit();
  initialized = true;
}

/** Package version. Matches rust/haiai-wasm/Cargo.toml::version. */
export function version(): string {
  return wasmVersion();
}

/** Build descriptor. */
export function about(): string {
  return wasmAbout();
}

/**
 * Typed error surface. Every `@haiai/wasm` rejection is a
 * `HaiaiWasmError` with a stable `.code` (PRD §3.1).
 */
export class HaiaiWasmError extends Error {
  readonly code: string;
  readonly details?: unknown;
  constructor(code: string, message: string, details?: unknown) {
    super(message);
    this.code = code;
    this.details = details;
    this.name = "HaiaiWasmError";
  }
}

/**
 * Wrap a thrown JS value as a `HaiaiWasmError`. The wasm-bindgen layer
 * throws `Error` whose `.message` is a JSON-encoded
 * `{ code, message, details? }` payload — parse it back into typed
 * fields here.
 */
function wrapWasmError(err: unknown): HaiaiWasmError {
  if (err instanceof HaiaiWasmError) return err;
  const message = err instanceof Error ? err.message : String(err);
  try {
    const parsed = JSON.parse(message);
    if (parsed && typeof parsed === "object" && typeof parsed.code === "string") {
      return new HaiaiWasmError(parsed.code, parsed.message ?? message, parsed.details);
    }
  } catch {
    // Fall through — message wasn't JSON.
  }
  return new HaiaiWasmError("Internal", message);
}

/** Promisified invocation that catches JsValue rejections. */
async function safe<T>(fn: () => Promise<T> | T): Promise<T> {
  try {
    return await fn();
  } catch (e) {
    throw wrapWasmError(e);
  }
}

function safeSync<T>(fn: () => T): T {
  try {
    return fn();
  } catch (e) {
    throw wrapWasmError(e);
  }
}

// ---------------------------------------------------------------------------
// BrowserAgent (Task 032) — lifecycle + local crypto.
// ---------------------------------------------------------------------------

/** Options accepted by every `BrowserAgent.*` constructor. */
export interface BrowserAgentInit {
  /** HAI API base URL. Defaults to `https://hai.ai` when omitted. */
  baseUrl?: string;
}

export class BrowserAgent {
  /** The wasm-bindgen handle. Exposed for low-level callers; prefer `client`. */
  readonly handle: BrowserAgentHandle;
  readonly client: BrowserHaiClient;

  private constructor(handle: BrowserAgentHandle) {
    this.handle = handle;
    this.client = new BrowserHaiClientImpl(handle);
  }

  static async createEphemeral(
    algorithm: Algorithm,
    init?: BrowserAgentInit,
  ): Promise<BrowserAgent> {
    await initHaiaiWasm();
    const handle = safeSync(() =>
      BrowserAgentHandle.createEphemeral(algorithm, init?.baseUrl ?? null),
    );
    return new BrowserAgent(handle);
  }

  static async importEncrypted(
    materialJson: string,
    password: string,
    init?: BrowserAgentInit,
  ): Promise<BrowserAgent> {
    await initHaiaiWasm();
    const handle = safeSync(() =>
      BrowserAgentHandle.importEncrypted(materialJson, password, init?.baseUrl ?? null),
    );
    return new BrowserAgent(handle);
  }

  /**
   * Load an encrypted agent previously persisted via
   * `BrowserAgent.save(storageKey, password)` from browser localStorage
   * (Issue 003). Returns `null` when the key is absent so callers can
   * branch on first-run flows without try/catching.
   *
   * Errors:
   *   - `InvalidPassword` — wrong password.
   *   - `MalformedDocument` — stored blob is not a valid AgentMaterial.
   *   - `Internal` — localStorage unavailable.
   */
  static async load(
    storageKey: string,
    options: { password: string } & BrowserAgentInit,
  ): Promise<BrowserAgent | null> {
    await initHaiaiWasm();
    if (!storageKey) {
      throw new HaiaiWasmError(
        "MalformedDocument",
        "load(storageKey, {password}) requires a non-empty storageKey",
      );
    }
    if (!options || typeof options.password !== "string" || options.password === "") {
      throw new HaiaiWasmError(
        "MalformedDocument",
        "load(storageKey, {password}) requires a non-empty password option",
      );
    }
    let materialJson: string | null;
    try {
      materialJson = jacsLocalStore.loadEncryptedAgent(storageKey);
    } catch (e) {
      throw wrapWasmError(e);
    }
    if (materialJson === null) {
      return null;
    }
    return BrowserAgent.importEncrypted(materialJson, options.password, {
      baseUrl: options.baseUrl,
    });
  }

  static async publicOnly(
    jacsId: string,
    publicKeyBase64: string,
    algorithm: Algorithm,
    init?: BrowserAgentInit,
  ): Promise<BrowserAgent> {
    await initHaiaiWasm();
    const handle = safeSync(() =>
      BrowserAgentHandle.publicOnly(jacsId, publicKeyBase64, algorithm, init?.baseUrl ?? null),
    );
    return new BrowserAgent(handle);
  }

  /** Sign a JSON-encodable payload. Returns the signed document. */
  sign(payload: unknown): unknown {
    const signedJson = safeSync(() => this.handle.signMessageJson(JSON.stringify(payload)));
    return JSON.parse(signedJson);
  }

  /** Verify a signed JACS document. Returns `{ valid, status, ... }`. */
  verify(signed: unknown): unknown {
    return safeSync(() => this.handle.verifyJson(JSON.stringify(signed)));
  }

  signAgreement(agreement: unknown, role?: string): unknown {
    const out = safeSync(() =>
      this.handle.signAgreement(JSON.stringify(agreement), role ?? null),
    );
    return JSON.parse(out);
  }

  verifyAgreement(agreement: unknown): unknown {
    return safeSync(() => this.handle.verifyAgreement(JSON.stringify(agreement)));
  }

  /** Drop the in-memory signer. Subsequent sign attempts throw `Locked`. */
  clearSecrets(): void {
    safeSync(() => this.handle.clearSecrets());
  }

  isUnlocked(): boolean {
    return safeSync(() => this.handle.isUnlocked());
  }

  exportAgent(): unknown {
    return JSON.parse(safeSync(() => this.handle.exportAgent()));
  }

  /**
   * Encrypt the agent under `password` and return the `AgentMaterial`
   * JSON string — the same shape `BrowserAgent.importEncrypted` accepts
   * (Issue 003). Suitable for handing to any storage layer.
   */
  exportEncrypted(password: string): string {
    return safeSync(() => this.handle.exportEncrypted(password));
  }

  /**
   * Persist the agent to browser localStorage under `storageKey`,
   * encrypted with `password`. Delegates the storage write to
   * `@jacs/wasm`'s `localStore.saveEncryptedAgent` so policy (key
   * namespacing, quota typing, encrypted-shape validation) stays in
   * the canonical layer (Issue 003).
   *
   * Errors:
   *   - `Locked` — signer was cleared / publicOnly handle.
   *   - `Internal` — localStorage unavailable or quota exceeded
   *     (typed by `@jacs/wasm` and re-thrown via `wrapWasmError`).
   */
  save(storageKey: string, password: string): void {
    if (!storageKey) {
      throw new HaiaiWasmError(
        "MalformedDocument",
        "save(storageKey, password) requires a non-empty storageKey",
      );
    }
    if (!password) {
      throw new HaiaiWasmError(
        "MalformedDocument",
        "save(storageKey, password) requires a non-empty password",
      );
    }
    const materialJson = this.exportEncrypted(password);
    try {
      jacsLocalStore.saveEncryptedAgent(storageKey, materialJson);
    } catch (e) {
      throw wrapWasmError(e);
    }
  }

  publicKeyBase64(): string {
    return safeSync(() => this.handle.getPublicKeyBase64());
  }

  /**
   * Set the `@hai.ai` email address used as the RFC 5322 `From:` header by
   * `sendSignedEmail`. Required when the caller has already established an
   * agent identity outside the wasm wrapper (e.g. server-side registration
   * or a restored agent) and wants to send signed mail without re-issuing
   * a `register` HTTP exchange.
   *
   * Mirrors the native `HaiClient::set_agent_email`. Throws
   * `MalformedDocument` if `email` is empty.
   */
  setAgentEmail(email: string): void {
    safeSync(() => this.handle.setAgentEmail(email));
  }

  algorithm(): Algorithm {
    return safeSync(() => this.handle.algorithm()) as Algorithm;
  }

  jacsId(): string {
    return safeSync(() => this.handle.jacsId());
  }

  metrics(): HaiaiWasmMetrics {
    return safeSync(() => this.handle.metrics() as HaiaiWasmMetrics);
  }

  // ── Low-level local helpers (Task 028) ──

  canonicalJson(value: unknown): string {
    return safeSync(() => this.handle.canonicalJson(JSON.stringify(value)));
  }

  buildAuthHeader(timestampSeconds: number, nonce: string): string {
    return safeSync(() => this.handle.buildAuthHeader(BigInt(timestampSeconds), nonce));
  }

  generateVerifyLink(document: unknown, baseUrl?: string): string {
    return safeSync(() =>
      this.handle.generateVerifyLink(JSON.stringify(document), baseUrl ?? null),
    );
  }
}

// ---------------------------------------------------------------------------
// BrowserHaiClient (Task 033) — typed wrappers + AsyncIterableIterator.
// ---------------------------------------------------------------------------

export interface EventStreamOptions {
  transport: EventStreamTransport;
  /** Absolute URL. Defaults to the agent's base URL with the canonical
   * SSE / WS path appended. */
  url?: string;
  /** Override the auth header. Computed on-the-fly when omitted. */
  authHeader?: string;
}

export interface BrowserHaiClient {
  // Registration & Identity
  hello(includeTest?: boolean): Promise<HelloResult>;
  register(options: RegisterAgentOptions): Promise<RegistrationResult>;
  rotateKeys(registerWithHai?: boolean): Promise<RotationResult>;
  verifyStatus(agentId?: string): Promise<AgentVerificationResult>;
  updateUsername(agentId: string, newUsername: string): Promise<unknown>;
  deleteUsername(agentId: string): Promise<unknown>;

  // Email send + inbox
  sendEmail(options: SendEmailOptions): Promise<SendEmailResult>;
  sendSignedEmail(options: SendEmailOptions): Promise<SendEmailResult>;
  listMessages(options: ListMessagesOptions): Promise<EmailMessage[]>;
  getMessage(messageId: string): Promise<EmailMessage>;
  getRawEmail(messageId: string): Promise<RawEmailResponse>;
  markRead(messageId: string): Promise<void>;
  markUnread(messageId: string): Promise<void>;
  deleteMessage(messageId: string): Promise<void>;
  archive(messageId: string): Promise<void>;
  unarchive(messageId: string): Promise<void>;
  getUnreadCount(): Promise<number>;
  getEmailStatus(): Promise<EmailStatus>;

  // Email reply / forward / search / contacts
  reply(messageId: string, body: string, subject?: string): Promise<SendEmailResult>;
  forward(messageId: string, to: string, comment?: string): Promise<SendEmailResult>;
  searchMessages(options: SearchOptions): Promise<EmailMessage[]>;
  contacts(): Promise<Contact[]>;

  // Email templates + raw signing
  createEmailTemplate(options: CreateEmailTemplateOptions): Promise<EmailTemplate>;
  listEmailTemplates(options?: unknown): Promise<EmailTemplate[]>;
  getEmailTemplate(templateId: string): Promise<EmailTemplate>;
  updateEmailTemplate(templateId: string, options: UpdateEmailTemplateOptions): Promise<EmailTemplate>;
  deleteEmailTemplate(templateId: string): Promise<void>;
  signEmailRaw(rawEmailB64: string): Promise<string>;
  verifyEmailRaw(rawEmailB64: string): Promise<unknown>;

  // Key & Verification
  fetchServerKeys(): Promise<unknown>;
  fetchRemoteKey(jacsId: string, version: string): Promise<PublicKeyInfo>;
  fetchKeyByHash(hash: string): Promise<PublicKeyInfo>;
  fetchKeyByEmail(email: string): Promise<PublicKeyInfo>;
  fetchKeyByDomain(domain: string): Promise<PublicKeyInfo>;
  fetchAllKeys(jacsId: string): Promise<unknown>;
  verifyDocument(documentJson: string): Promise<DocumentVerificationResult>;
  getVerification(agentId: string): Promise<AgentVerificationResult>;
  verifyAgentDocument(request: VerifyAgentDocumentRequest): Promise<unknown>;

  // Benchmark RPC
  benchmark(name?: string, tier?: string): Promise<unknown>;
  freeRun(transport?: EventStreamTransport): Promise<unknown>;
  proRun(transport?: EventStreamTransport, pollIntervalMs?: number, pollTimeoutMs?: number): Promise<unknown>;
  dnsCertifiedRun(transport?: EventStreamTransport, pollIntervalMs?: number, pollTimeoutMs?: number): Promise<unknown>;
  submitResponse(jobId: string, message: string, metadata?: unknown, processingTimeMs?: number): Promise<unknown>;

  // Event streams.
  //
  // `connectSse()` / `connectWs()` (no args) use the agent's configured
  // baseUrl + canonical paths + a freshly-signed auth header
  // (Issue 005). The overloads accepting `(url, authHeader)` remain as
  // a low-level escape hatch for callers who want to override either.
  // `eventStream({transport})` delegates to whichever variant matches
  // the supplied options.
  connectSse(url?: string, authHeader?: string): AsyncIterableIterator<HaiEvent>;
  connectWs(url?: string, authHeader?: string): AsyncIterableIterator<HaiEvent>;
  eventStream(options: EventStreamOptions): AsyncIterableIterator<HaiEvent>;
}

class BrowserHaiClientImpl implements BrowserHaiClient {
  constructor(private readonly handle: BrowserAgentHandle) {}

  hello(includeTest = false) {
    return safe(() => this.handle.hello(includeTest) as Promise<HelloResult>);
  }
  register(options: RegisterAgentOptions) {
    return safe(() =>
      this.handle.register(JSON.stringify(options)) as Promise<RegistrationResult>,
    );
  }
  rotateKeys(registerWithHai?: boolean) {
    return safe(() =>
      this.handle.rotateKeys(registerWithHai ?? null) as Promise<RotationResult>,
    );
  }
  verifyStatus(agentId?: string) {
    return safe(() =>
      this.handle.verifyStatus(agentId ?? null) as Promise<AgentVerificationResult>,
    );
  }
  updateUsername(agentId: string, newUsername: string) {
    return safe(() => this.handle.updateUsername(agentId, newUsername));
  }
  deleteUsername(agentId: string) {
    return safe(() => this.handle.deleteUsername(agentId));
  }

  sendEmail(options: SendEmailOptions) {
    return safe(() =>
      this.handle.sendEmail(JSON.stringify(options)) as Promise<SendEmailResult>,
    );
  }
  sendSignedEmail(options: SendEmailOptions) {
    return safe(() =>
      this.handle.sendSignedEmail(JSON.stringify(options)) as Promise<SendEmailResult>,
    );
  }
  listMessages(options: ListMessagesOptions) {
    return safe(() =>
      this.handle.listMessages(JSON.stringify(options)) as Promise<EmailMessage[]>,
    );
  }
  getMessage(messageId: string) {
    return safe(() => this.handle.getMessage(messageId) as Promise<EmailMessage>);
  }
  getRawEmail(messageId: string) {
    return safe(() => this.handle.getRawEmail(messageId) as Promise<RawEmailResponse>);
  }
  markRead(messageId: string) {
    return safe(() => this.handle.markRead(messageId));
  }
  markUnread(messageId: string) {
    return safe(() => this.handle.markUnread(messageId));
  }
  deleteMessage(messageId: string) {
    return safe(() => this.handle.deleteMessage(messageId));
  }
  archive(messageId: string) {
    return safe(() => this.handle.archive(messageId));
  }
  unarchive(messageId: string) {
    return safe(() => this.handle.unarchive(messageId));
  }
  getUnreadCount() {
    return safe(() => this.handle.getUnreadCount());
  }
  getEmailStatus() {
    return safe(() => this.handle.getEmailStatus() as Promise<EmailStatus>);
  }

  reply(messageId: string, body: string, subject?: string) {
    return safe(() =>
      this.handle.reply(messageId, body, subject ?? null) as Promise<SendEmailResult>,
    );
  }
  forward(messageId: string, to: string, comment?: string) {
    return safe(() =>
      this.handle.forward(messageId, to, comment ?? null) as Promise<SendEmailResult>,
    );
  }
  searchMessages(options: SearchOptions) {
    return safe(() =>
      this.handle.searchMessages(JSON.stringify(options)) as Promise<EmailMessage[]>,
    );
  }
  contacts() {
    return safe(() => this.handle.contacts() as Promise<Contact[]>);
  }

  createEmailTemplate(options: CreateEmailTemplateOptions) {
    return safe(() =>
      this.handle.createEmailTemplate(JSON.stringify(options)) as Promise<EmailTemplate>,
    );
  }
  listEmailTemplates(options?: unknown) {
    const json = options === undefined ? null : JSON.stringify(options);
    return safe(() => this.handle.listEmailTemplates(json) as Promise<EmailTemplate[]>);
  }
  getEmailTemplate(templateId: string) {
    return safe(() => this.handle.getEmailTemplate(templateId) as Promise<EmailTemplate>);
  }
  updateEmailTemplate(templateId: string, options: UpdateEmailTemplateOptions) {
    return safe(() =>
      this.handle.updateEmailTemplate(templateId, JSON.stringify(options)) as Promise<EmailTemplate>,
    );
  }
  deleteEmailTemplate(templateId: string) {
    return safe(() => this.handle.deleteEmailTemplate(templateId));
  }
  signEmailRaw(rawEmailB64: string) {
    return safe(() => this.handle.signEmailRaw(rawEmailB64));
  }
  verifyEmailRaw(rawEmailB64: string) {
    return safe(() => this.handle.verifyEmailRaw(rawEmailB64));
  }

  fetchServerKeys() {
    return safe(() => this.handle.fetchServerKeys());
  }
  fetchRemoteKey(jacsId: string, ver: string) {
    return safe(() => this.handle.fetchRemoteKey(jacsId, ver) as Promise<PublicKeyInfo>);
  }
  fetchKeyByHash(hash: string) {
    return safe(() => this.handle.fetchKeyByHash(hash) as Promise<PublicKeyInfo>);
  }
  fetchKeyByEmail(email: string) {
    return safe(() => this.handle.fetchKeyByEmail(email) as Promise<PublicKeyInfo>);
  }
  fetchKeyByDomain(domain: string) {
    return safe(() => this.handle.fetchKeyByDomain(domain) as Promise<PublicKeyInfo>);
  }
  fetchAllKeys(jacsId: string) {
    return safe(() => this.handle.fetchAllKeys(jacsId));
  }
  verifyDocument(documentJson: string) {
    return safe(
      () => this.handle.verifyDocument(documentJson) as Promise<DocumentVerificationResult>,
    );
  }
  getVerification(agentId: string) {
    return safe(
      () => this.handle.getVerification(agentId) as Promise<AgentVerificationResult>,
    );
  }
  verifyAgentDocument(request: VerifyAgentDocumentRequest) {
    return safe(() => this.handle.verifyAgentDocument(JSON.stringify(request)));
  }

  benchmark(name?: string, tier?: string) {
    return safe(() => this.handle.benchmark(name ?? null, tier ?? null));
  }
  freeRun(transport?: EventStreamTransport) {
    return safe(() => this.handle.freeRun(transport ?? null));
  }
  proRun(
    transport?: EventStreamTransport,
    pollIntervalMs?: number,
    pollTimeoutMs?: number,
  ) {
    return safe(() =>
      this.handle.proRun(transport ?? null, pollIntervalMs ?? null, pollTimeoutMs ?? null),
    );
  }
  dnsCertifiedRun(
    transport?: EventStreamTransport,
    pollIntervalMs?: number,
    pollTimeoutMs?: number,
  ) {
    return safe(() =>
      this.handle.dnsCertifiedRun(
        transport ?? null,
        pollIntervalMs ?? null,
        pollTimeoutMs ?? null,
      ),
    );
  }
  submitResponse(
    jobId: string,
    message: string,
    metadata?: unknown,
    processingTimeMs?: number,
  ) {
    const metadataJson = metadata === undefined ? null : JSON.stringify(metadata);
    return safe(() =>
      this.handle.submitResponse(jobId, message, metadataJson, processingTimeMs ?? 0),
    );
  }

  /**
   * Convenience wrapper around `eventStream({ transport: "sse" })`.
   * With no args, uses the agent-side `BrowserAgentHandle.connectSse`
   * which derives URL + auth from the handle (Issue 005). With explicit
   * `(url, authHeader)`, behaves like the low-level
   * `EventStreamHandle.openSse` escape hatch.
   */
  connectSse(url?: string, authHeader?: string): AsyncIterableIterator<HaiEvent> {
    return this.eventStream({ transport: "sse", url, authHeader });
  }

  /**
   * Convenience wrapper around `eventStream({ transport: "ws" })`.
   * See `connectSse` for the (no args) vs (url, authHeader) split.
   */
  connectWs(url?: string, authHeader?: string): AsyncIterableIterator<HaiEvent> {
    return this.eventStream({ transport: "ws", url, authHeader });
  }

  /**
   * Open an event stream and return an `AsyncIterableIterator<HaiEvent>`.
   *
   * The TS wrapper owns the `EventStreamHandle` lifecycle and closes it
   * when the iterator is exhausted or the caller breaks out of the
   * `for await` loop.
   *
   * Default path (no `url`/`authHeader`): delegates to the agent-side
   * `BrowserAgentHandle.connectSse` / `connectWs` (Issue 005) so the URL
   * and auth come from the handle. Override path: when `url` is
   * supplied, opens an `EventStreamHandle` against that URL with the
   * (also supplied) `authHeader`.
   */
  eventStream(options: EventStreamOptions): AsyncIterableIterator<HaiEvent> {
    const transport = options.transport;
    const overrideUrl = options.url;
    const overrideAuth = options.authHeader;
    let handle: EventStreamHandle | null = null;
    let done = false;

    const ensureOpen = async () => {
      if (handle !== null) return;
      try {
        if (overrideUrl !== undefined && overrideUrl !== "") {
          // Low-level escape hatch — caller takes responsibility for
          // both URL and auth header.
          handle =
            transport === "ws"
              ? await EventStreamHandle.openWs(overrideUrl, overrideAuth ?? "")
              : await EventStreamHandle.openSse(overrideUrl, overrideAuth ?? "");
        } else {
          // Default path — agent-side connector derives URL + auth.
          handle =
            transport === "ws"
              ? await this.handle.connectWs()
              : await this.handle.connectSse();
        }
      } catch (e) {
        throw wrapWasmError(e);
      }
    };

    const iter: AsyncIterableIterator<HaiEvent> = {
      [Symbol.asyncIterator]() {
        return iter;
      },
      async next(): Promise<IteratorResult<HaiEvent>> {
        if (done) return { value: undefined as unknown as HaiEvent, done: true };
        await ensureOpen();
        if (!handle) {
          done = true;
          return { value: undefined as unknown as HaiEvent, done: true };
        }
        try {
          const ev = (await handle.nextEvent()) as HaiEvent | null;
          if (ev === null) {
            done = true;
            await handle.close();
            handle = null;
            return { value: undefined as unknown as HaiEvent, done: true };
          }
          return { value: ev, done: false };
        } catch (e) {
          done = true;
          if (handle) {
            try {
              await handle.close();
            } catch {
              // best-effort
            }
            handle = null;
          }
          throw wrapWasmError(e);
        }
      },
      async return(): Promise<IteratorResult<HaiEvent>> {
        done = true;
        if (handle) {
          try {
            await handle.close();
          } catch {
            // best-effort
          }
          handle = null;
        }
        return { value: undefined as unknown as HaiEvent, done: true };
      },
    };
    return iter;
  }
}
