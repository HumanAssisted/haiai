package haisdk

import "encoding/json"

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
	JacsID        string          `json:"jacs_id"`
	Registered    bool            `json:"registered"`
	Registrations []Registration  `json:"registrations"`
	DNSVerified   bool            `json:"dns_verified"`
	RegisteredAt  string          `json:"registered_at"`
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
	JobID      string             `json:"job_id,omitempty"`
	ScenarioID string             `json:"scenario_id,omitempty"`
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
	Timestamp              string `json:"timestamp"`
	ClientIP               string `json:"client_ip"`
	HaiPublicKeyFingerprint string `json:"hai_public_key_fingerprint"`
	Message                string `json:"message"`
	HaiSignedAck           string `json:"hai_signed_ack"`
	HelloID                string `json:"hello_id"`
	TestScenario           string `json:"test_scenario,omitempty"`
}

// AttestationResult is the response from the attestation endpoint.
type AttestationResult struct {
	AgentID     string         `json:"agent_id"`
	Attestation json.RawMessage `json:"attestation"`
	Signatures  []HaiSignature `json:"signatures,omitempty"`
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
	Metadata      SignedDocumentMetadata  `json:"metadata"`
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

// CheckUsernameResult is the response from checking username availability.
type CheckUsernameResult struct {
	Available bool   `json:"available"`
	Username  string `json:"username"`
	Reason    string `json:"reason,omitempty"`
}

// ClaimUsernameResult is the response from claiming a username.
type ClaimUsernameResult struct {
	Username string `json:"username"`
	Email    string `json:"email"`
	AgentID  string `json:"agent_id"`
}

// RegisterNewAgentOptions configures RegisterNewAgent behavior.
type RegisterNewAgentOptions struct {
	Domain     string
	OwnerEmail string
	Quiet      bool
}

// RegisterResult is the result of RegisterNewAgent, containing
// the generated key material and registration response.
type RegisterResult struct {
	Registration *RegistrationResult
	PrivateKey   []byte // PEM-encoded Ed25519 private key
	PublicKey    []byte // PEM-encoded Ed25519 public key
	AgentJSON    string // The signed JACS agent document
}

// SendEmailOptions configures an email send request.
type SendEmailOptions struct {
	To        string `json:"to"`
	Subject   string `json:"subject"`
	Body      string `json:"body"`
	InReplyTo string `json:"in_reply_to,omitempty"`
}

// SendEmailResult is the response from sending an email.
type SendEmailResult struct {
	MessageID string `json:"message_id"`
	Status    string `json:"status"`
}

// EmailMessage represents an email message in the agent's mailbox.
type EmailMessage struct {
	ID          string  `json:"id"`
	FromAddress string  `json:"from_address"`
	ToAddress   string  `json:"to_address"`
	Subject     string  `json:"subject"`
	Body        string  `json:"body"`
	SentAt      string  `json:"sent_at"`
	ReadAt      *string `json:"read_at"`
	ThreadID    *string `json:"thread_id"`
}

// ListMessagesOptions configures a list messages request.
type ListMessagesOptions struct {
	Limit  int    // Maximum number of messages to return.
	Offset int    // Number of messages to skip.
	Folder string // "inbox", "outbox", or "all".
}

// MarkReadResult is the response from marking a message as read.
type MarkReadResult struct {
	Success bool `json:"success"`
}

// EmailStatus describes the agent's email usage and limits.
type EmailStatus struct {
	DailyLimit     int    `json:"daily_limit"`
	DailyUsed      int    `json:"daily_used"`
	ResetsAt       string `json:"resets_at"`
	ReputationTier string `json:"reputation_tier"`
	CurrentTier    string `json:"current_tier"`
}
