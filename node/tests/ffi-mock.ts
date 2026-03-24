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
    hello: defaultReject,
    checkUsername: defaultReject,
    register: defaultReject,
    rotateKeys: defaultReject,
    updateAgent: defaultReject,
    submitResponse: defaultReject,
    verifyStatus: defaultReject,
    claimUsername: defaultReject,
    updateUsername: defaultReject,
    deleteUsername: defaultReject,
    sendEmail: defaultReject,
    sendSignedEmail: defaultReject,
    listMessages: defaultReject,
    updateLabels: defaultReject,
    getEmailStatus: defaultReject,
    getMessage: defaultReject,
    getUnreadCount: defaultReject,
    markRead: defaultReject,
    markUnread: defaultReject,
    deleteMessage: defaultReject,
    archive: defaultReject,
    unarchive: defaultReject,
    replyWithOptions: defaultReject,
    forward: defaultReject,
    searchMessages: defaultReject,
    contacts: defaultReject,
    fetchRemoteKey: defaultReject,
    fetchKeyByHash: defaultReject,
    fetchKeyByEmail: defaultReject,
    fetchKeyByDomain: defaultReject,
    fetchAllKeys: defaultReject,
    verifyDocument: defaultReject,
    getVerification: defaultReject,
    verifyAgentDocument: defaultReject,
    benchmark: defaultReject,
    freeRun: defaultReject,
    proRun: defaultReject,
    enterpriseRun: defaultReject,
    buildAuthHeader: defaultReject,
    signMessage: defaultReject,
    canonicalJson: defaultReject,
    verifyA2aArtifact: defaultReject,
    exportAgentJson: defaultReject,
    jacsId: defaultReject,
    setHaiAgentId: defaultReject,
    setAgentEmail: defaultReject,
  };

  if (overrides) {
    Object.assign(mock, overrides);
  }

  return mock as unknown as FFIClientAdapter;
}
