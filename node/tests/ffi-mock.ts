/**
 * Test helper: creates a mock FFIClientAdapter for unit tests.
 *
 * Since the real FFIClientAdapter requires haiinpm (native Rust binding),
 * tests inject this mock via client._setFFIAdapter().
 */
import type { FFIClientAdapter } from '../src/ffi-client.js';

type MockFFI = {
  [K in keyof FFIClientAdapter]: FFIClientAdapter[K] extends (...args: infer A) => infer R
    ? jest.Mock<R, A> | ((...args: A) => R)
    : FFIClientAdapter[K];
};

/**
 * Create a mock FFIClientAdapter with all methods stubbed to reject.
 * Override specific methods as needed.
 */
export function createMockFFI(overrides?: Partial<MockFFI>): FFIClientAdapter {
  const defaultReject = () => Promise.reject(new Error('FFI method not mocked'));

  const mock: Record<string, unknown> = {
    // Registration & Identity
    hello: defaultReject,
    register: defaultReject,
    registerNewAgent: defaultReject,
    rotateKeys: defaultReject,
    updateAgent: defaultReject,
    submitResponse: defaultReject,
    verifyStatus: defaultReject,
    // Username
    updateUsername: defaultReject,
    deleteUsername: defaultReject,
    // Email Core
    sendEmail: defaultReject,
    sendSignedEmail: defaultReject,
    signEmailRaw: defaultReject,
    verifyEmailRaw: defaultReject,
    listMessages: defaultReject,
    updateLabels: defaultReject,
    getEmailStatus: defaultReject,
    getMessage: defaultReject,
    getRawEmail: defaultReject,
    getUnreadCount: defaultReject,
    // Email Actions
    markRead: defaultReject,
    markUnread: defaultReject,
    deleteMessage: defaultReject,
    archive: defaultReject,
    unarchive: defaultReject,
    replyWithOptions: defaultReject,
    forward: defaultReject,
    // Email Templates
    createEmailTemplate: defaultReject,
    listEmailTemplates: defaultReject,
    getEmailTemplate: defaultReject,
    updateEmailTemplate: defaultReject,
    deleteEmailTemplate: defaultReject,
    // Search & Contacts
    searchMessages: defaultReject,
    contacts: defaultReject,
    // Key Operations
    fetchRemoteKey: defaultReject,
    fetchKeyByHash: defaultReject,
    fetchKeyByEmail: defaultReject,
    fetchKeyByDomain: defaultReject,
    fetchAllKeys: defaultReject,
    fetchServerKeys: defaultReject,
    // Verification
    verifyDocument: defaultReject,
    getVerification: defaultReject,
    verifyAgentDocument: defaultReject,
    // Attestations
    createAttestation: defaultReject,
    listAttestations: defaultReject,
    getAttestation: defaultReject,
    verifyAttestation: defaultReject,
    // Benchmarks
    benchmark: defaultReject,
    freeRun: defaultReject,
    proRun: defaultReject,
    enterpriseRun: defaultReject,
    // Streaming
    connectSse: defaultReject,
    sseNextEvent: defaultReject,
    sseClose: defaultReject,
    connectWs: defaultReject,
    wsNextEvent: defaultReject,
    wsClose: defaultReject,
    // JACS Delegation
    buildAuthHeader: defaultReject,
    signMessage: defaultReject,
    canonicalJson: defaultReject,
    verifyA2aArtifact: defaultReject,
    exportAgentJson: defaultReject,
    // Client State
    jacsId: defaultReject,
    baseUrl: defaultReject,
    haiAgentId: defaultReject,
    agentEmail: defaultReject,
    setHaiAgentId: defaultReject,
    setAgentEmail: defaultReject,
    // Layer 8: local media (sign/verify/extract for inline text + images).
    signText: defaultReject,
    verifyText: defaultReject,
    signImage: defaultReject,
    verifyImage: defaultReject,
    extractMediaSignature: defaultReject,
    // JACS Document Store (Issue 025) — 13 generic + 4 D5 + 3 D9 = 20 methods.
    storeDocument: defaultReject,
    signAndStore: defaultReject,
    getDocument: defaultReject,
    getLatestDocument: defaultReject,
    getDocumentVersions: defaultReject,
    listDocuments: defaultReject,
    removeDocument: defaultReject,
    updateDocument: defaultReject,
    searchDocuments: defaultReject,
    queryByType: defaultReject,
    queryByField: defaultReject,
    queryByAgent: defaultReject,
    storageCapabilities: defaultReject,
    // D5 — MEMORY / SOUL convenience wrappers
    saveMemory: defaultReject,
    saveSoul: defaultReject,
    getMemory: defaultReject,
    getSoul: defaultReject,
    // D9 — typed-content helpers
    storeTextFile: defaultReject,
    storeImageFile: defaultReject,
    getRecordBytes: defaultReject,
  };

  if (overrides) {
    Object.assign(mock, overrides);
  }

  return mock as unknown as FFIClientAdapter;
}
