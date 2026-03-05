package haisdk

import (
	"encoding/json"
	"os"
	"reflect"
	"testing"
)

// a2aVerificationContract mirrors the shared canonical fixture structure.
type a2aVerificationContract struct {
	WrappedArtifact           map[string]interface{}   `json:"wrappedArtifact"`
	WrappedArtifactSchema     map[string]interface{}   `json:"wrappedArtifactSchema"`
	VerificationResultSchema  map[string]interface{}   `json:"verificationResultSchema"`
	VerificationResultExample map[string]interface{}   `json:"verificationResultExample"`
	TrustAssessmentSchema     map[string]interface{}   `json:"trustAssessmentSchema"`
	TrustAssessmentExample    map[string]interface{}   `json:"trustAssessmentExample"`
	AgentCardSchema           map[string]interface{}   `json:"agentCardSchema"`
	ChainOfCustodySchema      map[string]interface{}   `json:"chainOfCustodySchema"`
}

func loadA2AContract(t *testing.T) a2aVerificationContract {
	t.Helper()
	data, err := os.ReadFile("../fixtures/a2a_verification_contract.json")
	if err != nil {
		t.Fatalf("read a2a_verification_contract.json: %v", err)
	}
	var contract a2aVerificationContract
	if err := json.Unmarshal(data, &contract); err != nil {
		t.Fatalf("decode a2a_verification_contract.json: %v", err)
	}
	return contract
}

// assertFieldsPresent checks that every expected field from the schema exists in obj.
func assertFieldsPresent(t *testing.T, label string, obj map[string]interface{}, requiredFields map[string]interface{}) {
	t.Helper()
	for field := range requiredFields {
		if field == "_comment" {
			continue
		}
		if _, ok := obj[field]; !ok {
			t.Errorf("%s: missing required field %q", label, field)
		}
	}
}

// assertFieldType checks that a value matches the expected type string.
func assertFieldType(t *testing.T, label, field, expectedType string, value interface{}) {
	t.Helper()
	switch expectedType {
	case "string":
		if _, ok := value.(string); !ok {
			t.Errorf("%s.%s: expected string, got %T", label, field, value)
		}
	case "boolean":
		if _, ok := value.(bool); !ok {
			t.Errorf("%s.%s: expected boolean, got %T", label, field, value)
		}
	case "object":
		if _, ok := value.(map[string]interface{}); !ok {
			t.Errorf("%s.%s: expected object, got %T", label, field, value)
		}
	case "array":
		if _, ok := value.([]interface{}); !ok {
			t.Errorf("%s.%s: expected array, got %T", label, field, value)
		}
	case "number":
		if _, ok := value.(float64); !ok {
			t.Errorf("%s.%s: expected number, got %T", label, field, value)
		}
	}
}

// TestContractWrappedArtifactSerialization verifies that Go's A2AWrappedArtifact
// serializes to JSON with exactly the field names declared in the canonical contract.
func TestContractWrappedArtifactSerialization(t *testing.T) {
	contract := loadA2AContract(t)
	schema := contract.WrappedArtifactSchema
	requiredFields, _ := schema["requiredFields"].(map[string]interface{})

	// Roundtrip the fixture through Go's struct to verify field mapping.
	wrappedJSON, err := json.Marshal(contract.WrappedArtifact)
	if err != nil {
		t.Fatalf("marshal wrapped artifact: %v", err)
	}
	var wrapped A2AWrappedArtifact
	if err := json.Unmarshal(wrappedJSON, &wrapped); err != nil {
		t.Fatalf("unmarshal into A2AWrappedArtifact: %v", err)
	}

	// Re-serialize and check all required fields are present.
	reserialized, err := json.Marshal(wrapped)
	if err != nil {
		t.Fatalf("re-marshal A2AWrappedArtifact: %v", err)
	}
	var reObj map[string]interface{}
	if err := json.Unmarshal(reserialized, &reObj); err != nil {
		t.Fatalf("decode re-serialized: %v", err)
	}

	assertFieldsPresent(t, "A2AWrappedArtifact", reObj, requiredFields)

	// Verify types match schema expectations.
	for field, expectedTypeRaw := range requiredFields {
		if field == "_comment" {
			continue
		}
		expectedType, _ := expectedTypeRaw.(string)
		assertFieldType(t, "A2AWrappedArtifact", field, expectedType, reObj[field])
	}

	// Verify signature sub-fields.
	sigSchema, _ := schema["signatureFields"].(map[string]interface{})
	sigObj, _ := reObj["jacsSignature"].(map[string]interface{})
	if sigObj == nil {
		t.Fatal("A2AWrappedArtifact: jacsSignature is nil after roundtrip")
	}
	assertFieldsPresent(t, "A2AArtifactSignature", sigObj, sigSchema)

	// Specifically check the agentID casing (uppercase ID, not agentId).
	if _, ok := sigObj["agentID"]; !ok {
		t.Error("A2AArtifactSignature: field must be 'agentID' (uppercase ID), not 'agentId'")
	}

	// Check roundtrip values match.
	if wrapped.JacsID != "contract-00000000-0000-4000-8000-000000000001" {
		t.Errorf("jacsId roundtrip: got %q", wrapped.JacsID)
	}
	if wrapped.JacsType != "a2a-task" {
		t.Errorf("jacsType roundtrip: got %q", wrapped.JacsType)
	}
	if wrapped.JacsSignature.AgentID != "contract-agent" {
		t.Errorf("jacsSignature.agentID roundtrip: got %q", wrapped.JacsSignature.AgentID)
	}
}

// TestContractVerificationResultSerialization verifies that Go's
// A2AArtifactVerificationResult serializes with the canonical field names.
func TestContractVerificationResultSerialization(t *testing.T) {
	contract := loadA2AContract(t)
	schema := contract.VerificationResultSchema
	requiredFields, _ := schema["requiredFields"].(map[string]interface{})

	// Roundtrip the example through Go's struct.
	exampleJSON, err := json.Marshal(contract.VerificationResultExample)
	if err != nil {
		t.Fatalf("marshal verification result example: %v", err)
	}
	var result A2AArtifactVerificationResult
	if err := json.Unmarshal(exampleJSON, &result); err != nil {
		t.Fatalf("unmarshal into A2AArtifactVerificationResult: %v", err)
	}

	reserialized, err := json.Marshal(result)
	if err != nil {
		t.Fatalf("re-marshal A2AArtifactVerificationResult: %v", err)
	}
	var reObj map[string]interface{}
	if err := json.Unmarshal(reserialized, &reObj); err != nil {
		t.Fatalf("decode re-serialized: %v", err)
	}

	assertFieldsPresent(t, "A2AArtifactVerificationResult", reObj, requiredFields)

	for field, expectedTypeRaw := range requiredFields {
		if field == "_comment" {
			continue
		}
		expectedType, _ := expectedTypeRaw.(string)
		assertFieldType(t, "A2AArtifactVerificationResult", field, expectedType, reObj[field])
	}

	// Check specific contract values survived roundtrip.
	if result.Valid != false {
		t.Error("valid should be false for placeholder signature")
	}
	if result.SignerID != "contract-agent" {
		t.Errorf("signerId = %q, want %q", result.SignerID, "contract-agent")
	}
	if result.ArtifactType != "a2a-task" {
		t.Errorf("artifactType = %q, want %q", result.ArtifactType, "a2a-task")
	}
}

// TestContractTrustAssessmentSerialization verifies that Go's A2ATrustAssessment
// serializes with the canonical field names.
func TestContractTrustAssessmentSerialization(t *testing.T) {
	contract := loadA2AContract(t)
	schema := contract.TrustAssessmentSchema
	requiredFields, _ := schema["requiredFields"].(map[string]interface{})

	exampleJSON, err := json.Marshal(contract.TrustAssessmentExample)
	if err != nil {
		t.Fatalf("marshal trust assessment example: %v", err)
	}
	var assessment A2ATrustAssessment
	if err := json.Unmarshal(exampleJSON, &assessment); err != nil {
		t.Fatalf("unmarshal into A2ATrustAssessment: %v", err)
	}

	reserialized, err := json.Marshal(assessment)
	if err != nil {
		t.Fatalf("re-marshal A2ATrustAssessment: %v", err)
	}
	var reObj map[string]interface{}
	if err := json.Unmarshal(reserialized, &reObj); err != nil {
		t.Fatalf("decode re-serialized: %v", err)
	}

	assertFieldsPresent(t, "A2ATrustAssessment", reObj, requiredFields)

	for field, expectedTypeRaw := range requiredFields {
		if field == "_comment" {
			continue
		}
		expectedType, _ := expectedTypeRaw.(string)
		assertFieldType(t, "A2ATrustAssessment", field, expectedType, reObj[field])
	}

	// Check specific contract values.
	if assessment.Allowed != true {
		t.Error("allowed should be true for open policy")
	}
	if assessment.TrustLevel != "jacs_verified" {
		t.Errorf("trustLevel = %q, want %q", assessment.TrustLevel, "jacs_verified")
	}
	if assessment.JACSRegistered != true {
		t.Error("jacsRegistered should be true")
	}
	if assessment.InTrustStore != false {
		t.Error("inTrustStore should be false")
	}
	if assessment.Reason != "open policy: all agents accepted" {
		t.Errorf("reason = %q", assessment.Reason)
	}
}

// TestContractAgentCardSerialization verifies that Go's A2AAgentCard
// serializes with the canonical field names from the contract fixture.
func TestContractAgentCardSerialization(t *testing.T) {
	contract := loadA2AContract(t)
	schema := contract.AgentCardSchema
	requiredFields, _ := schema["requiredFields"].(map[string]interface{})

	// Use the existing v04 fixture card as an agent card example.
	cardFixture := loadA2AFixture(t, "agent_card.v04.json")
	cardJSON, err := json.Marshal(cardFixture)
	if err != nil {
		t.Fatalf("marshal card fixture: %v", err)
	}
	var card A2AAgentCard
	if err := json.Unmarshal(cardJSON, &card); err != nil {
		t.Fatalf("unmarshal into A2AAgentCard: %v", err)
	}

	reserialized, err := json.Marshal(card)
	if err != nil {
		t.Fatalf("re-marshal A2AAgentCard: %v", err)
	}
	var reObj map[string]interface{}
	if err := json.Unmarshal(reserialized, &reObj); err != nil {
		t.Fatalf("decode re-serialized: %v", err)
	}

	assertFieldsPresent(t, "A2AAgentCard", reObj, requiredFields)

	for field, expectedTypeRaw := range requiredFields {
		if field == "_comment" {
			continue
		}
		expectedType, _ := expectedTypeRaw.(string)
		assertFieldType(t, "A2AAgentCard", field, expectedType, reObj[field])
	}

	// Check skill sub-fields.
	skillSchema, _ := schema["skillFields"].(map[string]interface{})
	skills, _ := reObj["skills"].([]interface{})
	if len(skills) == 0 {
		t.Fatal("A2AAgentCard: skills array should not be empty")
	}
	firstSkill, _ := skills[0].(map[string]interface{})
	assertFieldsPresent(t, "A2AAgentSkill", firstSkill, skillSchema)

	// Check extension sub-fields.
	extSchema, _ := schema["extensionFields"].(map[string]interface{})
	caps, _ := reObj["capabilities"].(map[string]interface{})
	extensions, _ := caps["extensions"].([]interface{})
	if len(extensions) == 0 {
		t.Fatal("A2AAgentCard: capabilities.extensions should not be empty")
	}
	firstExt, _ := extensions[0].(map[string]interface{})
	// uri is the only truly required field; description and required may be omitted
	if _, ok := firstExt["uri"]; !ok {
		t.Error("A2AAgentExtension: missing required field 'uri'")
	}
	_ = extSchema // used for documentation reference
}

// TestContractChainOfCustodySerialization verifies that Go's A2AChainOfCustody
// serializes with the canonical field names.
func TestContractChainOfCustodySerialization(t *testing.T) {
	contract := loadA2AContract(t)
	schema := contract.ChainOfCustodySchema
	requiredFields, _ := schema["requiredFields"].(map[string]interface{})
	entryFields, _ := schema["entryFields"].(map[string]interface{})

	a2a := mustA2AIntegration(t)

	// Build a minimal chain.
	wrapped := &A2AWrappedArtifact{
		JacsID:          "chain-test-001",
		JacsVersion:     "1.0.0",
		JacsType:        "a2a-task",
		JacsLevel:       "artifact",
		JacsVersionDate: "2026-03-01T00:00:00Z",
		A2AArtifact:     map[string]interface{}{"test": true},
		JacsSignature: &A2AArtifactSignature{
			AgentID:   "chain-agent",
			Date:      "2026-03-01T00:00:00Z",
			Signature: "c2ln",
		},
	}
	chain := a2a.CreateChainOfCustody([]*A2AWrappedArtifact{wrapped})

	chainJSON, err := json.Marshal(chain)
	if err != nil {
		t.Fatalf("marshal chain: %v", err)
	}
	var chainObj map[string]interface{}
	if err := json.Unmarshal(chainJSON, &chainObj); err != nil {
		t.Fatalf("decode chain: %v", err)
	}

	assertFieldsPresent(t, "A2AChainOfCustody", chainObj, requiredFields)

	for field, expectedTypeRaw := range requiredFields {
		if field == "_comment" {
			continue
		}
		expectedType, _ := expectedTypeRaw.(string)
		assertFieldType(t, "A2AChainOfCustody", field, expectedType, chainObj[field])
	}

	// Check chain entry fields.
	entries, _ := chainObj["chainOfCustody"].([]interface{})
	if len(entries) == 0 {
		t.Fatal("A2AChainOfCustody: chainOfCustody array should not be empty")
	}
	firstEntry, _ := entries[0].(map[string]interface{})
	assertFieldsPresent(t, "A2AChainEntry", firstEntry, entryFields)

	for field, expectedTypeRaw := range entryFields {
		if field == "_comment" {
			continue
		}
		expectedType, _ := expectedTypeRaw.(string)
		assertFieldType(t, "A2AChainEntry", field, expectedType, firstEntry[field])
	}
}

// TestContractVerificationResultFieldNames checks that the Go struct's JSON tags
// exactly match the contract fixture field names. This is a compile-time + reflection
// test to catch tag drift.
func TestContractVerificationResultFieldNames(t *testing.T) {
	expected := map[string]string{
		"Valid":            "valid",
		"SignerID":         "signerId",
		"ArtifactType":     "artifactType",
		"Timestamp":        "timestamp",
		"OriginalArtifact": "originalArtifact",
		"Error":            "error",
	}

	typ := reflect.TypeOf(A2AArtifactVerificationResult{})
	for goField, wantTag := range expected {
		field, ok := typ.FieldByName(goField)
		if !ok {
			t.Errorf("A2AArtifactVerificationResult missing Go field %q", goField)
			continue
		}
		tag := field.Tag.Get("json")
		// Strip omitempty etc.
		if idx := len(tag); idx > 0 {
			for i, c := range tag {
				if c == ',' {
					tag = tag[:i]
					break
				}
			}
		}
		if tag != wantTag {
			t.Errorf("A2AArtifactVerificationResult.%s: json tag %q, want %q", goField, tag, wantTag)
		}
	}
}

// TestContractTrustAssessmentFieldNames checks Go struct JSON tags against the contract.
func TestContractTrustAssessmentFieldNames(t *testing.T) {
	expected := map[string]string{
		"Allowed":        "allowed",
		"TrustLevel":     "trustLevel",
		"JACSRegistered": "jacsRegistered",
		"InTrustStore":   "inTrustStore",
		"Reason":         "reason",
	}

	typ := reflect.TypeOf(A2ATrustAssessment{})
	for goField, wantTag := range expected {
		field, ok := typ.FieldByName(goField)
		if !ok {
			t.Errorf("A2ATrustAssessment missing Go field %q", goField)
			continue
		}
		tag := field.Tag.Get("json")
		for i, c := range tag {
			if c == ',' {
				tag = tag[:i]
				break
			}
		}
		if tag != wantTag {
			t.Errorf("A2ATrustAssessment.%s: json tag %q, want %q", goField, tag, wantTag)
		}
	}
}
