/**
 * FFI Client Adapter - wraps haiinpm native binding for Node SDK.
 *
 * All HTTP calls delegate to the Rust hai-binding-core via napi-rs.
 * This module handles:
 * - JSON serialization/deserialization across FFI boundary
 * - Error mapping from FFI error strings to TypeScript error classes
 * - Type conversion from JSON responses to TypeScript interfaces
 *
 * @module ffi-client
 */

import { createRequire } from 'node:module';

import {
  HaiError,
  AuthenticationError,
  HaiConnectionError,
  HaiApiError,
  EmailNotActiveError,
  RecipientNotFoundError,
  RateLimitedError,
} from './errors.js';

// =============================================================================
// FFI Binding Type Declarations
// =============================================================================

/**
 * Type declarations for the haiinpm native binding.
 * These match the Rust napi-rs exports in rust/haiinpm/src/lib.rs.
 */
interface NativeHaiClient {
  // Registration & Identity
  hello(includeTest: boolean): Promise<string>;
  checkUsername(username: string): Promise<string>;
  register(optionsJson: string): Promise<string>;
  rotateKeys(optionsJson: string): Promise<string>;
  updateAgent(newAgentData: string): Promise<string>;
  submitResponse(paramsJson: string): Promise<string>;
  verifyStatus(agentId?: string | null): Promise<string>;

  // Username
  claimUsername(agentId: string, username: string): Promise<string>;
  updateUsername(agentId: string, username: string): Promise<string>;
  deleteUsername(agentId: string): Promise<string>;

  // Email Core
  sendEmail(optionsJson: string): Promise<string>;
  sendSignedEmail(optionsJson: string): Promise<string>;
  listMessages(optionsJson: string): Promise<string>;
  updateLabels(paramsJson: string): Promise<string>;
  getEmailStatus(): Promise<string>;
  getMessage(messageId: string): Promise<string>;
  getUnreadCount(): Promise<string>;

  // Email Actions
  markRead(messageId: string): Promise<void>;
  markUnread(messageId: string): Promise<void>;
  deleteMessage(messageId: string): Promise<void>;
  archive(messageId: string): Promise<void>;
  unarchive(messageId: string): Promise<void>;
  replyWithOptions(paramsJson: string): Promise<string>;
  forward(paramsJson: string): Promise<string>;

  // Search & Contacts
  searchMessages(optionsJson: string): Promise<string>;
  contacts(): Promise<string>;

  // Key Operations
  fetchRemoteKey(jacsId: string, version: string): Promise<string>;
  fetchKeyByHash(hash: string): Promise<string>;
  fetchKeyByEmail(email: string): Promise<string>;
  fetchKeyByDomain(domain: string): Promise<string>;
  fetchAllKeys(jacsId: string): Promise<string>;

  // Verification
  verifyDocument(document: string): Promise<string>;
  getVerification(agentId: string): Promise<string>;
  verifyAgentDocument(requestJson: string): Promise<string>;

  // Benchmarks
  benchmark(name?: string | null, tier?: string | null): Promise<string>;
  freeRun(transport?: string | null): Promise<string>;
  proRun(optionsJson: string): Promise<string>;
  enterpriseRun(): Promise<void>;

  // Email Templates
  createEmailTemplate(optionsJson: string): Promise<string>;
  listEmailTemplates(optionsJson: string): Promise<string>;
  getEmailTemplate(templateId: string): Promise<string>;
  updateEmailTemplate(templateId: string, optionsJson: string): Promise<string>;
  deleteEmailTemplate(templateId: string): Promise<void>;

  // Attestations
  createAttestation(paramsJson: string): Promise<string>;
  listAttestations(paramsJson: string): Promise<string>;
  getAttestation(agentId: string, docId: string): Promise<string>;
  verifyAttestation(document: string): Promise<string>;

  // Server Keys
  fetchServerKeys(): Promise<string>;

  // JACS Delegation
  buildAuthHeader(): Promise<string>;
  signMessage(message: string): Promise<string>;
  canonicalJson(valueJson: string): Promise<string>;
  verifyA2aArtifact(wrappedJson: string): Promise<string>;

  // JACS Export
  exportAgentJson(): Promise<string>;

  // Client State
  jacsId(): Promise<string>;
  setHaiAgentId(id: string): Promise<void>;
  setAgentEmail(email: string): Promise<void>;

  // Streaming (SSE / WebSocket)
  connectSse(): Promise<number>;
  sseNextEvent(handle: number): Promise<string | null>;
  sseClose(handle: number): Promise<void>;
  connectWs(): Promise<number>;
  wsNextEvent(handle: number): Promise<string | null>;
  wsClose(handle: number): Promise<void>;
}

interface NativeHaiClientConstructor {
  new (configJson: string): NativeHaiClient;
}

interface HaiinpmModule {
  HaiClient: NativeHaiClientConstructor;
}

// =============================================================================
// Error Mapping
// =============================================================================

/**
 * Map an FFI error (thrown by napi-rs) to the appropriate TypeScript error class.
 *
 * FFI errors have the format: "{ErrorKind}: {message}"
 * e.g. "AuthFailed: JACS signature rejected"
 */
export function mapFFIError(err: unknown): HaiError {
  const message = err instanceof Error ? err.message : String(err);

  // Parse ErrorKind prefix
  if (message.startsWith('AuthFailed:')) {
    return new AuthenticationError(message.slice('AuthFailed:'.length).trim(), 401);
  }
  if (message.startsWith('RateLimited:')) {
    return new RateLimitedError(message.slice('RateLimited:'.length).trim(), 429);
  }
  if (message.startsWith('NotFound:')) {
    const msg = message.slice('NotFound:'.length).trim();
    if (msg.toLowerCase().includes('email not active')) {
      return new EmailNotActiveError(msg);
    }
    if (msg.toLowerCase().includes('recipient')) {
      return new RecipientNotFoundError(msg);
    }
    return new HaiApiError(msg, 404);
  }
  if (message.startsWith('NetworkFailed:')) {
    return new HaiConnectionError(message.slice('NetworkFailed:'.length).trim());
  }
  if (message.startsWith('ApiError:')) {
    const msg = message.slice('ApiError:'.length).trim();
    // Try to extract status code from message
    const statusMatch = msg.match(/status (\d+)/);
    const status = statusMatch ? parseInt(statusMatch[1], 10) : undefined;
    if (msg.toLowerCase().includes('email not active')) {
      return new EmailNotActiveError(msg, status ?? 403);
    }
    if (msg.toLowerCase().includes('recipient')) {
      return new RecipientNotFoundError(msg, status ?? 400);
    }
    return new HaiApiError(msg, status);
  }
  if (message.startsWith('ConfigFailed:')) {
    return new HaiError(message.slice('ConfigFailed:'.length).trim());
  }
  if (message.startsWith('SerializationFailed:')) {
    return new HaiError(message.slice('SerializationFailed:'.length).trim());
  }
  if (message.startsWith('InvalidArgument:')) {
    return new HaiError(message.slice('InvalidArgument:'.length).trim());
  }
  if (message.startsWith('ProviderError:')) {
    return new AuthenticationError(message.slice('ProviderError:'.length).trim());
  }

  // Generic fallback
  return new HaiError(message);
}

// =============================================================================
// FFI Client Adapter
// =============================================================================

/**
 * Wraps a haiinpm native client instance and provides JSON-to-type conversion.
 *
 * Every method:
 * 1. Serializes arguments to JSON where needed
 * 2. Calls the native FFI method
 * 3. Parses the JSON response
 * 4. Catches FFI errors and maps them to TypeScript error classes
 */
export class FFIClientAdapter {
  private native: NativeHaiClient;

  /**
   * Create a new FFIClientAdapter synchronously from a JSON config string.
   *
   * Note: Client construction is synchronous and may briefly block the event loop
   * while loading JACS config files and initializing cryptographic key material.
   * For most use cases this is negligible (<10ms). If construction time is a
   * concern, use {@link FFIClientAdapter.create} for an async alternative.
   */
  constructor(configJson: string) {
    // Load haiinpm native addon. Native .node addons require require(), so
    // use createRequire (imported at module level from 'node:module') for ESM
    // compatibility. The top-level import works in both CJS and ESM because
    // 'node:module' is a built-in that TypeScript compiles correctly in both modes.
    let haiinpm: HaiinpmModule;
    try {
      // In ESM, __filename is undefined; use import.meta.url as fallback.
      // In CJS, __filename is always defined.
      let refUrl: string;
      if (typeof __filename !== 'undefined') {
        refUrl = __filename;
      } else {
        // ESM context: import.meta.url is always available
        try {
          refUrl = (import.meta as { url: string }).url;
        } catch {
          // Last resort fallback for environments where import.meta is unavailable
          refUrl = process.cwd() + '/ffi-client.js';
        }
      }
      const dynamicRequire = createRequire(refUrl);
      haiinpm = dynamicRequire('haiinpm') as HaiinpmModule;
    } catch (err) {
      const cause = err instanceof Error ? err.message : String(err);
      throw new HaiError(
        `Failed to load haiinpm native binding: ${cause}. ` +
        'Ensure the haiinpm package is installed and the native addon is built.',
      );
    }
    this.native = new haiinpm.HaiClient(configJson);
  }

  /**
   * Async factory method. Currently delegates to the synchronous constructor,
   * but provides a migration path for future non-blocking initialization.
   */
  static async create(configJson: string): Promise<FFIClientAdapter> {
    return new FFIClientAdapter(configJson);
  }

  // ---------------------------------------------------------------------------
  // Registration & Identity
  // ---------------------------------------------------------------------------

  async hello(includeTest: boolean): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.hello(includeTest);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async checkUsername(username: string): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.checkUsername(username);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async register(options: Record<string, unknown>): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.register(JSON.stringify(options));
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async rotateKeys(options: Record<string, unknown>): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.rotateKeys(JSON.stringify(options));
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async updateAgent(agentData: string): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.updateAgent(agentData);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async submitResponse(params: Record<string, unknown>): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.submitResponse(JSON.stringify(params));
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async verifyStatus(agentId?: string): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.verifyStatus(agentId ?? null);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  // ---------------------------------------------------------------------------
  // Username
  // ---------------------------------------------------------------------------

  async claimUsername(agentId: string, username: string): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.claimUsername(agentId, username);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async updateUsername(agentId: string, username: string): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.updateUsername(agentId, username);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async deleteUsername(agentId: string): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.deleteUsername(agentId);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  // ---------------------------------------------------------------------------
  // Email Core
  // ---------------------------------------------------------------------------

  async sendEmail(options: Record<string, unknown>): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.sendEmail(JSON.stringify(options));
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async sendSignedEmail(options: Record<string, unknown>): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.sendSignedEmail(JSON.stringify(options));
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async listMessages(options: Record<string, unknown>): Promise<unknown[]> {
    try {
      const json = await this.native.listMessages(JSON.stringify(options));
      return JSON.parse(json) as unknown[];
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async updateLabels(params: Record<string, unknown>): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.updateLabels(JSON.stringify(params));
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async getEmailStatus(): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.getEmailStatus();
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async getMessage(messageId: string): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.getMessage(messageId);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async getUnreadCount(): Promise<number> {
    try {
      const json = await this.native.getUnreadCount();
      // binding-core serializes the u64 return directly, so JSON is a bare number
      const parsed = JSON.parse(json);
      if (typeof parsed === 'number') {
        return parsed;
      }
      // Fallback: if the shape is {count: N} (future API change)
      if (typeof parsed === 'object' && parsed !== null && 'count' in parsed) {
        return (parsed as Record<string, unknown>).count as number;
      }
      return 0;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  // ---------------------------------------------------------------------------
  // Email Actions
  // ---------------------------------------------------------------------------

  async markRead(messageId: string): Promise<void> {
    try {
      await this.native.markRead(messageId);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async markUnread(messageId: string): Promise<void> {
    try {
      await this.native.markUnread(messageId);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async deleteMessage(messageId: string): Promise<void> {
    try {
      await this.native.deleteMessage(messageId);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async archive(messageId: string): Promise<void> {
    try {
      await this.native.archive(messageId);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async unarchive(messageId: string): Promise<void> {
    try {
      await this.native.unarchive(messageId);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async replyWithOptions(params: Record<string, unknown>): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.replyWithOptions(JSON.stringify(params));
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async forward(params: Record<string, unknown>): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.forward(JSON.stringify(params));
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  // ---------------------------------------------------------------------------
  // Search & Contacts
  // ---------------------------------------------------------------------------

  async searchMessages(options: Record<string, unknown>): Promise<unknown[]> {
    try {
      const json = await this.native.searchMessages(JSON.stringify(options));
      return JSON.parse(json) as unknown[];
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async contacts(): Promise<unknown[]> {
    try {
      const json = await this.native.contacts();
      return JSON.parse(json) as unknown[];
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  // ---------------------------------------------------------------------------
  // Key Operations
  // ---------------------------------------------------------------------------

  async fetchRemoteKey(jacsId: string, version: string): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.fetchRemoteKey(jacsId, version);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async fetchKeyByHash(hash: string): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.fetchKeyByHash(hash);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async fetchKeyByEmail(email: string): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.fetchKeyByEmail(email);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async fetchKeyByDomain(domain: string): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.fetchKeyByDomain(domain);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async fetchAllKeys(jacsId: string): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.fetchAllKeys(jacsId);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  // ---------------------------------------------------------------------------
  // Verification
  // ---------------------------------------------------------------------------

  async verifyDocument(document: string): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.verifyDocument(document);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async getVerification(agentId: string): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.getVerification(agentId);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async verifyAgentDocument(requestJson: string): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.verifyAgentDocument(requestJson);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  // ---------------------------------------------------------------------------
  // Benchmarks
  // ---------------------------------------------------------------------------

  async benchmark(name?: string, tier?: string): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.benchmark(name ?? null, tier ?? null);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async freeRun(transport?: string): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.freeRun(transport ?? null);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async proRun(options: Record<string, unknown>): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.proRun(JSON.stringify(options));
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async enterpriseRun(): Promise<void> {
    try {
      await this.native.enterpriseRun();
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  // ---------------------------------------------------------------------------
  // Email Templates
  // ---------------------------------------------------------------------------

  async createEmailTemplate(optionsJson: string): Promise<string> {
    try {
      return await this.native.createEmailTemplate(optionsJson);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async listEmailTemplates(optionsJson: string): Promise<string> {
    try {
      return await this.native.listEmailTemplates(optionsJson);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async getEmailTemplate(templateId: string): Promise<string> {
    try {
      return await this.native.getEmailTemplate(templateId);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async updateEmailTemplate(templateId: string, optionsJson: string): Promise<string> {
    try {
      return await this.native.updateEmailTemplate(templateId, optionsJson);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async deleteEmailTemplate(templateId: string): Promise<void> {
    try {
      await this.native.deleteEmailTemplate(templateId);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  // ---------------------------------------------------------------------------
  // Attestations
  // ---------------------------------------------------------------------------

  async createAttestation(paramsJson: string): Promise<string> {
    try {
      return await this.native.createAttestation(paramsJson);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async listAttestations(paramsJson: string): Promise<string> {
    try {
      return await this.native.listAttestations(paramsJson);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async getAttestation(agentId: string, docId: string): Promise<string> {
    try {
      return await this.native.getAttestation(agentId, docId);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async verifyAttestation(document: string): Promise<string> {
    try {
      return await this.native.verifyAttestation(document);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  // ---------------------------------------------------------------------------
  // Server Keys
  // ---------------------------------------------------------------------------

  async fetchServerKeys(): Promise<string> {
    try {
      return await this.native.fetchServerKeys();
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  // ---------------------------------------------------------------------------
  // JACS Delegation
  // ---------------------------------------------------------------------------

  async buildAuthHeader(): Promise<string> {
    try {
      return await this.native.buildAuthHeader();
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async signMessage(message: string): Promise<string> {
    try {
      return await this.native.signMessage(message);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async canonicalJson(value: string): Promise<string> {
    try {
      return await this.native.canonicalJson(value);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async verifyA2aArtifact(wrappedJson: string): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.verifyA2aArtifact(wrappedJson);
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async exportAgentJson(): Promise<Record<string, unknown>> {
    try {
      const json = await this.native.exportAgentJson();
      return JSON.parse(json) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  // ---------------------------------------------------------------------------
  // Client State
  // ---------------------------------------------------------------------------

  async jacsId(): Promise<string> {
    try {
      return await this.native.jacsId();
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async setHaiAgentId(id: string): Promise<void> {
    try {
      await this.native.setHaiAgentId(id);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async setAgentEmail(email: string): Promise<void> {
    try {
      await this.native.setAgentEmail(email);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  // ---------------------------------------------------------------------------
  // Streaming (SSE / WebSocket)
  // ---------------------------------------------------------------------------

  async connectSse(): Promise<number> {
    try {
      return await this.native.connectSse();
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async sseNextEvent(handle: number): Promise<Record<string, unknown> | null> {
    try {
      const raw = await this.native.sseNextEvent(handle);
      if (raw === null || raw === undefined) return null;
      return JSON.parse(raw) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async sseClose(handle: number): Promise<void> {
    try {
      await this.native.sseClose(handle);
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async connectWs(): Promise<number> {
    try {
      return await this.native.connectWs();
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async wsNextEvent(handle: number): Promise<Record<string, unknown> | null> {
    try {
      const raw = await this.native.wsNextEvent(handle);
      if (raw === null || raw === undefined) return null;
      return JSON.parse(raw) as Record<string, unknown>;
    } catch (err) {
      throw mapFFIError(err);
    }
  }

  async wsClose(handle: number): Promise<void> {
    try {
      await this.native.wsClose(handle);
    } catch (err) {
      throw mapFFIError(err);
    }
  }
}
