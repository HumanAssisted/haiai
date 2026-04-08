/**
 * High-level Agent API for HAI email operations.
 *
 * Provides an `Agent` class with an `agent.email` namespace that wraps
 * the existing {@link HaiClient} for email operations. All emails are
 * sent signed with the agent's JACS key. There is no unsigned send path.
 *
 * @example
 * ```typescript
 * import { Agent } from '@haiai/haiai';
 *
 * const agent = await Agent.fromConfig('./jacs.config.json');
 * await agent.email.send({ to: 'other@hai.ai', subject: 'Hello', body: 'World' });
 * ```
 */

import { HaiClient } from './client.js';
import type {
  Contact,
  EmailMessage,
  EmailStatus,
  ForwardOptions,
  ListEmailTemplatesOptions,
  ListEmailTemplatesResult,
  ListMessagesOptions,
  SearchOptions,
  SendEmailOptions,
  SendEmailResult,
} from './types.js';

/** Options for creating an Agent. */
export interface AgentOptions {
  /** HAI API base URL (default: https://hai.ai). */
  baseUrl?: string;
  /** Request timeout in ms (default: 30000). */
  timeout?: number;
}

/**
 * High-level agent wrapper providing `agent.email.*` namespace.
 *
 * Created via {@link Agent.fromConfig} which loads the JACS config and
 * initializes the underlying {@link HaiClient}. All email operations
 * go through the agent's JACS key -- there is no unsigned path.
 */
export class Agent {
  /** Email operations namespace. */
  readonly email: EmailNamespace;

  private readonly _client: HaiClient;

  constructor(client: HaiClient) {
    this._client = client;
    this.email = new EmailNamespace(client);
  }

  /**
   * Create an Agent from a `jacs.config.json` file.
   *
   * Loads the JACS agent configuration and initializes the client.
   *
   * @param configPath - Path to jacs.config.json.
   * @param options - Optional client options (base URL, timeout).
   * @returns A configured Agent instance.
   */
  static async fromConfig(
    configPath: string = './jacs.config.json',
    options?: AgentOptions,
  ): Promise<Agent> {
    const client = await HaiClient.create({
      configPath,
      url: options?.baseUrl,
      timeout: options?.timeout,
    });
    return new Agent(client);
  }

  /** Access the underlying {@link HaiClient} for advanced operations. */
  get client(): HaiClient {
    return this._client;
  }
}

/** Options for the send method. */
export interface SendOptions {
  /** Recipient email address. */
  to: string;
  /** Email subject line. */
  subject: string;
  /** Plain text email body. */
  body: string;
  /** Optional Message-ID for threading. */
  inReplyTo?: string;
  /** Optional file attachments. */
  attachments?: Array<{
    filename: string;
    contentType: string;
    data: Buffer;
  }>;
  /** CC recipient addresses. */
  cc?: string[];
  /** BCC recipient addresses. */
  bcc?: string[];
  /** Labels/tags for the message. */
  labels?: string[];
}

/**
 * Email operations namespace.
 *
 * All methods delegate to {@link HaiClient} email methods. The
 * {@link EmailNamespace.send} method always signs with the agent's
 * JACS key via `sendSignedEmail`. There is no unsigned send path.
 */
export class EmailNamespace {
  private readonly _client: HaiClient;

  constructor(client: HaiClient) {
    this._client = client;
  }

  /**
   * Send an email, always signed with the agent's JACS key.
   *
   * Builds RFC 5322 MIME, signs with the agent's Ed25519 key via JACS,
   * and submits to the HAI API. There is no unsigned send path.
   *
   * @param options - Email options (to, subject, body, attachments).
   * @returns SendEmailResult with messageId and status.
   */
  async send(options: SendOptions): Promise<SendEmailResult> {
    return this._client.sendSignedEmail({
      to: options.to,
      subject: options.subject,
      body: options.body,
      inReplyTo: options.inReplyTo,
      attachments: options.attachments?.map(a => ({
        filename: a.filename,
        contentType: a.contentType,
        data: a.data,
      })),
      cc: options.cc,
      bcc: options.bcc,
      labels: options.labels,
    });
  }

  /**
   * List inbox messages (direction=inbound).
   *
   * @param options - Optional list options (limit, offset).
   * @returns Array of EmailMessage objects.
   */
  async inbox(options?: { limit?: number; offset?: number; isRead?: boolean; folder?: string; label?: string }): Promise<EmailMessage[]> {
    return this._client.listMessages({
      limit: options?.limit,
      offset: options?.offset,
      direction: 'inbound',
      isRead: options?.isRead,
      folder: options?.folder,
      label: options?.label,
    });
  }

  /**
   * List outbox messages (direction=outbound).
   *
   * @param options - Optional list options (limit, offset, filters).
   * @returns Array of EmailMessage objects.
   */
  async outbox(options?: { limit?: number; offset?: number; isRead?: boolean; folder?: string; label?: string }): Promise<EmailMessage[]> {
    return this._client.listMessages({
      limit: options?.limit,
      offset: options?.offset,
      direction: 'outbound',
      isRead: options?.isRead,
      folder: options?.folder,
      label: options?.label,
    });
  }

  /**
   * Get a specific message by ID.
   *
   * @param messageId - The message ID to retrieve.
   * @returns EmailMessage.
   */
  async get(messageId: string): Promise<EmailMessage> {
    return this._client.getMessage(messageId);
  }

  /**
   * Search email messages.
   *
   * @param options - Search options (query, direction, date range, etc.).
   * @returns Array of EmailMessage matching the search criteria.
   */
  async search(options: SearchOptions): Promise<EmailMessage[]> {
    return this._client.searchMessages(options);
  }

  /**
   * Get email status including capacity and tier information.
   *
   * @returns EmailStatus with daily limits, usage, and tier info.
   */
  async status(): Promise<EmailStatus> {
    return this._client.getEmailStatus();
  }

  /**
   * Get the count of unread messages.
   *
   * @returns Number of unread inbound messages.
   */
  async unreadCount(): Promise<number> {
    return this._client.getUnreadCount();
  }

  /**
   * Delete a message by ID.
   *
   * @param messageId - The message ID to delete.
   */
  async delete(messageId: string): Promise<void> {
    return this._client.deleteMessage(messageId);
  }

  /**
   * Mark a message as read.
   *
   * @param messageId - The message ID to mark as read.
   */
  async markRead(messageId: string): Promise<void> {
    return this._client.markRead(messageId);
  }

  /**
   * Mark a message as unread.
   *
   * @param messageId - The message ID to mark as unread.
   */
  async markUnread(messageId: string): Promise<void> {
    return this._client.markUnread(messageId);
  }

  /**
   * Reply to a message, always signed with the agent's JACS key.
   *
   * Fetches the original message, constructs a reply with proper
   * threading headers, and sends it signed.
   *
   * @param messageId - The message ID to reply to.
   * @param body - Reply body text.
   * @param subjectOverride - Optional subject override.
   * @returns SendEmailResult with messageId and status.
   */
  async reply(
    messageId: string,
    body: string,
    subjectOverride?: string,
  ): Promise<SendEmailResult> {
    return this._client.reply(messageId, body, subjectOverride);
  }

  /**
   * Forward a message to another recipient.
   *
   * @param messageId - The message ID to forward.
   * @param to - Recipient email address to forward to.
   * @param comment - Optional comment to prepend.
   * @returns SendEmailResult with messageId and status.
   */
  async forward(messageId: string, to: string, comment?: string): Promise<SendEmailResult> {
    return this._client.forward({ messageId, to, comment });
  }

  /**
   * Archive a message.
   *
   * @param messageId - The message ID to archive.
   */
  async archive(messageId: string): Promise<void> {
    return this._client.archive(messageId);
  }

  /**
   * Unarchive (restore) a message back to the inbox.
   *
   * @param messageId - The message ID to unarchive.
   */
  async unarchive(messageId: string): Promise<void> {
    return this._client.unarchive(messageId);
  }

  /**
   * List contacts derived from email history.
   *
   * @returns Array of Contact objects.
   */
  async contacts(): Promise<Contact[]> {
    return this._client.getContacts();
  }

  /**
   * List or search email templates.
   *
   * @param options - Optional pagination and search query.
   * @returns ListEmailTemplatesResult with templates array and total count.
   */
  async templates(options?: ListEmailTemplatesOptions): Promise<ListEmailTemplatesResult> {
    return this._client.listEmailTemplates(options);
  }
}
