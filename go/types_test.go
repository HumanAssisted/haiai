package haisdk

import (
	"encoding/json"
	"testing"
)

func TestAgentEventSerialization(t *testing.T) {
	event := AgentEvent{
		Type:       "benchmark_job",
		JobID:      "j-1",
		ScenarioID: "s-1",
		Config: &BenchmarkJobConfig{
			RunID:        "r-1",
			ScenarioName: "Test Scenario",
			Conversation: []ConversationTurn{
				{Speaker: "Alice", Message: "Hello", TurnNumber: 1},
			},
			RawMode:     false,
			TimeoutSecs: 60,
		},
	}

	data, err := json.Marshal(event)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	var parsed AgentEvent
	if err := json.Unmarshal(data, &parsed); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}

	if parsed.Type != "benchmark_job" {
		t.Errorf("expected type 'benchmark_job', got '%s'", parsed.Type)
	}
	if parsed.JobID != "j-1" {
		t.Errorf("expected job_id 'j-1', got '%s'", parsed.JobID)
	}
	if parsed.Config == nil {
		t.Fatal("config should not be nil")
	}
	if len(parsed.Config.Conversation) != 1 {
		t.Errorf("expected 1 conversation turn, got %d", len(parsed.Config.Conversation))
	}
}

func TestModerationResponseSerialization(t *testing.T) {
	ms := uint64(42)
	resp := ModerationResponse{
		Message:          "I understand both perspectives.",
		Metadata:         json.RawMessage(`{"confidence": 0.95}`),
		ProcessingTimeMs: &ms,
	}

	data, err := json.Marshal(resp)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	var parsed ModerationResponse
	if err := json.Unmarshal(data, &parsed); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}

	if parsed.Message != resp.Message {
		t.Errorf("message mismatch: '%s' vs '%s'", parsed.Message, resp.Message)
	}
	if parsed.ProcessingTimeMs == nil || *parsed.ProcessingTimeMs != 42 {
		t.Error("processing_time_ms should be 42")
	}
}

func TestHaiSignatureSerialization(t *testing.T) {
	sig := HaiSignature{
		KeyID:     "key-1",
		Algorithm: "Ed25519",
		Signature: "c2lnbmF0dXJl",
		SignedAt:  "2024-01-15T10:30:00Z",
	}

	data, err := json.Marshal(sig)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	var parsed HaiSignature
	if err := json.Unmarshal(data, &parsed); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}

	if parsed.KeyID != sig.KeyID {
		t.Errorf("KeyID mismatch: '%s' vs '%s'", parsed.KeyID, sig.KeyID)
	}
	if parsed.Algorithm != sig.Algorithm {
		t.Errorf("Algorithm mismatch: '%s' vs '%s'", parsed.Algorithm, sig.Algorithm)
	}
}

func TestTransportTypeConstants(t *testing.T) {
	if TransportSSE != "sse" {
		t.Errorf("expected TransportSSE 'sse', got '%s'", TransportSSE)
	}
	if TransportWS != "ws" {
		t.Errorf("expected TransportWS 'ws', got '%s'", TransportWS)
	}
}
