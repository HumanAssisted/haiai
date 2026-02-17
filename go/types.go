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

// StatusResult contains the registration status of an agent.
type StatusResult struct {
	Registered     bool     `json:"registered"`
	AgentID        string   `json:"agent_id"`
	RegistrationID string   `json:"registration_id"`
	RegisteredAt   string   `json:"registered_at"`
	HaiSignatures  []string `json:"hai_signatures"`
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
	Suite       string                `json:"suite"`
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
	Message string `json:"message"`
	AgentID string `json:"agent_id,omitempty"`
}

// AttestationResult is the response from the attestation endpoint.
type AttestationResult struct {
	AgentID     string         `json:"agent_id"`
	Attestation json.RawMessage `json:"attestation"`
	Signatures  []HaiSignature `json:"signatures,omitempty"`
}

// VerifyResult is the response from verifying another agent.
type VerifyResult struct {
	Valid       bool           `json:"valid"`
	AgentID     string         `json:"agent_id"`
	Signatures  []HaiSignature `json:"signatures,omitempty"`
	Errors      []string       `json:"errors,omitempty"`
}
