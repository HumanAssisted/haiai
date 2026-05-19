// @haiai/wasm/types — shared TS types (Task 034 / HAIAI_WASM_PRD §3.1).
//
// Mirrors `node/src/types.ts` minus the Node-only file/stream shapes
// (PRD §4.8: browser attachments are base64-in-JSON only). Browser
// callers import via `import type { ... } from "@haiai/wasm/types"`.
// `.js` companion is emitted but empty — pure type-only module.

// ── Generic ──

export type Algorithm = "ed25519" | "pq2025";

/** PRD §3.1 wire shape for typed errors. */
export interface HaiaiWasmErrorPayload {
  code: string;
  message: string;
  details?: unknown;
}

// ── Hello / Registration ──

export interface HelloResult {
  timestamp: string;
  client_ip: string;
  hai_public_key_fingerprint: string;
  message: string;
  hai_signed_ack: string;
  hello_id: string;
  test_scenario?: unknown;
}

export interface RegisterAgentOptions {
  agent_json: string;
  public_key_pem?: string;
  owner_email?: string;
  domain?: string;
  description?: string;
  registration_key?: string;
  is_mediator?: boolean;
}

export interface RegistrationResult {
  success: boolean;
  agent_id: string;
  jacs_id: string;
  dns_verified: boolean;
  registrations: unknown[];
  registered_at: string;
  message?: string;
  email?: string;
}

export interface RotationResult {
  jacs_id: string;
  old_version: string;
  new_version: string;
  new_public_key_hash: string;
  registered_with_hai: boolean;
  signed_agent_json: string;
}

// ── Email ──

export interface SendEmailOptions {
  to: string;
  subject: string;
  body: string;
  cc?: string[];
  bcc?: string[];
  in_reply_to?: string;
  /** Browser path requires base64-encoded attachments inline. */
  attachments?: Array<{
    filename: string;
    content_b64: string;
    content_type?: string;
  }>;
  labels?: string[];
  append_footer?: boolean;
  idempotency_key?: string;
}

export interface SendEmailResult {
  message_id: string;
  status: string;
  /** Optional URL-safe verify link the server returns for the send. */
  verify_link?: string;
}

export interface ListMessagesOptions {
  page?: number;
  page_size?: number;
  label?: string;
  unread_only?: boolean;
}

export interface EmailMessage {
  id: string;
  from_address: string;
  to_addresses: string[];
  cc_addresses: string[];
  bcc_addresses: string[];
  subject: string;
  body_text: string;
  body_html?: string;
  created_at: string;
  read: boolean;
  archived: boolean;
  labels: string[];
  message_id?: string;
  in_reply_to?: string;
  attachments?: Array<{ filename: string; content_type?: string }>;
}

export interface RawEmailResponse {
  message_id: string;
  available: boolean;
  raw_mime_b64?: string;
  omitted_reason?: string;
  jacs_signature?: string;
}

export interface EmailStatus {
  email: string;
  active: boolean;
  unread_count: number;
}

export interface Contact {
  email: string;
  display_name?: string;
}

// ── Email templates ──

export interface CreateEmailTemplateOptions {
  name: string;
  subject: string;
  body: string;
  description?: string;
}

export interface UpdateEmailTemplateOptions {
  name?: string;
  subject?: string;
  body?: string;
  description?: string;
}

export interface EmailTemplate {
  id: string;
  name: string;
  subject: string;
  body: string;
  description?: string;
  created_at: string;
  updated_at: string;
}

// ── Search ──

export interface SearchOptions {
  query?: string;
  page?: number;
  page_size?: number;
  labels?: string[];
}

// ── Key & Verification ──

export interface PublicKeyInfo {
  jacs_id: string;
  version: string;
  algorithm: Algorithm;
  public_key_base64: string;
  public_key_hash: string;
  registered_at?: string;
}

export interface DocumentVerificationResult {
  valid: boolean;
  status: string;
  signer_id?: string;
  timestamp?: string;
  errors?: string[];
}

export interface AgentVerificationResult {
  agent_id: string;
  verified: boolean;
  status: string;
  details?: unknown;
}

export interface VerifyAgentDocumentRequest {
  agent_id: string;
  document: string;
}

// ── Events ──

export interface HaiEvent {
  event_type: string;
  data: unknown;
  id?: string;
  raw: string;
}

export type EventStreamTransport = "sse" | "ws";

// ── Metrics ──

export interface HaiaiWasmMetrics {
  httpRequestCount: number;
  httpErrorCount: number;
  signCount: number;
  verifyCount: number;
  sseEventsDelivered: number;
  wsEventsDelivered: number;
  lastHttpDurationMs: number;
  lastSignDurationMs: number;
  lastVerifyDurationMs: number;
}
