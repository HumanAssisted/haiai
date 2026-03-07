// A2A (Agent-to-Agent) quickstart using HAIAI Go facade APIs.
//
// Demonstrates:
//  1. Register a new JACS agent with HAI
//  2. Export an A2A agent card via Client.GetA2A()
//  3. Prepare register options with embedded card metadata
//  4. Sign and verify A2A task/result artifacts
//  5. Build a chain of custody
//  6. Generate .well-known discovery documents
package main

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"log"
	"time"

	haiai "github.com/HumanAssisted/haiai-go"
)

const HAIURL = haiai.DefaultEndpoint

func main() {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Minute)
	defer cancel()

	fmt.Println("=== Step 1: Register a new JACS agent with HAI ===")
	reg, err := haiai.RegisterNewAgentWithEndpoint(ctx, HAIURL, "a2a-demo-agent", &haiai.RegisterNewAgentOptions{
		OwnerEmail: "you@example.com",
	})
	if err != nil {
		log.Fatalf("registration failed: %v", err)
	}
	if reg.Registration == nil {
		log.Fatal("registration failed: empty registration response")
	}

	jacsID := reg.Registration.JacsID
	if jacsID == "" {
		jacsID = reg.Registration.AgentID
	}
	fmt.Printf("Agent registered: %s\n", jacsID)

	privateKey, err := haiai.ParsePrivateKey(reg.PrivateKey)
	if err != nil {
		log.Fatalf("parse generated private key: %v", err)
	}
	publicKey, err := haiai.ParsePublicKey(reg.PublicKey)
	if err != nil {
		log.Fatalf("parse generated public key: %v", err)
	}

	client, err := haiai.NewClient(
		haiai.WithEndpoint(HAIURL),
		haiai.WithJACSID(jacsID),
		haiai.WithPrivateKey(privateKey),
	)
	if err != nil {
		log.Fatalf("build client from generated credentials: %v", err)
	}
	a2a := client.GetA2A(haiai.A2ATrustPolicyVerified)

	fmt.Println("\n=== Step 2: Export A2A agent card with facade ===")
	agentData := map[string]interface{}{
		"jacsId":          jacsID,
		"jacsName":        "a2a-demo-agent",
		"jacsVersion":     "1.0.0",
		"jacsAgentDomain": "demo.example.com",
		"a2aProfile":      haiai.A2AProtocolVersion10,
		"jacsServices": []interface{}{
			map[string]interface{}{
				"name":               "conflict_mediation",
				"serviceDescription": "Mediate multi-party conflict with signed provenance.",
			},
		},
	}
	card := a2a.ExportAgentCard(agentData)
	cardJSON, _ := json.MarshalIndent(card, "", "  ")
	fmt.Println(string(cardJSON))

	fmt.Println("\n=== Step 3: Prepare register options with embedded card metadata ===")
	merged, err := a2a.RegisterOptionsWithAgentCard(haiai.RegisterOptions{
		AgentJSON:  reg.AgentJSON,
		PublicKey:  string(reg.PublicKey),
		OwnerEmail: "you@example.com",
	}, card)
	if err != nil {
		log.Fatalf("merge register options: %v", err)
	}
	var mergedObj map[string]interface{}
	if err := json.Unmarshal([]byte(merged.AgentJSON), &mergedObj); err != nil {
		log.Fatalf("decode merged agent_json: %v", err)
	}
	fmt.Printf("Merged agent_json includes a2aAgentCard: %v\n", mergedObj["a2aAgentCard"] != nil)

	fmt.Println("\n=== Step 4: Sign and verify task artifact ===")
	task := map[string]interface{}{
		"taskId":    "task-001",
		"operation": "mediate_conflict",
		"input": map[string]interface{}{
			"parties": []string{"Alice", "Bob"},
			"topic":   "Resource allocation disagreement",
		},
	}
	wrappedTask, err := a2a.SignArtifact(task, "task", nil)
	if err != nil {
		log.Fatalf("sign task artifact: %v", err)
	}
	taskVerify, err := a2a.VerifyArtifact(wrappedTask)
	if err != nil {
		log.Fatalf("verify task artifact: %v", err)
	}
	fmt.Printf("Task artifact valid: %v (signer=%s)\n", taskVerify.Valid, taskVerify.SignerID)

	fmt.Println("\n=== Step 5: Sign result with parent provenance + chain of custody ===")
	result := map[string]interface{}{
		"taskId": "task-001",
		"result": "Mediation successful. Shared schedule accepted.",
	}
	parent := map[string]interface{}{}
	if err := json.Unmarshal(mustJSON(wrappedTask), &parent); err != nil {
		log.Fatalf("convert parent artifact: %v", err)
	}
	wrappedResult, err := a2a.SignArtifact(result, "task-result", []map[string]interface{}{parent})
	if err != nil {
		log.Fatalf("sign result artifact: %v", err)
	}

	chain := a2a.CreateChainOfCustody([]*haiai.A2AWrappedArtifact{wrappedTask, wrappedResult})
	fmt.Printf("Chain length: %d\n", chain.TotalArtifacts)
	for _, entry := range chain.Chain {
		fmt.Printf("  [%s] by %s at %s\n", entry.ArtifactType, entry.AgentID, entry.Timestamp)
	}

	fmt.Println("\n=== Step 6: Generate .well-known bundle ===")
	publicKeyB64 := base64.RawURLEncoding.EncodeToString(publicKey)
	wellKnown := a2a.GenerateWellKnownDocuments(card, "", publicKeyB64, agentData)
	for path, doc := range wellKnown {
		docJSON, _ := json.MarshalIndent(doc, "", "  ")
		preview := string(docJSON)
		if len(preview) > 180 {
			preview = preview[:180] + "..."
		}
		fmt.Printf("\n%s:\n%s\n", path, preview)
	}

	fmt.Println("\nA2A quickstart complete.")
}

func mustJSON(v interface{}) []byte {
	raw, err := json.Marshal(v)
	if err != nil {
		panic(err)
	}
	return raw
}
