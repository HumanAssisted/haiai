package haiai

import (
	"context"
	"encoding/json"
	"fmt"
	"sort"
	"strings"
	"sync"
	"time"
)

const (
	// A2AProtocolVersion04 is the legacy A2A protocol profile used by current quickstarts.
	A2AProtocolVersion04 = "0.4.0"
	// A2AProtocolVersion10 is the current A2A protocol profile.
	A2AProtocolVersion10 = "1.0"
	// A2AJACSExtensionURI identifies JACS provenance support on an A2A card.
	A2AJACSExtensionURI = "urn:jacs:provenance-v1"
)

// A2ATrustPolicy controls runtime acceptance behavior for remote A2A agents.
type A2ATrustPolicy string

const (
	A2ATrustPolicyOpen     A2ATrustPolicy = "open"
	A2ATrustPolicyVerified A2ATrustPolicy = "verified"
	A2ATrustPolicyStrict   A2ATrustPolicy = "strict"
)

// A2AIntegration is a facade for A2A workflows built on top of this HAI client.
//
// It reuses SDK signing/transport/email capabilities while keeping A2A-specific
// behavior centralized and DRY.
type A2AIntegration struct {
	client      *Client
	trustPolicy A2ATrustPolicy

	mu          sync.RWMutex
	trustedCard map[string]json.RawMessage
}

// A2AAgentInterface declares an endpoint/protocol binding pair.
type A2AAgentInterface struct {
	URL             string `json:"url"`
	ProtocolBinding string `json:"protocolBinding"`
	ProtocolVersion string `json:"protocolVersion,omitempty"`
	Tenant          string `json:"tenant,omitempty"`
}

// A2AAgentExtension declares an extension on an A2A card.
type A2AAgentExtension struct {
	URI         string `json:"uri"`
	Description string `json:"description,omitempty"`
	Required    bool   `json:"required,omitempty"`
}

// A2AAgentCapabilities declares card-level capabilities.
type A2AAgentCapabilities struct {
	Streaming    *bool               `json:"streaming,omitempty"`
	PushNotify   *bool               `json:"pushNotifications,omitempty"`
	ExtendedCard *bool               `json:"extendedAgentCard,omitempty"`
	Extensions   []A2AAgentExtension `json:"extensions,omitempty"`
}

// A2AAgentSkill describes a callable capability.
type A2AAgentSkill struct {
	ID          string   `json:"id"`
	Name        string   `json:"name"`
	Description string   `json:"description"`
	Tags        []string `json:"tags"`
	Examples    []string `json:"examples,omitempty"`
	InputModes  []string `json:"inputModes,omitempty"`
	OutputModes []string `json:"outputModes,omitempty"`
}

// A2AAgentCard is the A2A agent-card payload shape used by HAIAI wrappers.
type A2AAgentCard struct {
	Name                string                 `json:"name"`
	Description         string                 `json:"description"`
	Version             string                 `json:"version"`
	ProtocolVersions    []string               `json:"protocolVersions,omitempty"`
	SupportedInterfaces []A2AAgentInterface    `json:"supportedInterfaces"`
	DefaultInputModes   []string               `json:"defaultInputModes"`
	DefaultOutputModes  []string               `json:"defaultOutputModes"`
	Capabilities        A2AAgentCapabilities   `json:"capabilities"`
	Skills              []A2AAgentSkill        `json:"skills"`
	Metadata            map[string]interface{} `json:"metadata,omitempty"`
}

// A2AArtifactSignature stores signature metadata for a wrapped artifact.
type A2AArtifactSignature struct {
	AgentID   string `json:"agentID"`
	Date      string `json:"date"`
	Signature string `json:"signature"`
}

// A2AWrappedArtifact wraps an A2A payload with JACS provenance metadata.
type A2AWrappedArtifact struct {
	JacsID               string                   `json:"jacsId"`
	JacsVersion          string                   `json:"jacsVersion"`
	JacsType             string                   `json:"jacsType"`
	JacsLevel            string                   `json:"jacsLevel"`
	JacsVersionDate      string                   `json:"jacsVersionDate"`
	A2AArtifact          map[string]interface{}   `json:"a2aArtifact"`
	JacsParentSignatures []map[string]interface{} `json:"jacsParentSignatures,omitempty"`
	JacsSignature        *A2AArtifactSignature    `json:"jacsSignature,omitempty"`
}

// A2AArtifactVerificationResult is the normalized verification result.
type A2AArtifactVerificationResult struct {
	Valid            bool                   `json:"valid"`
	SignerID         string                 `json:"signerId"`
	ArtifactType     string                 `json:"artifactType"`
	Timestamp        string                 `json:"timestamp"`
	OriginalArtifact map[string]interface{} `json:"originalArtifact"`
	Error            string                 `json:"error,omitempty"`
}

// A2ATrustAssessment represents trust-policy evaluation of a remote card.
type A2ATrustAssessment struct {
	Allowed        bool   `json:"allowed"`
	TrustLevel     string `json:"trustLevel"`
	JACSRegistered bool   `json:"jacsRegistered"`
	InTrustStore   bool   `json:"inTrustStore"`
	Reason         string `json:"reason"`
}

// A2AChainEntry is one chain-of-custody hop.
type A2AChainEntry struct {
	ArtifactID       string `json:"artifactId"`
	ArtifactType     string `json:"artifactType"`
	Timestamp        string `json:"timestamp"`
	AgentID          string `json:"agentId"`
	SignaturePresent bool   `json:"signaturePresent"`
}

// A2AChainOfCustody is a normalized provenance chain.
type A2AChainOfCustody struct {
	Chain          []A2AChainEntry `json:"chainOfCustody"`
	Created        string          `json:"created"`
	TotalArtifacts int             `json:"totalArtifacts"`
}

// A2AMediatedJobOptions configures mediated benchmark handling.
type A2AMediatedJobOptions struct {
	Transport    TransportType
	NotifyEmail  string
	EmailSubject string
}

// GetA2A returns an A2A facade bound to this client.
func (c *Client) GetA2A(policy ...A2ATrustPolicy) *A2AIntegration {
	selected := A2ATrustPolicyVerified
	if len(policy) > 0 && policy[0] != "" {
		selected = normalizeTrustPolicy(policy[0])
	}
	return &A2AIntegration{
		client:      c,
		trustPolicy: selected,
		trustedCard: map[string]json.RawMessage{},
	}
}

// ExportAgentCard builds an A2A card from JACS/agent metadata.
//
// When FFI is available, delegates to the Rust core for card generation.
// Falls back to pure-Go card construction when FFI is unavailable.
func (a *A2AIntegration) ExportAgentCard(agentData map[string]interface{}) *A2AAgentCard {
	// Try FFI delegation first.
	if a.client.ffi != nil {
		agentJSON, err := a.client.ffi.ExportAgentJSON()
		if err == nil {
			var card A2AAgentCard
			if json.Unmarshal(agentJSON, &card) == nil {
				return &card
			}
		}
		// Fall through to metadata-based card construction on any error.
	}

	return a.exportAgentCardFromData(agentData)
}

// exportAgentCardFromData constructs an A2AAgentCard from agent metadata fields.
func (a *A2AIntegration) exportAgentCardFromData(agentData map[string]interface{}) *A2AAgentCard {
	agentID := stringValue(agentData["jacsId"])
	if agentID == "" {
		agentID = a.client.jacsID
	}
	agentName := stringValue(agentData["jacsName"])
	if agentName == "" {
		agentName = a.client.jacsID
	}
	description := stringValue(agentData["jacsDescription"])
	if description == "" {
		description = fmt.Sprintf("HAIAI agent %s", agentName)
	}
	version := stringValue(agentData["jacsVersion"])
	if version == "" {
		version = "1.0.0"
	}

	profile := stringValue(agentData["a2aProfile"])
	if profile == "" {
		profile = A2AProtocolVersion04
	}

	baseURL := fmt.Sprintf("https://agent-%s.example.com", agentID)
	if domain := stringValue(agentData["jacsAgentDomain"]); domain != "" {
		baseURL = fmt.Sprintf("https://%s/agent/%s", strings.TrimPrefix(domain, "https://"), agentID)
	}

	supported := []A2AAgentInterface{
		{
			URL:             baseURL,
			ProtocolBinding: "jsonrpc",
		},
	}
	if profile == A2AProtocolVersion10 {
		supported[0].ProtocolVersion = A2AProtocolVersion10
	}

	skills := convertServicesToSkills(agentData["jacsServices"])

	card := &A2AAgentCard{
		Name:                agentName,
		Description:         description,
		Version:             version,
		SupportedInterfaces: supported,
		DefaultInputModes:   []string{"text/plain", "application/json"},
		DefaultOutputModes:  []string{"text/plain", "application/json"},
		Capabilities: A2AAgentCapabilities{
			Extensions: []A2AAgentExtension{
				{
					URI:         A2AJACSExtensionURI,
					Description: "JACS cryptographic document signing and verification",
					Required:    false,
				},
			},
		},
		Skills: skills,
		Metadata: map[string]interface{}{
			"jacsId":      agentID,
			"jacsVersion": version,
			"a2aProfile":  profile,
		},
	}
	if profile == A2AProtocolVersion04 {
		card.ProtocolVersions = []string{A2AProtocolVersion04}
	}
	return card
}

// SignArtifact wraps and signs an A2A artifact with JACS provenance.
//
// Requires an FFI client to be configured. Signing is performed via
// ffi.SignMessage() -- there is no local crypto fallback.
func (a *A2AIntegration) SignArtifact(
	artifact map[string]interface{},
	artifactType string,
	parentSignatures []map[string]interface{},
) (*A2AWrappedArtifact, error) {
	if a.client.ffi == nil {
		return nil, newError(ErrSigningFailed, "FFI client is not configured on client")
	}

	return a.signArtifactViaFFI(artifact, artifactType, parentSignatures)
}

// signArtifactViaFFI wraps the artifact and signs it using ffi.SignMessage().
func (a *A2AIntegration) signArtifactViaFFI(
	artifact map[string]interface{},
	artifactType string,
	parentSignatures []map[string]interface{},
) (*A2AWrappedArtifact, error) {
	now := time.Now().UTC().Format(time.RFC3339Nano)
	wrapped := &A2AWrappedArtifact{
		JacsID:          generateUUID(),
		JacsVersion:     "1.0.0",
		JacsType:        fmt.Sprintf("a2a-%s", artifactType),
		JacsLevel:       "artifact",
		JacsVersionDate: now,
		A2AArtifact:     artifact,
	}
	if len(parentSignatures) > 0 {
		wrapped.JacsParentSignatures = parentSignatures
	}

	canonical, err := canonicalArtifactBytes(wrapped)
	if err != nil {
		return nil, wrapError(ErrSigningFailed, err, "failed to canonicalize artifact")
	}

	sigB64, signErr := a.client.ffi.SignMessage(string(canonical))
	if signErr != nil {
		return nil, wrapError(ErrSigningFailed, signErr, "failed to sign artifact via FFI")
	}
	wrapped.JacsSignature = &A2AArtifactSignature{
		AgentID:   a.client.jacsID,
		Date:      now,
		Signature: sigB64,
	}
	return wrapped, nil
}

// VerifyArtifact verifies a wrapped artifact signature.
//
// Delegates to the FFI layer for cryptographic verification.
// Returns a failing result when FFI is unavailable (no local crypto fallback).
func (a *A2AIntegration) VerifyArtifact(
	wrapped *A2AWrappedArtifact,
	publicKeyPEM ...string,
) (*A2AArtifactVerificationResult, error) {
	if wrapped == nil {
		return &A2AArtifactVerificationResult{
			Valid:            false,
			Error:            "wrapped artifact is nil",
			OriginalArtifact: map[string]interface{}{},
		}, nil
	}

	// Try FFI delegation first.
	if a.client.ffi != nil {
		wrappedJSON, marshalErr := json.Marshal(wrapped)
		if marshalErr == nil {
			resultJSON, err := a.client.ffi.VerifyA2AArtifact(string(wrappedJSON))
			if err == nil {
				var result A2AArtifactVerificationResult
				if json.Unmarshal(resultJSON, &result) == nil {
					return &result, nil
				}
			}
			// Fall through to failing fallback on any error.
		}
	}

	return a.verifyArtifactFallback(wrapped, publicKeyPEM...)
}

// verifyArtifactFallback returns a failing verification result when FFI is
// unavailable. It extracts metadata from the wrapped artifact but does not
// attempt cryptographic verification (which requires the FFI/JACS backend).
func (a *A2AIntegration) verifyArtifactFallback(
	wrapped *A2AWrappedArtifact,
	publicKeyPEM ...string,
) (*A2AArtifactVerificationResult, error) {
	result := &A2AArtifactVerificationResult{
		Valid:            false,
		ArtifactType:     wrapped.JacsType,
		Timestamp:        wrapped.JacsVersionDate,
		OriginalArtifact: wrapped.A2AArtifact,
	}
	if wrapped.JacsSignature != nil {
		result.SignerID = wrapped.JacsSignature.AgentID
	}

	if wrapped.JacsSignature == nil || wrapped.JacsSignature.Signature == "" {
		result.Error = "no signature found"
		return result, nil
	}

	result.Error = "signature verification requires FFI (JACS) backend"
	return result, nil
}

// CreateChainOfCustody builds a chain-of-custody document from wrapped artifacts.
func (a *A2AIntegration) CreateChainOfCustody(artifacts []*A2AWrappedArtifact) *A2AChainOfCustody {
	chain := make([]A2AChainEntry, 0, len(artifacts))
	for _, artifact := range artifacts {
		if artifact == nil {
			continue
		}
		entry := A2AChainEntry{
			ArtifactID:       artifact.JacsID,
			ArtifactType:     artifact.JacsType,
			Timestamp:        artifact.JacsVersionDate,
			SignaturePresent: artifact.JacsSignature != nil && artifact.JacsSignature.Signature != "",
		}
		if artifact.JacsSignature != nil {
			entry.AgentID = artifact.JacsSignature.AgentID
		}
		chain = append(chain, entry)
	}
	return &A2AChainOfCustody{
		Chain:          chain,
		Created:        time.Now().UTC().Format(time.RFC3339),
		TotalArtifacts: len(chain),
	}
}

// GenerateWellKnownDocuments builds a standard .well-known bundle for A2A discovery.
func (a *A2AIntegration) GenerateWellKnownDocuments(
	agentCard *A2AAgentCard,
	jwsSignature string,
	publicKeyB64 string,
	agentData map[string]interface{},
) map[string]interface{} {
	card := map[string]interface{}{}
	if agentCard != nil {
		_ = convertStruct(agentCard, &card)
	}
	if jwsSignature != "" {
		card["signatures"] = []map[string]string{{"jws": jwsSignature}}
	}

	agentID := stringValue(agentData["jacsId"])
	if agentID == "" {
		agentID = a.client.jacsID
	}
	agentVersion := stringValue(agentData["jacsVersion"])
	if agentVersion == "" {
		agentVersion = "1.0.0"
	}

	return map[string]interface{}{
		"/.well-known/agent-card.json": card,
		"/.well-known/jwks.json": map[string]interface{}{
			"keys": []map[string]interface{}{
				{
					"kty": "OKP",
					"crv": "Ed25519",
					"kid": agentID,
					"use": "sig",
					"alg": "EdDSA",
					"x":   publicKeyB64,
				},
			},
		},
		"/.well-known/jacs-agent.json": map[string]interface{}{
			"jacsVersion":  "1.0",
			"agentId":      agentID,
			"agentVersion": agentVersion,
			"capabilities": map[string]bool{
				"signing":      true,
				"verification": true,
			},
			"endpoints": map[string]string{
				"verify": "/jacs/verify",
				"sign":   "/jacs/sign",
				"agent":  "/jacs/agent",
			},
		},
		"/.well-known/jacs-extension.json": map[string]interface{}{
			"uri":                A2AJACSExtensionURI,
			"name":               "JACS Document Provenance",
			"version":            "1.0",
			"a2aProtocolVersion": stringValue(agentData["a2aProfile"]),
		},
	}
}

// RegisterOptionsWithAgentCard returns Register options with A2A metadata merged
// into agent_json while preserving caller-provided registration fields.
func (a *A2AIntegration) RegisterOptionsWithAgentCard(
	opts RegisterOptions,
	agentCard *A2AAgentCard,
) (RegisterOptions, error) {
	if agentCard == nil {
		return opts, nil
	}

	merged, err := mergeAgentJSONWithA2ACard(opts.AgentJSON, agentCard)
	if err != nil {
		return opts, err
	}
	opts.AgentJSON = merged
	return opts, nil
}

// RegisterWithAgentCard merges A2A card metadata into agent_json and registers
// the agent using the standard HAI registration endpoint.
func (a *A2AIntegration) RegisterWithAgentCard(
	ctx context.Context,
	opts RegisterOptions,
	agentCard *A2AAgentCard,
) (*RegistrationResult, error) {
	prepared, err := a.RegisterOptionsWithAgentCard(opts, agentCard)
	if err != nil {
		return nil, err
	}
	return a.client.Register(ctx, prepared)
}

// AssessRemoteAgent applies trust policy to a remote A2A agent card.
//
// When the JACS CGo backend is loaded, this delegates to the Rust core for
// trust assessment. Uses local Go logic otherwise.
func (a *A2AIntegration) AssessRemoteAgent(
	agentCardJSON string,
	policy ...A2ATrustPolicy,
) (*A2ATrustAssessment, error) {
	resolvedPolicy := a.trustPolicy
	if len(policy) > 0 {
		resolvedPolicy = normalizeTrustPolicy(policy[0])
	}

	// FFI does not currently expose AssessA2AAgent, so use local logic.

	return a.assessRemoteAgentLocal(agentCardJSON, resolvedPolicy)
}

// assessRemoteAgentLocal is the pure-Go local logic for AssessRemoteAgent.
func (a *A2AIntegration) assessRemoteAgentLocal(
	agentCardJSON string,
	resolvedPolicy A2ATrustPolicy,
) (*A2ATrustAssessment, error) {
	var card map[string]interface{}
	if err := json.Unmarshal([]byte(agentCardJSON), &card); err != nil {
		return nil, fmt.Errorf("invalid agent card json: %w", err)
	}

	jacsRegistered := hasJACSExtension(card)
	agentID := extractCardAgentID(card)
	inTrustStore := agentID != "" && a.IsTrustedA2AAgent(agentID)

	trustLevel := "untrusted"
	if inTrustStore {
		trustLevel = "explicitly_trusted"
	} else if jacsRegistered {
		trustLevel = "jacs_verified"
	}

	result := &A2ATrustAssessment{
		Allowed:        false,
		TrustLevel:     trustLevel,
		JACSRegistered: jacsRegistered,
		InTrustStore:   inTrustStore,
	}

	switch resolvedPolicy {
	case A2ATrustPolicyOpen:
		result.Allowed = true
		result.Reason = "open policy: all agents accepted"
	case A2ATrustPolicyVerified:
		result.Allowed = jacsRegistered
		if result.Allowed {
			result.Reason = "verified policy: card declares JACS extension"
		} else {
			result.Reason = "verified policy: card missing JACS extension"
		}
	case A2ATrustPolicyStrict:
		result.Allowed = inTrustStore
		if result.Allowed {
			result.Reason = "strict policy: agent is in local trust store"
		} else {
			result.Reason = "strict policy: agent is not in local trust store"
		}
	default:
		result.Allowed = false
		result.Reason = fmt.Sprintf("unknown trust policy: %s", resolvedPolicy)
	}
	return result, nil
}

// TrustA2AAgent adds an A2A card to this integration's local trust set.
func (a *A2AIntegration) TrustA2AAgent(agentCardJSON string) (string, error) {
	var card map[string]interface{}
	if err := json.Unmarshal([]byte(agentCardJSON), &card); err != nil {
		return "", fmt.Errorf("invalid agent card json: %w", err)
	}
	agentID := extractCardAgentID(card)
	if agentID == "" {
		return "", fmt.Errorf("cannot trust card without metadata.jacsId")
	}

	a.mu.Lock()
	a.trustedCard[agentID] = json.RawMessage(agentCardJSON)
	a.mu.Unlock()
	return agentID, nil
}

// IsTrustedA2AAgent checks if an agent id is trusted in this integration.
func (a *A2AIntegration) IsTrustedA2AAgent(agentID string) bool {
	a.mu.RLock()
	defer a.mu.RUnlock()
	_, ok := a.trustedCard[agentID]
	return ok
}

// ListTrustedA2AAgents lists trusted agent ids.
func (a *A2AIntegration) ListTrustedA2AAgents() []string {
	a.mu.RLock()
	defer a.mu.RUnlock()
	keys := make([]string, 0, len(a.trustedCard))
	for id := range a.trustedCard {
		keys = append(keys, id)
	}
	sort.Strings(keys)
	return keys
}

// SendSignedArtifactEmail sends a wrapped artifact using the agent email channel.
func (a *A2AIntegration) SendSignedArtifactEmail(
	ctx context.Context,
	to, subject string,
	artifact *A2AWrappedArtifact,
) (*SendEmailResult, error) {
	if artifact == nil {
		return nil, fmt.Errorf("artifact is nil")
	}
	pretty, err := json.MarshalIndent(artifact, "", "  ")
	if err != nil {
		return nil, fmt.Errorf("failed to encode artifact: %w", err)
	}
	body := "Signed A2A artifact:\n\n" + string(pretty)
	return a.client.SendEmail(ctx, to, subject, body)
}

// OnMediatedBenchmarkJob composes HAI transport + A2A provenance + optional email dispatch.
//
// For each benchmark job event:
// 1) wraps/signs inbound event as `a2a-task`
// 2) invokes handler
// 3) wraps/signs handler output as `a2a-task-result`
// 4) submits response to HAI (with signed artifacts in metadata)
// 5) optionally emails the signed result artifact
func (a *A2AIntegration) OnMediatedBenchmarkJob(
	ctx context.Context,
	opts A2AMediatedJobOptions,
	handler func(context.Context, *A2AWrappedArtifact) (map[string]interface{}, error),
) error {
	transport := opts.Transport
	if transport == "" {
		transport = TransportSSE
	}

	handle := func(ctx context.Context, event AgentEvent) error {
		taskPayload := map[string]interface{}{
			"type":       event.Type,
			"jobId":      event.JobID,
			"scenarioId": event.ScenarioID,
			"config":     event.Config,
		}
		taskArtifact, err := a.SignArtifact(taskPayload, "task", nil)
		if err != nil {
			return err
		}

		resultPayload, err := handler(ctx, taskArtifact)
		if err != nil {
			return err
		}

		parent := map[string]interface{}{}
		if err := convertStruct(taskArtifact, &parent); err != nil {
			return err
		}
		resultArtifact, err := a.SignArtifact(resultPayload, "task-result", []map[string]interface{}{parent})
		if err != nil {
			return err
		}

		message := stringValue(resultPayload["message"])
		if message == "" {
			raw, _ := json.Marshal(resultPayload)
			message = string(raw)
		}

		metaObj := map[string]interface{}{
			"a2aTask":   taskArtifact,
			"a2aResult": resultArtifact,
		}
		metaRaw, err := json.Marshal(metaObj)
		if err != nil {
			return err
		}
		processing := uint64(0)
		_, err = a.client.SubmitResponse(ctx, event.JobID, ModerationResponse{
			Message:          message,
			Metadata:         metaRaw,
			ProcessingTimeMs: &processing,
		})
		if err != nil {
			return err
		}

		if opts.NotifyEmail != "" {
			subject := opts.EmailSubject
			if subject == "" {
				subject = fmt.Sprintf("A2A mediated result for job %s", event.JobID)
			}
			if _, err := a.SendSignedArtifactEmail(ctx, opts.NotifyEmail, subject, resultArtifact); err != nil {
				return err
			}
		}
		return nil
	}

	switch transport {
	case TransportWS:
		return a.client.ConnectWSWithHandler(ctx, func(ctx context.Context, _ *WSConnection, event AgentEvent) error {
			if event.Type != "benchmark_job" {
				return nil
			}
			return handle(ctx, event)
		})
	default:
		return a.client.OnBenchmarkJob(ctx, func(ctx context.Context, event AgentEvent) error {
			if event.Type != "benchmark_job" {
				return nil
			}
			return handle(ctx, event)
		})
	}
}

func normalizeTrustPolicy(policy A2ATrustPolicy) A2ATrustPolicy {
	switch policy {
	case A2ATrustPolicyOpen, A2ATrustPolicyVerified, A2ATrustPolicyStrict:
		return policy
	default:
		return A2ATrustPolicyVerified
	}
}

func convertServicesToSkills(raw interface{}) []A2AAgentSkill {
	services, ok := raw.([]interface{})
	if !ok || len(services) == 0 {
		return []A2AAgentSkill{
			{
				ID:          "verify-signature",
				Name:        "verify_signature",
				Description: "Verify JACS document signatures",
				Tags:        []string{"jacs", "verification", "cryptography"},
			},
		}
	}

	skills := make([]A2AAgentSkill, 0, len(services))
	for _, serviceRaw := range services {
		service, ok := serviceRaw.(map[string]interface{})
		if !ok {
			continue
		}

		serviceName := stringValue(service["name"])
		if serviceName == "" {
			serviceName = stringValue(service["serviceDescription"])
		}
		if serviceName == "" {
			serviceName = "service"
		}
		serviceDesc := stringValue(service["serviceDescription"])
		if serviceDesc == "" {
			serviceDesc = "No description"
		}

		skills = append(skills, A2AAgentSkill{
			ID:          slugify(serviceName),
			Name:        serviceName,
			Description: serviceDesc,
			Tags:        []string{"jacs", slugify(serviceName)},
		})
	}

	if len(skills) == 0 {
		return []A2AAgentSkill{
			{
				ID:          "verify-signature",
				Name:        "verify_signature",
				Description: "Verify JACS document signatures",
				Tags:        []string{"jacs", "verification", "cryptography"},
			},
		}
	}
	return skills
}

func slugify(in string) string {
	s := strings.ToLower(strings.TrimSpace(in))
	s = strings.ReplaceAll(s, " ", "-")
	s = strings.ReplaceAll(s, "_", "-")
	var out []rune
	for _, ch := range s {
		if (ch >= 'a' && ch <= 'z') || (ch >= '0' && ch <= '9') || ch == '-' {
			out = append(out, ch)
		}
	}
	if len(out) == 0 {
		return "skill"
	}
	return string(out)
}

func stringValue(v interface{}) string {
	if s, ok := v.(string); ok {
		return s
	}
	return ""
}

func convertStruct(in interface{}, out interface{}) error {
	raw, err := json.Marshal(in)
	if err != nil {
		return err
	}
	return json.Unmarshal(raw, out)
}

func mergeAgentJSONWithA2ACard(agentJSON string, card *A2AAgentCard) (string, error) {
	if strings.TrimSpace(agentJSON) == "" {
		return "", fmt.Errorf("agent_json is required")
	}

	var agentDoc map[string]interface{}
	if err := json.Unmarshal([]byte(agentJSON), &agentDoc); err != nil {
		return "", fmt.Errorf("invalid agent_json: %w", err)
	}

	cardObj := map[string]interface{}{}
	if err := convertStruct(card, &cardObj); err != nil {
		return "", fmt.Errorf("failed to convert A2A card: %w", err)
	}
	agentDoc["a2aAgentCard"] = cardObj

	if _, exists := agentDoc["skills"]; !exists && len(card.Skills) > 0 {
		agentDoc["skills"] = card.Skills
	}
	if _, exists := agentDoc["capabilities"]; !exists {
		agentDoc["capabilities"] = card.Capabilities
	}

	meta, ok := agentDoc["metadata"].(map[string]interface{})
	if !ok || meta == nil {
		meta = map[string]interface{}{}
	}
	meta["a2aProfile"] = resolveCardProfile(card)
	meta["a2aSkillsCount"] = len(card.Skills)
	agentDoc["metadata"] = meta

	encoded, err := json.Marshal(agentDoc)
	if err != nil {
		return "", fmt.Errorf("failed to encode merged agent_json: %w", err)
	}
	return string(encoded), nil
}

func canonicalArtifactBytes(wrapped *A2AWrappedArtifact) ([]byte, error) {
	clone := *wrapped
	clone.JacsSignature = nil
	return json.Marshal(clone)
}

func hasJACSExtension(card map[string]interface{}) bool {
	capabilities, ok := card["capabilities"].(map[string]interface{})
	if !ok {
		return false
	}
	extensions, ok := capabilities["extensions"].([]interface{})
	if !ok {
		return false
	}
	for _, extRaw := range extensions {
		ext, ok := extRaw.(map[string]interface{})
		if !ok {
			continue
		}
		if stringValue(ext["uri"]) == A2AJACSExtensionURI {
			return true
		}
	}
	return false
}

func extractCardAgentID(card map[string]interface{}) string {
	meta, ok := card["metadata"].(map[string]interface{})
	if !ok {
		return ""
	}
	return stringValue(meta["jacsId"])
}

func resolveCardProfile(card *A2AAgentCard) string {
	if card == nil {
		return A2AProtocolVersion04
	}
	if raw, ok := card.Metadata["a2aProfile"]; ok {
		if profile := stringValue(raw); profile != "" {
			return profile
		}
	}
	if len(card.ProtocolVersions) > 0 && strings.TrimSpace(card.ProtocolVersions[0]) != "" {
		return card.ProtocolVersions[0]
	}
	for _, iface := range card.SupportedInterfaces {
		if strings.TrimSpace(iface.ProtocolVersion) != "" {
			return iface.ProtocolVersion
		}
	}
	return A2AProtocolVersion04
}

