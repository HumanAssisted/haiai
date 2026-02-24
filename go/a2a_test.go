package haisdk

import (
	"encoding/json"
	"os"
	"testing"
)

func mustA2AIntegration(t *testing.T) *A2AIntegration {
	t.Helper()
	_, priv, err := GenerateKeyPair()
	if err != nil {
		t.Fatalf("GenerateKeyPair: %v", err)
	}

	client, err := NewClient(
		WithJACSID("demo-agent"),
		WithPrivateKey(priv),
	)
	if err != nil {
		t.Fatalf("NewClient: %v", err)
	}
	return client.GetA2A()
}

func loadA2AFixture(t *testing.T, name string) map[string]interface{} {
	t.Helper()
	path := "../fixtures/a2a/" + name
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read fixture %s: %v", name, err)
	}

	var out map[string]interface{}
	if err := json.Unmarshal(data, &out); err != nil {
		t.Fatalf("decode fixture %s: %v", name, err)
	}
	return out
}

func TestA2AFixturesLoad(t *testing.T) {
	cardV04 := loadA2AFixture(t, "agent_card.v04.json")
	cardV10 := loadA2AFixture(t, "agent_card.v10.json")
	wrapped := loadA2AFixture(t, "wrapped_task.with_parents.json")
	wellKnown := loadA2AFixture(t, "well_known_bundle.v10.json")

	if got := cardV04["name"]; got != "HAISDK Demo Agent" {
		t.Fatalf("card v0.4 name = %v", got)
	}
	if got := wrapped["jacsType"]; got != "a2a-task-result" {
		t.Fatalf("wrapped jacsType = %v", got)
	}
	if _, ok := wellKnown["/.well-known/agent-card.json"].(map[string]interface{}); !ok {
		t.Fatalf("well-known bundle missing /.well-known/agent-card.json")
	}

	supported, ok := cardV10["supportedInterfaces"].([]interface{})
	if !ok || len(supported) == 0 {
		t.Fatalf("card v1.0 supportedInterfaces missing")
	}
	first, ok := supported[0].(map[string]interface{})
	if !ok {
		t.Fatalf("card v1.0 supportedInterfaces[0] is not object")
	}
	if got := first["protocolVersion"]; got != "1.0" {
		t.Fatalf("card v1.0 protocolVersion = %v", got)
	}
}

func TestA2ASignAndVerifyRoundtrip(t *testing.T) {
	a2a := mustA2AIntegration(t)

	task := map[string]interface{}{
		"taskId": "task-1",
		"input":  "hello",
	}
	wrapped, err := a2a.SignArtifact(task, "task", nil)
	if err != nil {
		t.Fatalf("SignArtifact: %v", err)
	}

	result, err := a2a.VerifyArtifact(wrapped)
	if err != nil {
		t.Fatalf("VerifyArtifact: %v", err)
	}
	if !result.Valid {
		t.Fatalf("expected valid signature, got error: %s", result.Error)
	}
	if result.SignerID != "demo-agent" {
		t.Fatalf("signerId = %q", result.SignerID)
	}
	if result.ArtifactType != "a2a-task" {
		t.Fatalf("artifactType = %q", result.ArtifactType)
	}
}

func TestAssessRemoteAgentTrustCasesFixture(t *testing.T) {
	a2a := mustA2AIntegration(t)
	casesFixture := loadA2AFixture(t, "trust_assessment_cases.json")
	casesRaw, ok := casesFixture["cases"].([]interface{})
	if !ok || len(casesRaw) == 0 {
		t.Fatalf("trust fixture missing cases")
	}

	for _, raw := range casesRaw {
		testCase, ok := raw.(map[string]interface{})
		if !ok {
			t.Fatalf("case is not object: %#v", raw)
		}

		policyStr, _ := testCase["policy"].(string)
		cardObj, _ := testCase["card"].(map[string]interface{})
		expectedObj, _ := testCase["expected"].(map[string]interface{})
		expectedAllowed, _ := expectedObj["allowed"].(bool)

		cardJSON, err := json.Marshal(cardObj)
		if err != nil {
			t.Fatalf("marshal card: %v", err)
		}
		got, err := a2a.AssessRemoteAgent(string(cardJSON), A2ATrustPolicy(policyStr))
		if err != nil {
			t.Fatalf("AssessRemoteAgent(%s): %v", policyStr, err)
		}
		if got.Allowed != expectedAllowed {
			t.Fatalf("policy=%s allowed=%v, want %v", policyStr, got.Allowed, expectedAllowed)
		}
	}
}

func TestRegisterOptionsWithAgentCardEmbedsA2AFields(t *testing.T) {
	a2a := mustA2AIntegration(t)

	cardFixture := loadA2AFixture(t, "agent_card.v10.json")
	cardJSON, err := json.Marshal(cardFixture)
	if err != nil {
		t.Fatalf("marshal card fixture: %v", err)
	}

	var card A2AAgentCard
	if err := json.Unmarshal(cardJSON, &card); err != nil {
		t.Fatalf("decode card fixture: %v", err)
	}

	opts := RegisterOptions{
		AgentJSON: `{"jacsId":"demo-agent","name":"Demo Agent"}`,
	}
	out, err := a2a.RegisterOptionsWithAgentCard(opts, &card)
	if err != nil {
		t.Fatalf("RegisterOptionsWithAgentCard: %v", err)
	}

	var merged map[string]interface{}
	if err := json.Unmarshal([]byte(out.AgentJSON), &merged); err != nil {
		t.Fatalf("decode merged agent_json: %v", err)
	}

	if _, ok := merged["a2aAgentCard"].(map[string]interface{}); !ok {
		t.Fatalf("missing a2aAgentCard in merged agent_json")
	}
	meta, ok := merged["metadata"].(map[string]interface{})
	if !ok {
		t.Fatalf("missing metadata object in merged agent_json")
	}
	if got := meta["a2aProfile"]; got != "1.0" {
		t.Fatalf("metadata.a2aProfile = %v", got)
	}
	if got := meta["a2aSkillsCount"]; got != float64(1) {
		t.Fatalf("metadata.a2aSkillsCount = %v", got)
	}
}
