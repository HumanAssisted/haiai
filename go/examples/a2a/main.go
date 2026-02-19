// A2A (Agent-to-Agent) Quickstart using HAISDK Go.
//
// Demonstrates how to use haisdk with the A2A protocol (v0.4.0):
//  1. Register a JACS agent with HAI
//  2. Export the agent as an A2A Agent Card
//  3. Wrap an artifact with JACS provenance signature
//  4. Verify a wrapped artifact
//  5. Create a chain of custody for multi-agent workflows
//  6. Publish .well-known documents
//
// Prerequisites:
//
//	go get github.com/HumanAssisted/haisdk-go
//
// Usage:
//
//	go run .
package main

import (
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"log"
	"time"

	haisdk "github.com/HumanAssisted/haisdk-go"
)

const HAIURL = haisdk.DefaultEndpoint

// generateUUID produces a UUIDv4 string.
func generateUUID() string {
	var uuid [16]byte
	_, _ = rand.Read(uuid[:])
	uuid[6] = (uuid[6] & 0x0f) | 0x40 // version 4
	uuid[8] = (uuid[8] & 0x3f) | 0x80 // variant 2
	return fmt.Sprintf("%08x-%04x-%04x-%04x-%012x",
		uuid[0:4], uuid[4:6], uuid[6:8], uuid[8:10], uuid[10:16])
}

// ---------------------------------------------------------------------------
// A2A v0.4.0 Types
// ---------------------------------------------------------------------------

// A2AAgentCard is the A2A Agent Card (v0.4.0), published at
// /.well-known/agent-card.json for zero-config discovery.
type A2AAgentCard struct {
	Name                string              `json:"name"`
	Description         string              `json:"description"`
	Version             string              `json:"version"`
	ProtocolVersions    []string            `json:"protocolVersions"`
	SupportedInterfaces []A2AAgentInterface `json:"supportedInterfaces"`
	DefaultInputModes   []string            `json:"defaultInputModes"`
	DefaultOutputModes  []string            `json:"defaultOutputModes"`
	Capabilities        A2ACapabilities     `json:"capabilities"`
	Skills              []A2AAgentSkill     `json:"skills"`
	Metadata            map[string]string   `json:"metadata,omitempty"`
}

type A2AAgentInterface struct {
	URL             string `json:"url"`
	ProtocolBinding string `json:"protocolBinding"`
}

type A2ACapabilities struct {
	Extensions []A2AExtension `json:"extensions,omitempty"`
}

type A2AExtension struct {
	URI         string `json:"uri"`
	Description string `json:"description,omitempty"`
	Required    bool   `json:"required"`
}

type A2AAgentSkill struct {
	ID          string   `json:"id"`
	Name        string   `json:"name"`
	Description string   `json:"description"`
	Tags        []string `json:"tags"`
	Examples    []string `json:"examples,omitempty"`
}

// WrappedArtifact is a JACS-signed A2A artifact.
type WrappedArtifact struct {
	JacsID               string                   `json:"jacsId"`
	JacsVersion          string                   `json:"jacsVersion"`
	JacsType             string                   `json:"jacsType"`
	JacsLevel            string                   `json:"jacsLevel"`
	JacsVersionDate      string                   `json:"jacsVersionDate"`
	A2AArtifact          map[string]interface{}   `json:"a2aArtifact"`
	JacsParentSignatures []map[string]interface{} `json:"jacsParentSignatures,omitempty"`
	JacsSignature        *JacsArtifactSignature   `json:"jacsSignature,omitempty"`
}

type JacsArtifactSignature struct {
	AgentID   string `json:"agentID"`
	Date      string `json:"date"`
	Signature string `json:"signature"`
}

// ChainOfCustody tracks provenance across a multi-agent workflow.
type ChainOfCustody struct {
	Chain          []ChainEntry `json:"chainOfCustody"`
	Created        string       `json:"created"`
	TotalArtifacts int          `json:"totalArtifacts"`
}

type ChainEntry struct {
	ArtifactID       string `json:"artifactId"`
	ArtifactType     string `json:"artifactType"`
	Timestamp        string `json:"timestamp"`
	AgentID          string `json:"agentId"`
	SignaturePresent bool   `json:"signaturePresent"`
}

// ---------------------------------------------------------------------------
// A2A Helper Functions
// ---------------------------------------------------------------------------

// ExportAgentCard creates an A2A Agent Card from a JACS agent identity.
func ExportAgentCard(jacsID, agentName, domain string) *A2AAgentCard {
	baseURL := fmt.Sprintf("https://hai.ai/agent/%s", jacsID)
	if domain != "" {
		baseURL = fmt.Sprintf("https://%s/agent/%s", domain, jacsID)
	}

	return &A2AAgentCard{
		Name:             agentName,
		Description:      fmt.Sprintf("HAI-registered JACS agent: %s", agentName),
		Version:          "1.0.0",
		ProtocolVersions: []string{"0.4.0"},
		SupportedInterfaces: []A2AAgentInterface{
			{URL: baseURL, ProtocolBinding: "jsonrpc"},
		},
		DefaultInputModes:  []string{"text/plain", "application/json"},
		DefaultOutputModes: []string{"text/plain", "application/json"},
		Capabilities: A2ACapabilities{
			Extensions: []A2AExtension{
				{
					URI:         "urn:jacs:provenance-v1",
					Description: "JACS cryptographic document signing and verification",
					Required:    false,
				},
			},
		},
		Skills: []A2AAgentSkill{
			{
				ID:          "mediation",
				Name:        "conflict_mediation",
				Description: "Mediate conflicts between parties using de-escalation techniques",
				Tags:        []string{"jacs", "mediation", "conflict-resolution"},
				Examples:    []string{"Mediate a workplace dispute", "Help resolve a disagreement"},
			},
		},
		Metadata: map[string]string{
			"jacsId":         jacsID,
			"registeredWith": "hai.ai",
		},
	}
}

// WrapArtifactWithProvenance signs an A2A artifact with JACS provenance.
func WrapArtifactWithProvenance(
	privateKey ed25519.PrivateKey,
	jacsID string,
	artifact map[string]interface{},
	artifactType string,
	parentSignatures []map[string]interface{},
) (*WrappedArtifact, error) {
	now := time.Now().UTC().Format(time.RFC3339Nano)

	wrapped := &WrappedArtifact{
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

	// Canonical JSON for signing (Go encoding/json sorts map keys)
	canonical, err := json.Marshal(wrapped)
	if err != nil {
		return nil, fmt.Errorf("failed to marshal artifact: %w", err)
	}

	sig := haisdk.Sign(privateKey, canonical)
	wrapped.JacsSignature = &JacsArtifactSignature{
		AgentID:   jacsID,
		Date:      now,
		Signature: base64.StdEncoding.EncodeToString(sig),
	}

	return wrapped, nil
}

// VerifyWrappedArtifact checks a JACS-wrapped artifact's signature.
func VerifyWrappedArtifact(wrapped *WrappedArtifact) map[string]interface{} {
	if wrapped.JacsSignature == nil || wrapped.JacsSignature.Signature == "" {
		return map[string]interface{}{"valid": false, "error": "No signature found"}
	}

	// For full verification you would fetch the signer's public key from HAI
	// and use haisdk.Verify(). Here we show the structure:
	return map[string]interface{}{
		"valid":            true,
		"signerId":         wrapped.JacsSignature.AgentID,
		"artifactType":     wrapped.JacsType,
		"timestamp":        wrapped.JacsVersionDate,
		"originalArtifact": wrapped.A2AArtifact,
	}
}

// CreateChainOfCustody builds a chain of custody from wrapped artifacts.
func CreateChainOfCustody(artifacts []*WrappedArtifact) *ChainOfCustody {
	chain := make([]ChainEntry, 0, len(artifacts))
	for _, a := range artifacts {
		agentID := "unknown"
		hasSig := false
		if a.JacsSignature != nil {
			agentID = a.JacsSignature.AgentID
			hasSig = a.JacsSignature.Signature != ""
		}
		chain = append(chain, ChainEntry{
			ArtifactID:       a.JacsID,
			ArtifactType:     a.JacsType,
			Timestamp:        a.JacsVersionDate,
			AgentID:          agentID,
			SignaturePresent: hasSig,
		})
	}

	return &ChainOfCustody{
		Chain:          chain,
		Created:        time.Now().UTC().Format(time.RFC3339),
		TotalArtifacts: len(chain),
	}
}

// GenerateWellKnownDocuments creates the .well-known files for A2A discovery.
func GenerateWellKnownDocuments(card *A2AAgentCard, jacsID string) map[string]interface{} {
	return map[string]interface{}{
		"/.well-known/agent-card.json": card,
		"/.well-known/jacs-agent.json": map[string]interface{}{
			"jacsVersion":    "1.0",
			"agentId":        jacsID,
			"registeredWith": "hai.ai",
			"capabilities": map[string]bool{
				"signing":      true,
				"verification": true,
			},
			"endpoints": map[string]string{
				"verify": "/jacs/verify",
				"sign":   "/jacs/sign",
			},
		},
	}
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

func main() {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Minute)
	defer cancel()

	// --- Step 1: Register agent with HAI ---
	fmt.Println("=== Step 1: Register a JACS agent with HAI ===")
	reg, err := haisdk.RegisterNewAgentWithEndpoint(ctx, HAIURL, "a2a-demo-agent", &haisdk.RegisterNewAgentOptions{
		OwnerEmail: "you@example.com",
	})
	if err != nil {
		log.Fatalf("Registration failed: %v", err)
	}
	if reg.Registration == nil {
		log.Fatal("Registration failed: empty registration response")
	}
	jacsID := reg.Registration.JacsID
	if jacsID == "" {
		jacsID = reg.Registration.AgentID
	}
	fmt.Printf("Agent registered with ID: %s\n", jacsID)

	// Use generated bootstrap key material for artifact signing.
	privateKey, err := haisdk.ParsePrivateKey(reg.PrivateKey)
	if err != nil {
		log.Fatalf("Failed to parse generated private key: %v", err)
	}

	// --- Step 2: Export as A2A Agent Card ---
	fmt.Println("\n=== Step 2: Export A2A Agent Card (v0.4.0) ===")
	agentCard := ExportAgentCard(jacsID, "a2a-demo-agent", "demo.example.com")
	cardJSON, _ := json.MarshalIndent(agentCard, "", "  ")
	fmt.Println(string(cardJSON))

	// --- Step 3: Wrap artifact with JACS provenance ---
	fmt.Println("\n=== Step 3: Wrap artifact with JACS provenance ===")
	taskArtifact := map[string]interface{}{
		"taskId":    "task-001",
		"operation": "mediate_conflict",
		"input": map[string]interface{}{
			"parties": []string{"Alice", "Bob"},
			"topic":   "Resource allocation disagreement",
		},
	}
	wrapped, err := WrapArtifactWithProvenance(privateKey, jacsID, taskArtifact, "task", nil)
	if err != nil {
		log.Fatalf("Failed to wrap artifact: %v", err)
	}
	fmt.Printf("Wrapped artifact ID: %s\n", wrapped.JacsID)
	fmt.Printf("Artifact type: %s\n", wrapped.JacsType)
	fmt.Printf("Signed by: %s\n", wrapped.JacsSignature.AgentID)

	// --- Step 4: Verify the wrapped artifact ---
	fmt.Println("\n=== Step 4: Verify wrapped artifact ===")
	verification := VerifyWrappedArtifact(wrapped)
	fmt.Printf("Valid: %v\n", verification["valid"])
	fmt.Printf("Signer: %v\n", verification["signerId"])
	fmt.Printf("Type: %v\n", verification["artifactType"])

	// --- Step 5: Chain of custody (multi-agent workflow) ---
	fmt.Println("\n=== Step 5: Chain of custody ===")
	resultArtifact := map[string]interface{}{
		"taskId": "task-001",
		"result": "Mediation successful -- both parties agreed to shared schedule",
	}
	parentSigs := []map[string]interface{}{
		{
			"agentID":   wrapped.JacsSignature.AgentID,
			"date":      wrapped.JacsSignature.Date,
			"signature": wrapped.JacsSignature.Signature,
		},
	}
	wrappedResult, err := WrapArtifactWithProvenance(privateKey, jacsID, resultArtifact, "task-result", parentSigs)
	if err != nil {
		log.Fatalf("Failed to wrap result: %v", err)
	}

	chain := CreateChainOfCustody([]*WrappedArtifact{wrapped, wrappedResult})
	fmt.Printf("Chain length: %d\n", chain.TotalArtifacts)
	for _, entry := range chain.Chain {
		fmt.Printf("  [%s] by %s at %s\n", entry.ArtifactType, entry.AgentID, entry.Timestamp)
	}

	// --- Step 6: Generate .well-known documents ---
	fmt.Println("\n=== Step 6: .well-known documents ===")
	wellKnown := GenerateWellKnownDocuments(agentCard, jacsID)
	for path, doc := range wellKnown {
		docJSON, _ := json.MarshalIndent(doc, "", "  ")
		preview := string(docJSON)
		if len(preview) > 200 {
			preview = preview[:200] + "..."
		}
		fmt.Printf("\n%s:\n%s\n", path, preview)
	}

	fmt.Println("\nA2A quickstart complete!")
	fmt.Println("Serve the .well-known documents at your agent's domain for A2A discovery.")
}
