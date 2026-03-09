package haiai

import "context"

// Agent is the high-level wrapper providing agent.Email.* namespace.
//
// Created via AgentFromConfig which loads the JACS config and
// initializes the underlying Client. All email operations go through
// the agent's JACS key -- there is no unsigned path.
type Agent struct {
	// Email operations namespace.
	Email *EmailNamespace

	client *Client
}

// AgentFromConfig creates an Agent from a jacs.config.json file.
//
// Loads the JACS agent configuration and initializes the client.
// If configPath is empty, the client uses its default discovery order
// (JACS_CONFIG_PATH env var, then ./jacs.config.json).
func AgentFromConfig(configPath string, opts ...Option) (*Agent, error) {
	client, err := NewClient(opts...)
	if err != nil {
		return nil, err
	}
	return &Agent{
		Email:  &EmailNamespace{client: client},
		client: client,
	}, nil
}

// Client returns the underlying Client for advanced operations.
func (a *Agent) Client() *Client {
	return a.client
}

// EmailNamespace provides email operations.
//
// All methods delegate to Client email methods. The Send method
// always signs with the agent's JACS key via SendSignedEmail.
// There is no unsigned send path.
type EmailNamespace struct {
	client *Client
}

// Send sends an email, always signed with the agent's JACS key.
//
// Builds RFC 5322 MIME, signs with the agent's Ed25519 key via JACS,
// and submits to the HAI API. There is no unsigned send path.
func (e *EmailNamespace) Send(ctx context.Context, opts SendEmailOptions) (*SendEmailResult, error) {
	return e.client.SendSignedEmail(ctx, opts)
}

// Inbox lists inbox messages (direction=inbound).
func (e *EmailNamespace) Inbox(ctx context.Context, opts ListMessagesOptions) ([]EmailMessage, error) {
	opts.Direction = "inbound"
	return e.client.ListMessages(ctx, opts)
}

// Outbox lists outbox messages (direction=outbound).
func (e *EmailNamespace) Outbox(ctx context.Context, opts ListMessagesOptions) ([]EmailMessage, error) {
	opts.Direction = "outbound"
	return e.client.ListMessages(ctx, opts)
}

// Get retrieves a specific message by ID.
func (e *EmailNamespace) Get(ctx context.Context, messageID string) (*EmailMessage, error) {
	return e.client.GetMessage(ctx, messageID)
}

// Search searches email messages with filters.
func (e *EmailNamespace) Search(ctx context.Context, opts SearchOptions) ([]EmailMessage, error) {
	return e.client.SearchMessages(ctx, opts)
}

// Status returns email status including capacity and tier information.
func (e *EmailNamespace) Status(ctx context.Context) (*EmailStatus, error) {
	return e.client.GetEmailStatus(ctx)
}

// UnreadCount returns the count of unread messages.
func (e *EmailNamespace) UnreadCount(ctx context.Context) (int, error) {
	return e.client.GetUnreadCount(ctx)
}

// Delete deletes a message by ID.
func (e *EmailNamespace) Delete(ctx context.Context, messageID string) error {
	return e.client.DeleteMessage(ctx, messageID)
}

// MarkRead marks a message as read.
func (e *EmailNamespace) MarkRead(ctx context.Context, messageID string) error {
	return e.client.MarkRead(ctx, messageID)
}

// MarkUnread marks a message as unread.
func (e *EmailNamespace) MarkUnread(ctx context.Context, messageID string) error {
	return e.client.MarkUnread(ctx, messageID)
}

// Reply replies to a message, always signed with the agent's JACS key.
//
// Fetches the original message, constructs a reply with proper
// threading headers, and sends it signed.
func (e *EmailNamespace) Reply(ctx context.Context, messageID, body, subjectOverride string) (*SendEmailResult, error) {
	return e.client.Reply(ctx, messageID, body, subjectOverride)
}
