// Hand-maintained stub mirroring the wasm-bindgen exports of
// `rust/haiai-wasm/` (HAIAI_WASM_PRD §4.3 / §4.10).
//
// Used only by `tsc --noEmit` from a fresh checkout — at publish time
// `finalize-pkg.sh` produces the real `pkg/haiai_wasm.d.ts` from
// `wasm-pack build` and ships that instead. Keep this stub in sync
// with the wasm-bindgen exports surface (Tasks 021-029) in
// rust/haiai-wasm/src/browser_agent.rs.

/** Initialize the wasm runtime. Idempotent. */
export function initHaiaiWasm(): void;

/** Package version string (matches rust/haiai-wasm/Cargo.toml::version). */
export function version(): string;

/** One-line build descriptor. */
export function about(): string;

/**
 * Stateful browser agent handle. Owns a CoreAgent + HaiClient and
 * exposes the JS surface promised by PRD §4.3.
 *
 * Lifecycle constructors are static (`createEphemeral`,
 * `importEncrypted`, `publicOnly`). Local crypto + HAI HTTP wrappers
 * are instance methods. Each fallible method rejects with a JSON-
 * encoded `{ code, message, details? }` payload — the TS wrapper
 * (`index.ts::BrowserAgent`) catches and rethrows as `HaiaiWasmError`.
 */
export class BrowserAgentHandle {
  free(): void;

  // ── Lifecycle ──
  static createEphemeral(algorithm: string, baseUrl?: string | null): BrowserAgentHandle;
  static importEncrypted(materialJson: string, password: string, baseUrl?: string | null): BrowserAgentHandle;
  static publicOnly(jacsId: string, publicKeyBase64: string, algorithm: string, baseUrl?: string | null): BrowserAgentHandle;
  clearSecrets(): void;
  isUnlocked(): boolean;
  exportAgent(): string;
  exportEncrypted(password: string): string;
  getPublicKeyBase64(): string;
  algorithm(): string;
  jacsId(): string;

  // ── Local crypto ──
  signMessageJson(dataJson: string): string;
  verifyJson(signedJson: string): unknown;
  signAgreement(agreementJson: string, role?: string | null): string;
  verifyAgreement(agreementJson: string): unknown;

  // ── Local helpers ──
  canonicalJson(valueJson: string): string;
  buildAuthHeader(ts: bigint, nonce: string): string;
  generateVerifyLink(documentJson: string, baseUrl?: string | null): string;

  // ── Metrics ──
  metrics(): unknown;

  // ── Registration & Identity ──
  hello(includeTest: boolean): Promise<unknown>;
  register(optionsJson: string): Promise<unknown>;
  rotateKeys(registerWithHai?: boolean | null): Promise<unknown>;
  verifyStatus(agentId?: string | null): Promise<unknown>;
  updateUsername(agentId: string, newUsername: string): Promise<unknown>;
  deleteUsername(agentId: string): Promise<unknown>;

  // ── Email send + inbox ──
  sendEmail(optionsJson: string): Promise<unknown>;
  sendSignedEmail(optionsJson: string): Promise<unknown>;
  listMessages(optionsJson: string): Promise<unknown>;
  getMessage(messageId: string): Promise<unknown>;
  getRawEmail(messageId: string): Promise<unknown>;
  markRead(messageId: string): Promise<void>;
  markUnread(messageId: string): Promise<void>;
  deleteMessage(messageId: string): Promise<void>;
  archive(messageId: string): Promise<void>;
  unarchive(messageId: string): Promise<void>;
  getUnreadCount(): Promise<number>;
  getEmailStatus(): Promise<unknown>;

  // ── Email reply / forward / search / contacts ──
  reply(messageId: string, body: string, subject?: string | null): Promise<unknown>;
  forward(messageId: string, to: string, comment?: string | null): Promise<unknown>;
  searchMessages(optionsJson: string): Promise<unknown>;
  contacts(): Promise<unknown>;

  // ── Email templates + raw signing ──
  createEmailTemplate(optionsJson: string): Promise<unknown>;
  listEmailTemplates(optionsJson?: string | null): Promise<unknown>;
  getEmailTemplate(templateId: string): Promise<unknown>;
  updateEmailTemplate(templateId: string, optionsJson: string): Promise<unknown>;
  deleteEmailTemplate(templateId: string): Promise<void>;
  signEmailRaw(rawEmailB64: string): Promise<string>;
  verifyEmailRaw(rawEmailB64: string): Promise<unknown>;

  // ── Key & Verification ──
  fetchServerKeys(): Promise<unknown>;
  fetchRemoteKey(jacsId: string, version: string): Promise<unknown>;
  fetchKeyByHash(hash: string): Promise<unknown>;
  fetchKeyByEmail(email: string): Promise<unknown>;
  fetchKeyByDomain(domain: string): Promise<unknown>;
  fetchAllKeys(jacsId: string): Promise<unknown>;
  verifyDocument(documentJson: string): Promise<unknown>;
  getVerification(agentId: string): Promise<unknown>;
  verifyAgentDocument(requestJson: string): Promise<unknown>;

  // ── Benchmark RPC ──
  benchmark(name?: string | null, tier?: string | null): Promise<unknown>;
  freeRun(transport?: string | null): Promise<unknown>;
  proRun(transport?: string | null, pollIntervalMs?: number | null, pollTimeoutMs?: number | null): Promise<unknown>;
  dnsCertifiedRun(transport?: string | null, pollIntervalMs?: number | null, pollTimeoutMs?: number | null): Promise<unknown>;
  submitResponse(jobId: string, message: string, metadataJson: string | null | undefined, processingTimeMs: number): Promise<unknown>;

  // ── Event-stream connectors (Issue 005) ──
  // Open a stream using the agent's own baseUrl + a freshly-signed
  // auth header. Returns an `EventStreamHandle` ready to iterate via
  // `nextEvent()`.
  connectSse(): Promise<EventStreamHandle>;
  connectWs(): Promise<EventStreamHandle>;
}

/**
 * SSE / WS event stream handle (Task 029). Open via the static
 * constructors; drive via `nextEvent` until it returns `null`.
 */
export class EventStreamHandle {
  free(): void;
  static openSse(url: string, authHeader: string): Promise<EventStreamHandle>;
  static openWs(baseWsUrl: string, authHeader: string): Promise<EventStreamHandle>;
  nextEvent(): Promise<unknown>;
  close(): Promise<void>;
}

declare const _default: () => Promise<unknown>;
export default _default;
