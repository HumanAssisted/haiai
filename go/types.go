package haiai

import (
	"encoding/json"
	"errors"
	"fmt"
)

// TransportType specifies the real-time transport protocol.
type TransportType string

const (
	// TransportSSE uses Server-Sent Events for receiving jobs.
	TransportSSE TransportType = "sse"
	// TransportWS uses WebSocket for bidirectional communication.
	TransportWS TransportType = "ws"
)

// RegistrationResult contains the result of registering an agent with HAI.
type RegistrationResult struct {
	AgentID     string         `json:"agent_id"`
	JacsID      string         `json:"jacs_id"`
	DNSVerified bool           `json:"dns_verified"`
	Signatures  []HaiSignature `json:"signatures"`
}

// HaiSignature represents a signature from HAI.
type HaiSignature struct {
	KeyID     string `json:"key_id"`
	Algorithm string `json:"algorithm"`
	Signature string `json:"signature"`
	SignedAt  string `json:"signed_at"`
}

// StatusResult contains the verification/registration status of an agent.
// Maps to GET /api/v1/agents/{jacs_id}/verify response.
type StatusResult struct {
	JacsID        string         `json:"jacs_id"`
	Registered    bool           `json:"registered"`
	Registrations []Registration `json:"registrations"`
	DNSVerified   bool           `json:"dns_verified"`
	RegisteredAt  string         `json:"registered_at"`
}

// Registration represents a single registration entry in the verify response.
type Registration struct {
	KeyID         string `json:"key_id"`
	Algorithm     string `json:"algorithm"`
	SignatureJSON string `json:"signature_json"`
	SignedAt      string `json:"signed_at"`
}

// PublicKeyInfo contains information about a public key fetched from HAI.
type PublicKeyInfo struct {
	PublicKey     []byte `json:"public_key"`
	Algorithm     string `json:"algorithm"`
	PublicKeyHash string `json:"public_key_hash"`
	AgentID       string `json:"agent_id"`
	Version       string `json:"version"`
}

// DocumentVerificationResult is the response from POST /api/jacs/verify.
type DocumentVerificationResult struct {
	Valid             bool   `json:"valid"`
	VerifiedAt        string `json:"verified_at"`
	DocumentType      string `json:"document_type"`
	IssuerVerified    bool   `json:"issuer_verified"`
	SignatureVerified bool   `json:"signature_verified"`
	SignerID          string `json:"signer_id"`
	SignedAt          string `json:"signed_at"`
	Error             string `json:"error,omitempty"`
}

// VerificationStatus describes 3-level verification for advanced trust endpoints.
type VerificationStatus struct {
	JacsValid     bool   `json:"jacs_valid"`
	DNSValid      bool   `json:"dns_valid"`
	HAIRegistered bool   `json:"hai_registered"`
	Badge         string `json:"badge"`
}

// AgentVerificationResult is returned by:
// - GET /api/v1/agents/{agent_id}/verification
// - POST /api/v1/agents/verify
type AgentVerificationResult struct {
	AgentID       string             `json:"agent_id"`
	Verification  VerificationStatus `json:"verification"`
	HaiSignatures []string           `json:"hai_signatures,omitempty"`
	VerifiedAt    string             `json:"verified_at"`
	Errors        []string           `json:"errors,omitempty"`
}

// VerifyAgentDocumentRequest is the payload for POST /api/v1/agents/verify.
type VerifyAgentDocumentRequest struct {
	AgentJSON string `json:"agent_json"`
	PublicKey string `json:"public_key,omitempty"`
	Domain    string `json:"domain,omitempty"`
}

// BenchmarkResult contains the result of a benchmark run.
type BenchmarkResult struct {
	RunID       string                `json:"run_id"`
	Name        string                `json:"name"`
	Tier        string                `json:"tier"`
	Score       float64               `json:"score"`
	Results     []BenchmarkTestResult `json:"results"`
	CompletedAt string                `json:"completed_at"`
}

// BenchmarkTestResult contains an individual test result.
type BenchmarkTestResult struct {
	Name    string  `json:"name"`
	Passed  bool    `json:"passed"`
	Score   float64 `json:"score"`
	Message string  `json:"message,omitempty"`
}

// AgentEvent represents events sent from HAI to connected agents.
// The Type field determines which payload fields are populated.
type AgentEvent struct {
	Type string `json:"type"`

	// Connected event fields
	AgentID   string `json:"agent_id,omitempty"`
	AgentName string `json:"agent_name,omitempty"`

	// Heartbeat event fields
	Timestamp int64 `json:"timestamp,omitempty"`

	// Disconnect event fields
	Reason string `json:"reason,omitempty"`

	// BenchmarkJob event fields
	JobID      string              `json:"job_id,omitempty"`
	ScenarioID string              `json:"scenario_id,omitempty"`
	Config     *BenchmarkJobConfig `json:"config,omitempty"`
}

// BenchmarkJobConfig contains the configuration for a benchmark job.
type BenchmarkJobConfig struct {
	RunID        string             `json:"run_id"`
	ScenarioName string             `json:"scenario_name"`
	Conversation []ConversationTurn `json:"conversation"`
	RawMode      bool               `json:"raw_mode"`
	TimeoutSecs  uint64             `json:"timeout_secs"`
}

// ConversationTurn represents a single turn in a conversation.
type ConversationTurn struct {
	Speaker    string `json:"speaker"`
	Message    string `json:"message"`
	TurnNumber int    `json:"turn_number"`
}

// ModerationResponse is the agent's response to a benchmark job.
type ModerationResponse struct {
	Message          string          `json:"message"`
	Metadata         json.RawMessage `json:"metadata,omitempty"`
	ProcessingTimeMs *uint64         `json:"processing_time_ms,omitempty"`
}

// JobResponseResult is the server's acknowledgment of a submitted job response.
type JobResponseResult struct {
	Success bool   `json:"success"`
	JobID   string `json:"job_id"`
	Message string `json:"message"`
}

// HelloResult is the response from the hello endpoint.
type HelloResult struct {
	Timestamp               string `json:"timestamp"`
	ClientIP                string `json:"client_ip"`
	HaiPublicKeyFingerprint string `json:"hai_public_key_fingerprint"`
	Message                 string `json:"message"`
	HaiSignedAck            string `json:"hai_signed_ack"`
	HelloID                 string `json:"hello_id"`
	TestScenario            string `json:"test_scenario,omitempty"`
}

// AttestationResult is the response from the attestation endpoint.
type AttestationResult struct {
	AgentID     string          `json:"agent_id"`
	Attestation json.RawMessage `json:"attestation"`
	Signatures  []HaiSignature  `json:"signatures,omitempty"`
}

// VerifyResult is the response from verifying another agent.
// Maps to GET /api/v1/agents/{jacs_id}/verify response.
type VerifyResult struct {
	JacsID        string         `json:"jacs_id"`
	Registered    bool           `json:"registered"`
	Registrations []Registration `json:"registrations"`
	DNSVerified   bool           `json:"dns_verified"`
	RegisteredAt  string         `json:"registered_at"`
}

// SignedDocument is a JACS-signed document envelope matching the Python SDK format.
type SignedDocument struct {
	Version       string                 `json:"version"`
	DocumentType  string                 `json:"document_type"`
	Data          map[string]interface{} `json:"data"`
	Metadata      SignedDocumentMetadata `json:"metadata"`
	JacsSignature JacsSignatureBlock     `json:"jacsSignature"`
}

// SignedDocumentMetadata contains metadata about a signed document.
type SignedDocumentMetadata struct {
	Issuer     string `json:"issuer"`
	DocumentID string `json:"document_id"`
	CreatedAt  string `json:"created_at"`
	Hash       string `json:"hash"`
}

// JacsSignatureBlock contains the JACS signature fields.
type JacsSignatureBlock struct {
	AgentID   string `json:"agentID"`
	Date      string `json:"date"`
	Signature string `json:"signature"`
}

// UpdateUsernameResult is the response from updating a claimed username.
type UpdateUsernameResult struct {
	Username         string `json:"username"`
	Email            string `json:"email"`
	PreviousUsername string `json:"previous_username"`
}

// DeleteUsernameResult is the response from deleting a claimed username.
type DeleteUsernameResult struct {
	ReleasedUsername string `json:"released_username"`
	CooldownUntil    string `json:"cooldown_until"`
	Message          string `json:"message"`
}

// RotateKeysOptions configures key rotation behavior.
type RotateKeysOptions struct {
	// RegisterWithHai controls whether to re-register with HAI after local
	// rotation. Default: true (when called with nil options).
	RegisterWithHai *bool
	// ConfigPath overrides the jacs.config.json path for config updates.
	// If empty, the standard discovery order is used.
	ConfigPath string
}

// RotationResult contains the outcome of a key rotation operation.
type RotationResult struct {
	// JacsID is the agent's stable JACS identifier (unchanged by rotation).
	JacsID string `json:"jacs_id"`
	// OldVersion is the agent version before rotation.
	OldVersion string `json:"old_version"`
	// NewVersion is the newly assigned agent version.
	NewVersion string `json:"new_version"`
	// NewPublicKeyHash is the SHA-256 hex digest of the new public key (SPKI DER).
	NewPublicKeyHash string `json:"new_public_key_hash"`
	// RegisteredWithHai indicates whether re-registration with HAI succeeded.
	RegisteredWithHai bool `json:"registered_with_hai"`
	// SignedAgentJSON is the complete self-signed JACS agent document (JSON string).
	SignedAgentJSON string `json:"signed_agent_json"`
}

// RegisterNewAgentOptions configures RegisterNewAgent behavior.
type RegisterNewAgentOptions struct {
	Domain        string
	Description   string
	OwnerEmail    string
	Password      string
	KeyDirectory  string
	DataDirectory string
	ConfigPath    string
	Algorithm     string
	Quiet         bool
}

// RegisterResult is the result of RegisterNewAgent. The FFI returns a merged
// JSON containing both CreateAgentResult fields (paths, algorithm) and
// RegistrationResult fields (agent_id, jacs_id, success).
type RegisterResult struct {
	// From CreateAgentResult
	AgentID        string `json:"agent_id"`
	Name           string `json:"name"`
	PublicKeyPath  string `json:"public_key_path"`
	ConfigPath     string `json:"config_path"`
	Version        string `json:"version"`
	Algorithm      string `json:"algorithm"`
	PrivateKeyPath string `json:"private_key_path"`
	DataDirectory  string `json:"data_directory"`
	KeyDirectory   string `json:"key_directory"`
	Domain         string `json:"domain"`
	DNSRecord      string `json:"dns_record"`

	// From RegistrationResult
	Success      bool                `json:"success"`
	JacsID       string              `json:"jacs_id"`
	DNSVerified  bool                `json:"dns_verified"`
	Registrations []RegistrationEntry `json:"registrations"`
	RegisteredAt string              `json:"registered_at"`
	Message      string              `json:"message,omitempty"`
}

// RegistrationEntry is an individual registration record returned by the server.
type RegistrationEntry struct {
	KeyID         string `json:"key_id"`
	Algorithm     string `json:"algorithm"`
	SignatureJSON string `json:"signature_json"`
	SignedAt      string `json:"signed_at"`
}

// SendEmailOptions configures an email send request.
// EmailAttachment represents a file attachment for an email.
type EmailAttachment struct {
	Filename    string `json:"filename"`
	ContentType string `json:"content_type"`
	Data        []byte `json:"-"`                          // Raw bytes (not sent in JSON)
	DataBase64  string `json:"data_base64,omitempty"`       // Base64-encoded data for API
}

type SendEmailOptions struct {
	To          string            `json:"to"`
	Subject     string            `json:"subject"`
	Body        string            `json:"body"`
	InReplyTo   string            `json:"in_reply_to,omitempty"`
	Attachments []EmailAttachment `json:"attachments,omitempty"`
	CC          []string          `json:"cc,omitempty"`
	BCC         []string          `json:"bcc,omitempty"`
	Labels      []string          `json:"labels,omitempty"`
}

// SearchOptions configures a message search request.
type SearchOptions struct {
	Q              string `json:"q,omitempty"`
	Direction      string `json:"direction,omitempty"`
	FromAddress    string `json:"from_address,omitempty"`
	ToAddress      string `json:"to_address,omitempty"`
	Limit          int    `json:"limit,omitempty"`
	Offset         int    `json:"offset,omitempty"`
	IsRead         *bool  `json:"is_read,omitempty"`
	JacsVerified   *bool  `json:"jacs_verified,omitempty"`
	Folder         string `json:"folder,omitempty"`
	Label          string `json:"label,omitempty"`
	HasAttachments *bool  `json:"has_attachments,omitempty"`
	Since          string `json:"since,omitempty"` // Filter messages since this date (ISO 8601 or YYYY-MM-DD).
	Until          string `json:"until,omitempty"` // Filter messages until this date (ISO 8601 or YYYY-MM-DD).
}

// UnreadCountResult is the response from the unread count endpoint.
type UnreadCountResult struct {
	Count int `json:"count"`
}

// SendEmailResult is the response from sending an email.
type SendEmailResult struct {
	MessageID string `json:"message_id"`
	Status    string `json:"status"`
}

// EmailMessage represents an email message in the agent's mailbox.
type EmailMessage struct {
	ID             string   `json:"id"`
	Direction      string   `json:"direction"`
	FromAddress    string   `json:"from_address"`
	ToAddress      string   `json:"to_address"`
	Subject        string   `json:"subject"`
	BodyText       string   `json:"body_text"`
	MessageID      string   `json:"message_id,omitempty"`
	InReplyTo      string   `json:"in_reply_to,omitempty"`
	IsRead         bool     `json:"is_read"`
	DeliveryStatus string   `json:"delivery_status"`
	CreatedAt      string   `json:"created_at"`
	ReadAt         *string  `json:"read_at"`
	JacsVerified   *bool    `json:"jacs_verified"`
	CcAddresses    []string `json:"cc_addresses,omitempty"`
	Labels         []string `json:"labels,omitempty"`
	TrustScore     *float64 `json:"trust_score,omitempty"`
	Folder         string   `json:"folder,omitempty"`
}

// ListMessagesResponse is the wrapper returned by the list messages API.
type ListMessagesResponse struct {
	Messages []EmailMessage `json:"messages"`
	Total    int            `json:"total"`
	Unread   int            `json:"unread"`
}

// ListMessagesOptions configures a list messages request.
type ListMessagesOptions struct {
	Limit          int    // Maximum number of messages to return.
	Offset         int    // Number of messages to skip.
	Direction      string // "inbound" or "outbound".
	IsRead         *bool  // Filter by read status (nil = no filter).
	Folder         string // Filter by folder ("inbox", "archive").
	Label          string // Filter by label.
	HasAttachments *bool  // Filter by attachment presence (nil = no filter).
	Since          string // Filter messages since this date (ISO 8601 or YYYY-MM-DD).
	Until          string // Filter messages until this date (ISO 8601 or YYYY-MM-DD).
}

// MarkReadResult is the response from marking a message as read.
type MarkReadResult struct {
	Success bool `json:"success"`
}

// EmailVolumeInfo contains volume statistics from the email status response.
type EmailVolumeInfo struct {
	SentTotal     int64 `json:"sent_total"`
	ReceivedTotal int64 `json:"received_total"`
	Sent24h       int64 `json:"sent_24h"`
}

// EmailDeliveryInfo contains delivery metrics from the email status response.
type EmailDeliveryInfo struct {
	BounceCount     int     `json:"bounce_count"`
	SpamReportCount int     `json:"spam_report_count"`
	DeliveryRate    float64 `json:"delivery_rate"`
}

// EmailReputationInfo contains reputation scoring from the email status response.
type EmailReputationInfo struct {
	Score      float64  `json:"score"`
	Tier       string   `json:"tier"`
	EmailScore float64  `json:"email_score"`
	HaiScore   *float64 `json:"hai_score"`
}

// EmailStatus describes the agent's email usage and limits.
type EmailStatus struct {
	Email              string               `json:"email"`
	Status             string               `json:"status"`
	Tier               string               `json:"tier"`
	BillingTier        string               `json:"billing_tier"`
	MessagesSent24h    int                  `json:"messages_sent_24h"`
	DailyLimit         int                  `json:"daily_limit"`
	DailyUsed          int                  `json:"daily_used"`
	ResetsAt           string               `json:"resets_at"`
	MessagesSentTotal  int                  `json:"messages_sent_total"`
	ExternalEnabled    bool                 `json:"external_enabled"`
	ExternalSendsToday int                  `json:"external_sends_today"`
	LastTierChange     *string              `json:"last_tier_change"`
	Volume             *EmailVolumeInfo     `json:"volume,omitempty"`
	Delivery           *EmailDeliveryInfo   `json:"delivery,omitempty"`
	Reputation         *EmailReputationInfo `json:"reputation,omitempty"`
}

// KeyRegistryResponse is the response from GET /api/agents/keys/{email}.
type KeyRegistryResponse struct {
	Email          string `json:"email"`
	JacsID         string `json:"jacs_id"`
	PublicKey      string `json:"public_key"`
	Algorithm      string `json:"algorithm"`
	ReputationTier string `json:"reputation_tier"`
	RegisteredAt   string `json:"registered_at"`
}

// FieldStatus represents the verification status of a single field.
type FieldStatus string

const (
	FieldStatusPass         FieldStatus = "pass"
	FieldStatusModified     FieldStatus = "modified"
	FieldStatusFail         FieldStatus = "fail"
	FieldStatusUnverifiable FieldStatus = "unverifiable"
)

// FieldResult is the verification result for a single email field.
type FieldResult struct {
	Field         string      `json:"field"`
	Status        FieldStatus `json:"status"`
	OriginalHash  *string     `json:"original_hash,omitempty"`
	CurrentHash   *string     `json:"current_hash,omitempty"`
	OriginalValue *string     `json:"original_value,omitempty"`
	CurrentValue  *string     `json:"current_value,omitempty"`
}

// ChainEntry represents an entry in a JACS email forwarding chain.
type ChainEntry struct {
	Signer    string `json:"signer"`
	JacsID    string `json:"jacs_id"`
	Valid     bool   `json:"valid"`
	Forwarded bool   `json:"forwarded"`
}

// EmailVerificationResultV2 is the result of verifying a JACS attachment-signed email.
type EmailVerificationResultV2 struct {
	Valid               bool          `json:"valid"`
	JacsID              string        `json:"jacs_id"`
	Algorithm           string        `json:"algorithm"`
	ReputationTier      string        `json:"reputation_tier"`
	DNSVerified         *bool         `json:"dns_verified"`
	FieldResults        []FieldResult `json:"field_results"`
	Chain               []ChainEntry  `json:"chain"`
	Error               *string       `json:"error,omitempty"`
	AgentStatus         *string       `json:"agent_status,omitempty"`
	BenchmarksCompleted []string      `json:"benchmarks_completed,omitempty"`
}

// HaiAPIError represents a structured error response from the HAI API.
type HaiAPIError struct {
	Message   string `json:"message"`
	ErrorCode string `json:"error_code"`
	Status    int    `json:"status"`
	RequestID string `json:"request_id"`
}

// Error implements the error interface.
func (e *HaiAPIError) Error() string {
	if e.ErrorCode != "" {
		return fmt.Sprintf("%s (code: %s, HTTP %d)", e.Message, e.ErrorCode, e.Status)
	}
	return fmt.Sprintf("%s (HTTP %d)", e.Message, e.Status)
}

// RawEmailResult is the result of GetRawEmail — raw RFC 5322 bytes for
// local JACS verification.
//
// Byte-fidelity (PRD R2): RawEmail, when present, is byte-identical to what
// JACS signed. No trimming, no line-ending normalization, no UTF-8 lossy.
//
// When Available is false, RawEmail is nil and OmittedReason is one of:
//   - "not_stored": legacy row predating the feature.
//   - "oversize":  MIME exceeded the 25 MB storage cap.
//
// The server distinguishes "field present and empty" from "field absent (null)"
// on the wire. Go exposes the primary fields as plain string/int for ergonomics
// (see PRD §4.5 Go signature), and tracks nullability with the optional
// hasSizeBytes / hasOmittedReason / hasRfcMessageID bookkeeping consulted via
// RawEmailResult.HasSizeBytes / HasOmittedReason / HasRfcMessageID.
// Happy-path (available=true) payloads usually have a non-null size_bytes, so
// the sentinel convention ("" / 0 = unknown) is enough for most callers; the
// accessors are there when the distinction matters.
type RawEmailResult struct {
	MessageID     string
	RfcMessageID  string
	Available     bool
	RawEmail      []byte
	SizeBytes     int
	OmittedReason string

	// Explicit null-vs-default trackers. Set to true when the wire carried a
	// non-null value for the corresponding field.
	hasRfcMessageID  bool
	hasSizeBytes     bool
	hasOmittedReason bool
}

// HasSizeBytes reports whether the server supplied a non-null size_bytes.
// Returns false for legacy rows and any payload where size_bytes was null.
// Happy-path rows always return true.
func (r *RawEmailResult) HasSizeBytes() bool { return r.hasSizeBytes }

// HasOmittedReason reports whether the server supplied a non-null omitted_reason.
// Returns true iff OmittedReason was explicitly set to "not_stored" or "oversize".
// Happy-path rows (available=true, bytes stored) always return false.
func (r *RawEmailResult) HasOmittedReason() bool { return r.hasOmittedReason }

// HasRfcMessageID reports whether the server supplied a non-null rfc_message_id.
func (r *RawEmailResult) HasRfcMessageID() bool { return r.hasRfcMessageID }

// Contact represents a contact derived from email history.
type Contact struct {
	Email          string `json:"email"`
	DisplayName    string `json:"display_name,omitempty"`
	LastContact    string `json:"last_contact"`
	JacsVerified   bool   `json:"jacs_verified,omitempty"`
	ReputationTier string `json:"reputation_tier,omitempty"`
}

// ContactsResponse is the wrapper returned by the contacts API.
type ContactsResponse struct {
	Contacts []Contact `json:"contacts"`
}

// EmailTemplate represents an agent email template.
type EmailTemplate struct {
	ID           string  `json:"id"`
	AgentID      string  `json:"agent_id"`
	Name         string  `json:"name"`
	HowToSend    *string `json:"how_to_send,omitempty"`
	HowToRespond *string `json:"how_to_respond,omitempty"`
	Goal         *string `json:"goal,omitempty"`
	Rules        *string `json:"rules,omitempty"`
	CreatedAt    string  `json:"created_at"`
	UpdatedAt    string  `json:"updated_at"`
}

// CreateEmailTemplateOptions are the options for creating an email template.
type CreateEmailTemplateOptions struct {
	Name         string  `json:"name"`
	HowToSend    *string `json:"how_to_send,omitempty"`
	HowToRespond *string `json:"how_to_respond,omitempty"`
	Goal         *string `json:"goal,omitempty"`
	Rules        *string `json:"rules,omitempty"`
}

// UpdateEmailTemplateOptions are the options for updating an email template.
type UpdateEmailTemplateOptions struct {
	Name         *string `json:"name,omitempty"`
	HowToSend    *string `json:"how_to_send,omitempty"`
	HowToRespond *string `json:"how_to_respond,omitempty"`
	Goal         *string `json:"goal,omitempty"`
	Rules        *string `json:"rules,omitempty"`
}

// ListEmailTemplatesOptions are the options for listing email templates.
type ListEmailTemplatesOptions struct {
	Limit  *int    `json:"limit,omitempty"`
	Offset *int    `json:"offset,omitempty"`
	Q      *string `json:"q,omitempty"`
}

// ListEmailTemplatesResult is the response from listing email templates.
type ListEmailTemplatesResult struct {
	Templates []EmailTemplate `json:"templates"`
	Total     int             `json:"total"`
	Limit     int             `json:"limit"`
	Offset    int             `json:"offset"`
}

// ForwardOptions configures a forward email request.
type ForwardOptions struct {
	MessageID string `json:"message_id"`
	To        string `json:"to"`
	Comment   string `json:"comment,omitempty"`
}

// Sentinel error types for email-related API errors.
var (
	ErrEmailNotActive    = errors.New("agent email is not active")
	ErrRecipientNotFound = errors.New("recipient not found")
	ErrEmailRateLimited  = errors.New("email rate limited")
)
