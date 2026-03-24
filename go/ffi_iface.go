package haiai

import "encoding/json"

// FFIClient defines the interface for the Rust FFI layer.
// This is satisfied by ffi.Client (CGo) and by test mocks.
// Every method returns either json.RawMessage (the "ok" payload) or an error.
type FFIClient interface {
	Close()

	// Registration & Identity
	Hello(includeTest bool) (json.RawMessage, error)
	CheckUsername(username string) (json.RawMessage, error)
	Register(optionsJSON string) (json.RawMessage, error)
	RegisterNewAgent(optionsJSON string) (json.RawMessage, error)
	RotateKeys(optionsJSON string) (json.RawMessage, error)
	UpdateAgent(agentData string) (json.RawMessage, error)
	SubmitResponse(paramsJSON string) (json.RawMessage, error)
	VerifyStatus(agentID string) (json.RawMessage, error)

	// Username
	ClaimUsername(agentID, username string) (json.RawMessage, error)
	UpdateUsername(agentID, username string) (json.RawMessage, error)
	DeleteUsername(agentID string) (json.RawMessage, error)

	// Email Core
	SendEmail(optionsJSON string) (json.RawMessage, error)
	SendSignedEmail(optionsJSON string) (json.RawMessage, error)
	ListMessages(optionsJSON string) (json.RawMessage, error)
	UpdateLabels(paramsJSON string) (json.RawMessage, error)
	GetEmailStatus() (json.RawMessage, error)
	GetMessage(messageID string) (json.RawMessage, error)
	GetUnreadCount() (json.RawMessage, error)

	// Email Actions
	MarkRead(messageID string) error
	MarkUnread(messageID string) error
	DeleteMessage(messageID string) error
	Archive(messageID string) error
	Unarchive(messageID string) error
	ReplyWithOptions(paramsJSON string) (json.RawMessage, error)
	Forward(paramsJSON string) (json.RawMessage, error)

	// Search & Contacts
	SearchMessages(optionsJSON string) (json.RawMessage, error)
	Contacts() (json.RawMessage, error)

	// Key Operations
	FetchRemoteKey(jacsID, version string) (json.RawMessage, error)
	FetchKeyByHash(hash string) (json.RawMessage, error)
	FetchKeyByEmail(email string) (json.RawMessage, error)
	FetchKeyByDomain(domain string) (json.RawMessage, error)
	FetchAllKeys(jacsID string) (json.RawMessage, error)

	// Verification
	VerifyDocument(document string) (json.RawMessage, error)
	GetVerification(agentID string) (json.RawMessage, error)
	VerifyAgentDocument(requestJSON string) (json.RawMessage, error)

	// Benchmarks
	Benchmark(name, tier string) (json.RawMessage, error)
	FreeRun(transport string) (json.RawMessage, error)
	ProRun(optionsJSON string) (json.RawMessage, error)
	EnterpriseRun() error

	// JACS Delegation
	BuildAuthHeader() (string, error)
	SignMessage(message string) (string, error)
	CanonicalJSON(valueJSON string) (string, error)
	VerifyA2AArtifact(wrappedJSON string) (json.RawMessage, error)
	ExportAgentJSON() (json.RawMessage, error)

	// Client State
	JacsID() (string, error)
	SetHaiAgentID(id string) error
	SetAgentEmail(email string) error

	// Server Keys
	FetchServerKeys() (json.RawMessage, error)

	// Email Sign/Verify (raw, base64-encoded)
	SignEmailRaw(rawEmailB64 string) (json.RawMessage, error)
	VerifyEmailRaw(rawEmailB64 string) (json.RawMessage, error)

	// Attestations
	CreateAttestation(paramsJSON string) (json.RawMessage, error)
	ListAttestations(paramsJSON string) (json.RawMessage, error)
	GetAttestation(agentID, docID string) (json.RawMessage, error)
	VerifyAttestation(document string) (json.RawMessage, error)

	// Email Templates
	CreateEmailTemplate(optionsJSON string) (json.RawMessage, error)
	ListEmailTemplates(optionsJSON string) (json.RawMessage, error)
	GetEmailTemplate(templateID string) (json.RawMessage, error)
	UpdateEmailTemplate(templateID, optionsJSON string) (json.RawMessage, error)
	DeleteEmailTemplate(templateID string) (json.RawMessage, error)

	// SSE Streaming
	ConnectSSE() (uint64, error)
	SSENextEvent(handleID uint64) (json.RawMessage, error)
	SSEClose(handleID uint64)

	// WebSocket Streaming
	ConnectWS() (uint64, error)
	WSNextEvent(handleID uint64) (json.RawMessage, error)
	WSClose(handleID uint64)
}
